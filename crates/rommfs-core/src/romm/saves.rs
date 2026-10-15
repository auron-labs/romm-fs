use super::client::{transport, RommClient};
use crate::error::{Error, Result};
use futures_lite::io::AsyncReadExt;
use isahc::prelude::*;
use serde::Deserialize;
use std::future::Future;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

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
        let path = if slot.is_empty() {
            format!("/api/saves?rom_id={rom_id}")
        } else {
            format!("/api/saves?rom_id={rom_id}&slot={}", query_encode(slot))
        };
        self.save_inventory_path(&path)
    }
    pub fn all_save_inventory(&self) -> std::result::Result<Vec<RemoteSave>, SaveApiFailure> {
        self.save_inventory_path("/api/saves?")
    }
    fn save_inventory_path(
        &self,
        path: &str,
    ) -> std::result::Result<Vec<RemoteSave>, SaveApiFailure> {
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

    /// Upload an immutable snapshot without buffering its contents in memory.
    /// Slot carries the original relative path; filenames may be rewritten by
    /// RomM and identify only this immutable revision, not a local destination.
    pub fn upload_save_file(
        &self,
        rom_id: i64,
        slot: &str,
        filename: &str,
        path: &Path,
        active: &std::sync::Arc<AtomicBool>,
    ) -> std::result::Result<RemoteSave, SaveApiFailure> {
        crate::save_sync::path_from_file_slot(slot).map_err(local_failure)?;
        if filename.is_empty() || filename.contains(['\r', '\n', '"', '/', '\\']) {
            return Err(local_failure(Error::Unsupported(
                "Invalid save upload filename".into(),
            )));
        }
        crate::save_sync::ensure_no_reparse_components(path).map_err(local_failure)?;
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1);
        }
        let file = options.open(path).map_err(|e| local_failure(e.into()))?;
        let metadata = file.metadata().map_err(|e| local_failure(e.into()))?;
        if !metadata.is_file() {
            return Err(local_failure(Error::Unsupported(
                "Save snapshot is not an ordinary file".into(),
            )));
        }
        let size = metadata.len();
        let boundary = format!("rommfs-save-{}", uuid::Uuid::new_v4());
        let prefix = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"saveFile\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n");
        let suffix = format!("\r\n--{boundary}--\r\n");
        let length = size
            .checked_add((prefix.len() + suffix.len()) as u64)
            .ok_or_else(|| {
                local_failure(Error::Unsupported("Save upload length overflow".into()))
            })?;
        if !active.load(Ordering::Acquire) {
            return Err(local_failure(Error::Cancelled));
        }
        let reader = std::io::Cursor::new(prefix.into_bytes())
            .chain(file)
            .chain(std::io::Cursor::new(suffix.into_bytes()));
        let reader = CancellableReader {
            inner: reader,
            active: active.clone(),
        };
        let request = isahc::Request::post(format!("{}/api/saves?rom_id={rom_id}&slot={}&emulator=rommfs&overwrite=false&autocleanup=false", self.base_url, query_encode(slot)))
            .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
            .body(isahc::AsyncBody::from_reader_sized(blocking::Unblock::new(reader), length))
            .map_err(|_| local_failure(Error::Transport("Could not build save upload request".into())))?;
        let mut response = self.save_content_request(request, active)?;
        if response.status().is_success() {
            wait_for_save(response.json(), active)?
                .map_err(|e| local_failure(Error::InvalidCatalogue(e.to_string())))
        } else {
            let status = response.status().as_u16();
            Err(self.content_response_failure(&mut response, status, active)?)
        }
    }

    /// Stream exactly the inventoried bytes, including a valid zero-length
    /// file, into a private staging writer. No emulator-format or 1 MiB cap.
    pub fn download_save_to(
        &self,
        save_id: i64,
        size: u64,
        writer: &mut impl Write,
        active: &AtomicBool,
    ) -> std::result::Result<(), SaveApiFailure> {
        if !active.load(Ordering::Acquire) {
            return Err(local_failure(Error::Cancelled));
        }
        let request = isahc::Request::get(format!(
            "{}/api/saves/{save_id}/content?optimistic=false",
            self.base_url
        ))
        .body(())
        .map_err(|_| {
            local_failure(Error::Transport(
                "Could not build save download request".into(),
            ))
        })?;
        let mut response = self.save_content_request(request, active)?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            return Err(self.content_response_failure(&mut response, status, active)?);
        }
        let mut received = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            if !active.load(Ordering::Acquire) {
                return Err(local_failure(Error::Cancelled));
            }
            let count = wait_for_save(response.body_mut().read(&mut buffer), active)?
                .map_err(|e| local_failure(e.into()))?;
            if count == 0 {
                break;
            }
            received = received
                .checked_add(count as u64)
                .ok_or_else(|| local_failure(Error::Unsupported("Save size overflow".into())))?;
            if received > size {
                return Err(local_failure(Error::Unsupported(
                    "Remote save exceeds its inventoried size".into(),
                )));
            }
            writer
                .write_all(&buffer[..count])
                .map_err(|e| local_failure(e.into()))?;
        }
        if received != size {
            return Err(local_failure(Error::Truncated {
                expected: size,
                received,
            }));
        }
        Ok(())
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

    pub(super) fn save_request<T: Into<isahc::Body>>(
        &self,
        request: isahc::http::Request<T>,
    ) -> std::result::Result<isahc::Response<isahc::Body>, isahc::Error> {
        self.build_save_request(request, false)?.send()
    }
    pub(super) fn save_content_request<T: Into<isahc::AsyncBody>>(
        &self,
        request: isahc::http::Request<T>,
        active: &AtomicBool,
    ) -> std::result::Result<isahc::Response<isahc::AsyncBody>, SaveApiFailure> {
        let request = self
            .build_save_request(request, true)
            .map_err(save_failure)?;
        wait_for_save(request.send_async(), active)?.map_err(save_failure)
    }
    fn build_save_request<T>(
        &self,
        request: isahc::http::Request<T>,
        content: bool,
    ) -> std::result::Result<isahc::Request<T>, isahc::Error> {
        let (parts, body) = request.into_parts();
        let mut builder = isahc::Request::builder()
            .method(parts.method)
            .uri(parts.uri)
            .version(parts.version)
            .connect_timeout(self.metadata.connect_timeout);
        builder = if content {
            builder.low_speed_timeout(1, std::time::Duration::from_secs(30))
        } else {
            builder.timeout(self.metadata.response_timeout)
        };
        *builder.headers_mut().expect("request builder is valid") = parts.headers;
        if let Some(token) = self.token.read().unwrap().clone() {
            builder = builder.header("Authorization", format!("Bearer {token}"));
        }
        builder.body(body).map_err(isahc::Error::from)
    }

    pub(super) fn content_response_failure(
        &self,
        response: &mut isahc::Response<isahc::AsyncBody>,
        status: u16,
        active: &AtomicBool,
    ) -> std::result::Result<SaveApiFailure, SaveApiFailure> {
        if status == 401 {
            self.clear_token();
            return Ok(failure_with_detail(response, status, String::new()));
        }
        let detail =
            wait_for_save(response.text(), active)?.unwrap_or_else(|_| "no response body".into());
        Ok(failure_with_detail(response, status, detail))
    }

    pub(super) fn response_failure(
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

struct SaveTransferWake(std::thread::Thread);
impl Wake for SaveTransferWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// Poll the HTTP operation on its worker thread. Dropping either the response
/// future or its body cancels isahc's transfer, including waits for headers and
/// reads of success/error bodies after the upload has finished.
pub(super) fn wait_for_save<F: Future>(
    future: F,
    active: &AtomicBool,
) -> std::result::Result<F::Output, SaveApiFailure> {
    let mut future = std::pin::pin!(future);
    let waker = Waker::from(Arc::new(SaveTransferWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    loop {
        if !active.load(Ordering::Acquire) {
            return Err(local_failure(Error::Cancelled));
        }
        match future.as_mut().poll(&mut context) {
            Poll::Ready(result) => return Ok(result),
            Poll::Pending => std::thread::park_timeout(Duration::from_millis(50)),
        }
    }
}

pub(super) struct CancellableReader<R> {
    pub(super) inner: R,
    pub(super) active: std::sync::Arc<AtomicBool>,
}
impl<R: Read> Read for CancellableReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if !self.active.load(Ordering::Acquire) {
            return Err(std::io::Error::other("Save upload cancelled"));
        }
        self.inner.read(buffer)
    }
}

pub(super) fn local_failure(error: Error) -> SaveApiFailure {
    SaveApiFailure {
        error,
        retry_after: None,
    }
}

pub(super) fn save_failure(error: isahc::Error) -> SaveApiFailure {
    SaveApiFailure {
        error: transport(error),
        retry_after: None,
    }
}

fn response_failure(response: &mut isahc::Response<isahc::Body>, status: u16) -> SaveApiFailure {
    let detail = if status == 401 {
        String::new()
    } else {
        response_detail(response)
    };
    failure_with_detail(response, status, detail)
}

fn failure_with_detail<T>(
    response: &isahc::Response<T>,
    status: u16,
    detail: String,
) -> SaveApiFailure {
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let error = match status {
        401 => Error::Auth("session expired or token rejected".into()),
        403 => Error::Forbidden(detail.chars().take(200).collect()),
        _ => Error::Http {
            status,
            message: detail.chars().take(200).collect(),
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

pub(super) fn query_encode(value: &str) -> String {
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
