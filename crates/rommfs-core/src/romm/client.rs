//! Blocking RomM client against the verified 5.3.1 contract.
//!
//! - `POST /api/token` (form: grant_type=password, username, password,
//!   scope="platforms.read roms.read") -> Bearer token kept in memory only.
//! - `GET /api/platforms` -> [PlatformDto]
//! - `GET /api/roms?limit=&offset=&with_files=true` -> RomsPage; `all_roms`
//!   pages until the catalogue is complete.
//! - `GET /api/roms/{id}/content/{file_name}` -> streamed body into a caller
//!   writer with progress callback; connect-timeout + low-speed (no progress)
//!   timeout, never a total timeout (PRD R3).
//!
//! Errors: 401 -> Error::Auth, 403 -> Error::Forbidden, other non-2xx ->
//! Error::Http, connect/stream failures -> Error::Transport, short body ->
//! Error::Truncated. Credentials must never appear in any error/log string.

use crate::error::{Error, Result};
use crate::romm::types::*;
use isahc::prelude::*;
use std::io::{Read, Write};
use std::time::Duration;

/// Page size used by `all_roms`.
const ROMS_PAGE_LIMIT: u64 = 200;

/// Username/password pair; kept in memory only, never logged or persisted.
pub struct Credentials<'a> {
    pub username: &'a str,
    pub password: &'a str,
}

/// Timeouts and read chunking for content downloads.
#[derive(Clone, Copy, Debug)]
pub struct DownloadConfig {
    /// Time to establish the connection.
    pub connect_timeout: Duration,
    /// Abort when fewer than `low_speed_bytes_per_sec` bytes arrive within
    /// `low_speed_window` (the "no progress" kill — not a total timeout).
    pub low_speed_bytes_per_sec: u64,
    pub low_speed_window: Duration,
    /// Reader chunk size while streaming to disk.
    pub chunk_bytes: usize,
}

/// Connection and total response limits for authentication and catalogue calls.
/// Content downloads use `DownloadConfig` instead so progressing large ROMs are
/// not subject to a total transfer deadline.
#[derive(Clone, Copy, Debug)]
pub struct MetadataConfig {
    /// Time to establish the connection.
    pub connect_timeout: Duration,
    /// Maximum time for the request and complete response body.
    pub response_timeout: Duration,
}

impl Default for MetadataConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(15),
            response_timeout: Duration::from_secs(60),
        }
    }
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(15),
            low_speed_bytes_per_sec: 1024,
            low_speed_window: Duration::from_secs(30),
            chunk_bytes: 256 * 1024,
        }
    }
}

/// A client bound to one base URL + one in-memory token.
pub struct RommClient {
    base_url: String,
    download: DownloadConfig,
    metadata: MetadataConfig,
    token: std::sync::RwLock<Option<String>>,
}

