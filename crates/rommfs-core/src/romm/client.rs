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
use std::io::Write;
use std::time::Duration;

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
    token: std::sync::RwLock<Option<String>>,
}

impl RommClient {
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        let base_url = base_url.into();
        if base_url.trim().is_empty() {
            return Err(Error::InvalidCatalogue("empty server URL".into()));
        }
        todo!("normalize base url (strip trailing /), build isahc client")
    }

    pub fn with_download_config(mut self, cfg: DownloadConfig) -> Self {
        self.download = cfg;
        self
    }

    /// OAuth2 password grant. On success the token is held in memory and sent
    /// as `Authorization: Bearer` on every subsequent call.
    pub fn authenticate(&self, creds: Credentials<'_>) -> Result<()> {
        let _ = creds;
        todo!("POST base/api/token form; map 401->Auth, 403->Forbidden; store access_token")
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
        todo!()
    }

    /// One page of `GET /api/roms?with_files=true&limit=&offset=`.
    pub fn roms_page(&self, limit: u64, offset: u64) -> Result<RomsPage> {
        let _ = (limit, offset);
        todo!()
    }

    /// The complete ROM list across every page needed.
    pub fn all_roms(&self) -> Result<Vec<RomDto>> {
        todo!("loop roms_page with page size 200 until total reached or short/empty page")
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
        let _ = (rom_id, file_name, writer, progress);
        todo!("percent-encode file_name; stream chunks via isahc with low_speed_timeout")
    }
}
