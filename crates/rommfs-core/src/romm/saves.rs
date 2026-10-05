use super::client::{transport, RommClient};
use crate::error::{Error, Result};
use isahc::prelude::*;
use serde::Deserialize;
use std::io::Read;

const MAX_REMOTE_SAVE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveSyncIdentity {
    pub account_id: i64,
    pub scopes: Vec<String>,
}

impl SaveSyncIdentity {
    pub fn can_read_saves(&self) -> bool {
        self.scopes.iter().any(|scope| scope == "assets.read")
    }

    pub fn can_write_saves(&self) -> bool {
        self.scopes.iter().any(|scope| scope == "assets.write")
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct RemoteSave {
    pub id: i64,
    pub rom_id: i64,
    pub user_id: i64,
    pub file_name: String,
    pub file_size_bytes: u64,
    #[serde(default)]
    pub missing_from_fs: bool,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub emulator: Option<String>,
    #[serde(default)]
    pub slot: Option<String>,
}

/// HTTP error details kept separate from the ROM client's general error type
/// so the upload worker can honor the API's `Retry-After` header.
#[derive(Debug)]
pub struct SaveApiFailure {
    pub error: Error,
    pub retry_after: Option<String>,
}

impl std::fmt::Display for SaveApiFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for SaveApiFailure {}

#[derive(Deserialize)]
struct MeUser {
    id: i64,
    oauth_scopes: Vec<String>,
}

impl RommClient {
    pub fn save_sync_identity(&self) -> Result<Option<SaveSyncIdentity>> {
        let user: Option<MeUser> = self.get_json("/api/users/me")?;
        Ok(user.map(|user| SaveSyncIdentity {
            account_id: user.id,
            scopes: user.oauth_scopes,
        }))
    }

    pub fn save_inventory(
        &self,
        rom_id: i64,
        slot: &str,
    ) -> std::result::Result<Vec<RemoteSave>, SaveApiFailure> {
        let path = format!("/api/saves?rom_id={rom_id}&slot={}", query_encode(slot));
        let request = isahc::Request::get(format!("{}{path}", self.base_url))
            .body(())
            .map_err(|error| SaveApiFailure {
                error: Error::Transport(format!("request build failed: {error}")),
                retry_after: None,
            })?;
        let mut response = self.save_request(request).map_err(save_failure)?;
        match response.status().as_u16() {
            status if (200..300).contains(&status) => {
                response.json().map_err(|error| SaveApiFailure {
                    error: Error::InvalidCatalogue(error.to_string()),
                    retry_after: None,
                })
            }
            status => Err(self.response_failure(&mut response, status)),
        }
    }

    pub fn upload_save(
        &self,
        rom_id: i64,
        slot: &str,
        emulator: &str,
        filename: &str,
        bytes: &[u8],
    ) -> std::result::Result<RemoteSave, SaveApiFailure> {
        if bytes.is_empty() || bytes.len() > MAX_REMOTE_SAVE_BYTES {
            return Err(SaveApiFailure {
                error: Error::Unsupported("save upload size is outside the supported limit".into()),
                retry_after: None,
            });
        }
        if filename.contains(['\r', '\n', '"']) {
            return Err(SaveApiFailure {
                error: Error::Unsupported("save upload filename is invalid".into()),
                retry_after: None,
            });
        }
        let boundary = multipart_boundary(bytes);
        let mut body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"saveFile\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .into_bytes();
        body.extend_from_slice(bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let url = format!(
            "{}/api/saves?rom_id={rom_id}&slot={}&emulator={}&overwrite=false&autocleanup=false",
            self.base_url,
            query_encode(slot),
            query_encode(emulator),
        );
        let request = isahc::Request::post(url)
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body)
            .map_err(|error| SaveApiFailure {
                error: Error::Transport(format!("request build failed: {error}")),
                retry_after: None,
            })?;
        let mut response = self.save_request(request).map_err(save_failure)?;
        match response.status().as_u16() {
            status if (200..300).contains(&status) => {
                response.json().map_err(|error| SaveApiFailure {
                    error: Error::InvalidCatalogue(error.to_string()),
                    retry_after: None,
                })
            }
            status => Err(self.response_failure(&mut response, status)),
        }
    }

    pub fn download_save_content(
        &self,
        save_id: i64,
    ) -> std::result::Result<Vec<u8>, SaveApiFailure> {
        let url = format!(
            "{}/api/saves/{save_id}/content?optimistic=false",
            self.base_url
        );
        let request = isahc::Request::get(url)
            .body(())
            .map_err(|error| SaveApiFailure {
                error: Error::Transport(format!("request build failed: {error}")),
                retry_after: None,
            })?;
        let mut response = self.save_request(request).map_err(save_failure)?;
        match response.status().as_u16() {
            status if (200..300).contains(&status) => {
                let mut bytes = Vec::new();
                response
                    .body_mut()
                    .take((MAX_REMOTE_SAVE_BYTES + 1) as u64)
                    .read_to_end(&mut bytes)
                    .map_err(|error| SaveApiFailure {
                        error: Error::Transport(error.to_string()),
                        retry_after: None,
                    })?;
                if bytes.is_empty() || bytes.len() > MAX_REMOTE_SAVE_BYTES {
                    return Err(SaveApiFailure {
                        error: Error::Unsupported("remote save content has an invalid size".into()),
                        retry_after: None,
                    });
                }
                Ok(bytes)
            }
            status => Err(self.response_failure(&mut response, status)),
        }
    }

    fn save_request<T: Into<isahc::Body>>(
        &self,
        request: isahc::http::Request<T>,
    ) -> std::result::Result<isahc::Response<isahc::Body>, isahc::Error> {
        let (parts, body) = request.into_parts();
        let mut builder = isahc::Request::builder()
            .method(parts.method)
            .uri(parts.uri)
            .version(parts.version)
            .connect_timeout(self.metadata.connect_timeout)
            .timeout(self.metadata.response_timeout);
        *builder.headers_mut().expect("request builder is valid") = parts.headers;
        if let Some(token) = self.token.read().unwrap().clone() {
            builder = builder.header("Authorization", format!("Bearer {token}"));
        }
        builder.body(body).map_err(isahc::Error::from)?.send()
    }

    fn response_failure(
        &self,
        response: &mut isahc::Response<isahc::Body>,
        status: u16,
    ) -> SaveApiFailure {
        if status == 401 {
            self.clear_token();
        }
        response_failure(response, status)
    }
}

fn save_failure(error: isahc::Error) -> SaveApiFailure {
    SaveApiFailure {
        error: transport(error),
        retry_after: None,
    }
}

fn response_failure(response: &mut isahc::Response<isahc::Body>, status: u16) -> SaveApiFailure {
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let error = match status {
        401 => Error::Auth("session expired or token rejected".into()),
        403 => Error::Forbidden(response_detail(response)),
        _ => Error::Http {
            status,
            message: response_detail(response),
        },
    };
    SaveApiFailure { error, retry_after }
}

fn response_detail(response: &mut isahc::Response<isahc::Body>) -> String {
    response
        .text()
        .unwrap_or_else(|_| "no response body".into())
        .chars()
        .take(200)
        .collect()
}

fn query_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn multipart_boundary(bytes: &[u8]) -> String {
    loop {
        let boundary = format!("rommfs-save-{}", uuid::Uuid::new_v4());
        if !bytes
            .windows(boundary.len())
            .any(|part| part == boundary.as_bytes())
        {
            return boundary;
        }
    }
}
