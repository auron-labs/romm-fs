//! The mounted catalogue: validated ROM entries grouped under platform dirs.
//! Built once at mount start from the real client; immutable for the session
//! (PRD R1: no polling/live sync — stop/start reloads).

use crate::error::Result;
use crate::romm::{PlatformDto, RomDto};

/// Stable per-ROM content identity scoped to the server + ROM + file.
/// Version metadata (sha1/md5/crc/last_modified/size) distinguishes known
/// changed content for cache invalidation (PRD R4).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RomKey {
    /// Derived from normalized server URL so different servers never share
    /// cache entries or managed root content.
    pub server_id: String,
    pub rom_id: i64,
    pub file_id: i64,
}

impl RomKey {
    /// Filesystem-safe cache file stem derived from the identity (never from
    /// untrusted filenames). Stable across runs: FNV-1a of the server id.
    pub fn cache_stem(&self) -> String {
        let mut h: u64 = 0xcbf29ce484222325;
        for &b in self.server_id.as_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        format!("{h:016x}-{}-{}", self.rom_id, self.file_id)
    }
}

/// Best available content-version fingerprint, e.g. `sha1:<hash>` or a
/// composite of the supplied metadata. `None` when RomM provides none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionKey(pub String);

/// One visible file in the projected tree.
#[derive(Clone, Debug)]
pub struct RomEntry {
    pub key: RomKey,
    /// Visible platform directory name (fs_slug, sanitized).
    pub platform_dir: String,
    /// Visible filename after sanitize/collision handling.
    pub file_name: String,
    /// Byte size reported by the contract.
    pub size: u64,
    /// Remote content path fragment (`file_name` used in the content URL).
    pub content_name: String,
    pub version: Option<VersionKey>,
}

/// The full mounted snapshot.
pub struct Catalogue {
    /// Sorted platform directory names.
    pub platforms: Vec<String>,
    /// Entries grouped per platform dir, each list sorted for stable order.
    pub entries: Vec<RomEntry>,
    /// Count of ROMs skipped as unsupported (multi-file/folder games) or
    /// invalid names — surfaced in status/logs.
    pub skipped_unsupported: usize,
    /// Stable inode assignment: platform dirs 2..=2+P-1 (sorted), ROM files
    /// sequential from a base — deterministic for an unchanged catalogue.
    inodes: Vec<(u64, u64, u64)>, // (inode, parent_inode, rom_index)
}

/// Build the catalogue from verified-contract DTOs. Pure; no IO.
///
/// Rules: one `files` entry per ROM (else skip+count); platform dir =
/// sanitized `fs_slug`; visible name = sanitized `fs_name`/file `file_name`;
/// case-insensitive collisions disambiguated deterministically keeping the
/// extension; invalid components are rejected and logged by the caller.
pub fn build_catalogue(
    server_id: &str,
    platforms: &[PlatformDto],
    roms: &[RomDto],
    mut warn: impl FnMut(String),
) -> Result<Catalogue> {
    let _ = (server_id, platforms, roms, &mut warn);
    todo!()
}

/// Derive the server identity used for cache/root scoping: normalized URL
/// string (scheme+host+port, no credentials, no trailing slash).
pub fn server_id_of(base_url: &str) -> String {
    let _ = base_url;
    todo!()
}

/// A file or directory in the projected tree (inode-model for the adapter).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Root,
    PlatformDir { index: usize },
    Rom { index: usize },
}