impl RommClient {
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        let base_url = base_url.into();
        if base_url.trim().is_empty() {
            return Err(Error::InvalidCatalogue("empty server URL".into()));
        }
        let base_url = base_url.trim().trim_end_matches('/').to_string();
        // Validate the URL shape early so catalogue calls never build a bad
        // request mid-mount.
        isahc::Request::get(&base_url)
            .body(())
            .map_err(|_| Error::InvalidCatalogue("invalid server URL".into()))?;
        Ok(Self {
            base_url,
            download: DownloadConfig::default(),
            metadata: MetadataConfig::default(),
            token: std::sync::RwLock::new(None),
        })
    }

    pub fn with_download_config(mut self, cfg: DownloadConfig) -> Self {
        self.download = cfg;
        self
    }

    pub fn with_metadata_config(mut self, cfg: MetadataConfig) -> Self {
        self.metadata = cfg;
        self
    }

    /// OAuth2 password grant. On success the token is held in memory and sent
    /// as `Authorization: Bearer` on every subsequent call.
    pub fn authenticate(&self, creds: Credentials<'_>) -> Result<()> {
        let body = format!(
            "grant_type=password&username={}&password={}&scope=platforms.read+roms.read",
            form_encode(creds.username),
            form_encode(creds.password)
        );
        let request = isahc::Request::post(format!("{}/api/token", self.base_url))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .connect_timeout(self.metadata.connect_timeout)
            .timeout(self.metadata.response_timeout)
            .body(body)
            .map_err(|e| Error::Transport(format!("request build failed: {e}")))?;
        let mut resp = request.send().map_err(transport)?;
        match resp.status().as_u16() {
            s if (200..300).contains(&s) => {
                let token: TokenResponse = resp.json().map_err(invalid_payload)?;
                *self.token.write().unwrap() = Some(token.access_token);
                Ok(())
            }
            401 => Err(Error::Auth("credentials rejected by server".into())),
            403 => Err(Error::Forbidden(detail_of(&mut resp))),
            s => Err(Error::Http {
                status: s,
                message: detail_of(&mut resp),
            }),
        }
    }

    /// Clear the held token (sign-out / after an Auth failure surfaces).
    pub fn clear_token(&self) {
        *self.token.write().unwrap() = None;
    }

    /// Whether a token is currently held (informational for UI state).
    pub fn has_token(&self) -> bool {
        self.token.read().unwrap().is_some()
    }

    /// `GET /api/platforms` — all accessible platforms.
    pub fn platforms(&self) -> Result<Vec<PlatformDto>> {
        self.get_json("/api/platforms")
    }

    /// One page of `GET /api/roms?with_files=true&limit=&offset=`.
    pub fn roms_page(&self, limit: u64, offset: u64) -> Result<RomsPage> {
        self.get_json(&format!(
            "/api/roms?limit={limit}&offset={offset}&with_files=true"
        ))
    }

    /// The complete ROM list across every page needed.
    pub fn all_roms(&self) -> Result<Vec<RomDto>> {
        let mut out = Vec::new();
        let mut offset = 0u64;
        loop {
            let page = self.roms_page(ROMS_PAGE_LIMIT, offset)?;
            let got = page.items.len() as u64;
            out.extend(page.items);
            offset += got;
            // Stop when the declared total is covered or the page came back
            // short/empty (per the verified contract).
            let done = page.total.is_some_and(|t| offset >= t) || got < ROMS_PAGE_LIMIT;
            if done || got == 0 {
                break;
            }
        }
        Ok(out)
    }

    /// Stream `GET /api/roms/{rom_id}/content/{file_name}` into `writer`.
    ///
    /// `progress(received, total)` is invoked per chunk; `total` is the
    /// Content-Length when the server supplies one. Must return Err on
    /// non-2xx, short/truncated body, write failure, or no-progress stall.
    /// Never returns success for a partial transfer.
    pub fn download_file(
        &self,
        rom_id: i64,
        file_name: &str,
        writer: &mut dyn Write,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<u64> {
        let url = format!(
            "{}/api/roms/{}/content/{}",
            self.base_url,
            rom_id,
            path_encode(file_name)
        );
        let mut builder = isahc::Request::get(url)
            .connect_timeout(self.download.connect_timeout)
            .low_speed_timeout(
                self.download.low_speed_bytes_per_sec.min(u32::MAX as u64) as u32,
                self.download.low_speed_window,
            );
        if let Some(t) = self.token.read().unwrap().clone() {
            builder = builder.header("Authorization", format!("Bearer {t}"));
        }
        let request = builder
            .body(())
            .map_err(|e| Error::Transport(format!("request build failed: {e}")))?;
        let mut resp = request.send().map_err(transport)?;
        match resp.status().as_u16() {
            s if (200..300).contains(&s) => {}
            401 => {
                self.clear_token();
                return Err(Error::Auth("session expired or token rejected".into()));
            }
            403 => return Err(Error::Forbidden(detail_of(&mut resp))),
            s => {
                return Err(Error::Http {
                    status: s,
                    message: detail_of(&mut resp),
                });
            }
        }

        let total = resp
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());
        let mut received = 0u64;
        let mut buf = vec![0u8; self.download.chunk_bytes.max(1)];
        let body = resp.body_mut();
        loop {
            match body.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    writer.write_all(&buf[..n]).map_err(Error::Io)?;
                    received += n as u64;
                    progress(received, total);
                }
                Err(e) => {
                    // Body-read failures are wrapped isahc errors: a clean
                    // early close lands as partial-file (io), a stall lands
                    // as the no-progress timeout.
                    let e = isahc::Error::from(e);
                    if e.is_timeout() {
                        return Err(Error::Transport(format!("transfer stalled: {e}")));
                    }
                    if let Some(t) = total {
                        if received < t {
                            return Err(Error::Truncated {
                                expected: t,
                                received,
                            });
                        }
                    }
                    return Err(transport(e));
                }
            }
        }
        if let Some(t) = total {
            if received < t {
                return Err(Error::Truncated {
                    expected: t,
                    received,
                });
            }
        }
        Ok(received)
    }

    /// Authenticated `GET` whose body is parsed as the documented JSON shape.
    /// A mid-session 401 clears the token (sign-in is required again).
    fn get_json<T: serde::de::DeserializeOwned>(&self, path_and_query: &str) -> Result<T> {
        let mut builder = isahc::Request::get(format!("{}{path_and_query}", self.base_url));
        if let Some(t) = self.token.read().unwrap().clone() {
            builder = builder.header("Authorization", format!("Bearer {t}"));
        }
        let request = builder
            .connect_timeout(self.metadata.connect_timeout)
            .timeout(self.metadata.response_timeout)
            .body(())
            .map_err(|e| Error::Transport(format!("request build failed: {e}")))?;
        let mut resp = request.send().map_err(transport)?;
        match resp.status().as_u16() {
            s if (200..300).contains(&s) => resp.json().map_err(invalid_payload),
            401 => {
                self.clear_token();
                Err(Error::Auth("session expired or token rejected".into()))
            }
            403 => Err(Error::Forbidden(detail_of(&mut resp))),
            s => Err(Error::Http {
                status: s,
                message: detail_of(&mut resp),
            }),
        }
    }
}

/// Network/connect/stream failures all surface as Transport; messages carry
/// no credentials (the token only ever travels in the Authorization header).
fn transport(e: isahc::Error) -> Error {
    Error::Transport(e.to_string())
}

fn invalid_payload(e: serde_json::Error) -> Error {
    Error::InvalidCatalogue(e.to_string())
}

/// Short response-body detail for error reporting, credentials excluded.
fn detail_of(resp: &mut isahc::Response<isahc::Body>) -> String {
    match resp.text() {
        Ok(t) => t.chars().take(200).collect(),
        Err(_) => "no response body".to_string(),
    }
}

/// `application/x-www-form-urlencoded` value encoding: unreserved characters
/// are kept, everything else is percent-encoded at the UTF-8 byte level.
fn form_encode(s: &str) -> String {
    pct_encode(s)
}

/// Percent-encode one URL path segment (the `file_name` of a content URL).
fn path_encode(s: &str) -> String {
    pct_encode(s)
}

fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                out.push('%');
                out.push(HEX[(b >> 4) as usize] as char);
                out.push(HEX[(b & 0xf) as usize] as char);
            }
        }
    }
    out
}
