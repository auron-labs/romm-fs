//! RomM memory-card client. Source-backed against the pinned commit recorded
//! in `docs/contracts/romm-api.md`: cards are per (user, emulator); a card's
//! data is an ordered set of zip versions. `assets.read`/`assets.write` scope
//! the routes, which the pairing set already requests.

use super::client::RommClient;
use super::saves::{
    local_failure, query_encode, save_failure, wait_for_save, CancellableReader, SaveApiFailure,
};
use crate::error::Error;
use futures_lite::io::AsyncReadExt;
use isahc::prelude::*;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The server caps a card transfer here; larger uploads and downloads are
/// refused locally rather than sent and rejected.
pub const MAX_MEMORY_CARD_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct MemoryCard {
    pub id: i64,
    pub user_id: i64,
    pub emulator: String,
    #[serde(default)]
    pub platform_id: Option<i64>,
    pub name: String,
    #[serde(default)]
    pub slot: i64,
    #[serde(default)]
    pub is_public: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct MemoryCardVersion {
    pub id: i64,
    pub memory_card_id: i64,
    pub file_name: String,
    pub file_size_bytes: u64,
    #[serde(default)]
    pub content_hash: Option<String>,
    #[serde(default)]
    pub missing_from_fs: bool,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Serialize)]
struct CreateMemoryCard<'a> {
    name: &'a str,
    emulator: &'a str,
}

impl RommClient {
    /// The caller's own cards, optionally narrowed to one emulator.
    pub fn memory_cards(
        &self,
        emulator: &str,
    ) -> std::result::Result<Vec<MemoryCard>, SaveApiFailure> {
        let request = isahc::Request::get(format!(
            "{}/api/memory-cards?emulator={}",
            self.base_url,
            query_encode(emulator)
        ))
        .body(())
        .map_err(request_failure)?;
        let mut response = self.save_request(request).map_err(save_failure)?;
        match response.status().as_u16() {
            status if (200..300).contains(&status) => response
                .json()
                .map_err(|error| local_failure(Error::InvalidCatalogue(error.to_string()))),
            status => Err(self.response_failure(&mut response, status)),
        }
    }

    /// Create an empty card. Its data accrues as uploaded versions.
    pub fn create_memory_card(
        &self,
        name: &str,
        emulator: &str,
    ) -> std::result::Result<MemoryCard, SaveApiFailure> {
        crate::save_sync::validate_emulator_name(emulator).map_err(local_failure)?;
        if name.trim().is_empty() {
            return Err(local_failure(Error::Unsupported(
                "memory card name is empty".into(),
            )));
        }
        let body = serde_json::to_string(&CreateMemoryCard { name, emulator })
            .map_err(|error| local_failure(Error::InvalidCatalogue(error.to_string())))?;
        let request = isahc::Request::post(format!("{}/api/memory-cards", self.base_url))
            .header("Content-Type", "application/json")
            .body(body)
            .map_err(request_failure)?;
        let mut response = self.save_request(request).map_err(save_failure)?;
        match response.status().as_u16() {
            status if (200..300).contains(&status) => response
                .json()
                .map_err(|error| local_failure(Error::InvalidCatalogue(error.to_string()))),
            status => Err(self.response_failure(&mut response, status)),
        }
    }

    /// A card's snapshot history, newest first.
    pub fn memory_card_versions(
        &self,
        card_id: i64,
    ) -> std::result::Result<Vec<MemoryCardVersion>, SaveApiFailure> {
        let request = isahc::Request::get(format!(
            "{}/api/memory-cards/{card_id}/versions",
            self.base_url
        ))
        .body(())
        .map_err(request_failure)?;
        let mut response = self.save_request(request).map_err(save_failure)?;
        match response.status().as_u16() {
            status if (200..300).contains(&status) => response
                .json()
                .map_err(|error| local_failure(Error::InvalidCatalogue(error.to_string()))),
            status => Err(self.response_failure(&mut response, status)),
        }
    }

    /// Stream one version's archive into `writer`, refusing a body that differs
    /// from the inventoried size.
    pub fn download_memory_card_version(
        &self,
        version_id: i64,
        size: u64,
        writer: &mut impl Write,
        active: &AtomicBool,
    ) -> std::result::Result<(), SaveApiFailure> {
        if !active.load(Ordering::Acquire) {
            return Err(local_failure(Error::Cancelled));
        }
        if size > MAX_MEMORY_CARD_BYTES {
            return Err(local_failure(Error::Unsupported(
                "remote memory card exceeds the 256 MB limit".into(),
            )));
        }
        let request = isahc::Request::get(format!(
            "{}/api/memory-cards/versions/{version_id}/content",
            self.base_url
        ))
        .body(())
        .map_err(|_| {
            local_failure(Error::Transport(
                "Could not build memory card download request".into(),
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
            received = received.checked_add(count as u64).ok_or_else(|| {
                local_failure(Error::Unsupported("Memory card size overflow".into()))
            })?;
            if received > size {
                return Err(local_failure(Error::Unsupported(
                    "Remote memory card exceeds its inventoried size".into(),
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

    /// Upload a card archive as the card's newest version, streaming the file
    /// rather than buffering a 256 MB payload. The multipart field name matches
    /// the server's `cardFile` upload parameter.
    pub fn upload_memory_card_version(
        &self,
        card_id: i64,
        filename: &str,
        path: &Path,
        active: &Arc<AtomicBool>,
    ) -> std::result::Result<MemoryCardVersion, SaveApiFailure> {
        if filename.is_empty() || filename.contains(['\r', '\n', '"', '/', '\\']) {
            return Err(local_failure(Error::Unsupported(
                "Invalid memory card upload filename".into(),
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
                "Memory card archive is not an ordinary file".into(),
            )));
        }
        let size = metadata.len();
        if size == 0 || size > MAX_MEMORY_CARD_BYTES {
            return Err(local_failure(Error::Unsupported(
                "memory card upload is outside the 256 MB limit".into(),
            )));
        }
        let boundary = format!("rommfs-card-{}", uuid::Uuid::new_v4());
        let prefix = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"cardFile\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n");
        let suffix = format!("\r\n--{boundary}--\r\n");
        let length = size
            .checked_add((prefix.len() + suffix.len()) as u64)
            .ok_or_else(|| {
                local_failure(Error::Unsupported(
                    "Memory card upload length overflow".into(),
                ))
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
        let request = isahc::Request::post(format!(
            "{}/api/memory-cards/{card_id}/versions",
            self.base_url
        ))
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(isahc::AsyncBody::from_reader_sized(
            blocking::Unblock::new(reader),
            length,
        ))
        .map_err(|_| {
            local_failure(Error::Transport(
                "Could not build memory card upload request".into(),
            ))
        })?;
        let mut response = self.save_content_request(request, active)?;
        if response.status().is_success() {
            wait_for_save(response.json(), active)?
                .map_err(|e| local_failure(Error::InvalidCatalogue(e.to_string())))
        } else {
            let status = response.status().as_u16();
            Err(self.content_response_failure(&mut response, status, active)?)
        }
    }
}

fn request_failure(error: isahc::http::Error) -> SaveApiFailure {
    local_failure(Error::Transport(format!("request build failed: {error}")))
}
