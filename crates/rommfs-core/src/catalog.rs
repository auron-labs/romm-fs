//! The mounted catalogue: validated ROM entries grouped under platform dirs.
//! Built once at mount start from the real client; immutable for the session
//! (PRD R1: no polling/live sync — stop/start reloads).

use crate::error::Result;
use crate::romm::{PlatformDto, RomDto, RomFileDto};
use crate::sanitize::{disambiguate, sanitize_component};
use std::collections::{HashMap, HashSet};

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

impl Catalogue {
    /// Inode of the platform dir at `index` in `platforms`.
    pub fn platform_inode(&self, index: usize) -> Option<u64> {
        (index < self.platforms.len()).then(|| 2 + index as u64)
    }

    /// Inode of the ROM file at `rom_index` in `entries`.
    pub fn rom_inode(&self, rom_index: usize) -> Option<u64> {
        (rom_index < self.entries.len()).then(|| 2 + self.platforms.len() as u64 + rom_index as u64)
    }

    /// Classify an inode as root / platform dir / ROM file.
    pub fn inode_kind(&self, inode: u64) -> Option<NodeKind> {
        match inode {
            1 => Some(NodeKind::Root),
            i if i >= 2 && i < 2 + self.platforms.len() as u64 => Some(NodeKind::PlatformDir {
                index: (i - 2) as usize,
            }),
            i => {
                let idx = i
                    .checked_sub(2 + self.platforms.len() as u64)
                    .map(|v| v as usize);
                idx.filter(|&r| r < self.entries.len())
                    .map(|index| NodeKind::Rom { index })
            }
        }
    }

    /// The inode plan: (inode, parent inode, rom index) for every file node.
    pub fn inode_plan(&self) -> &[(u64, u64, u64)] {
        &self.inodes
    }
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
    // fs_slug -> visible dir name; `None` = rejected component, ROMs on it
    // are unplaceable and skip with the same accounting.
    let mut dir_of_slug: HashMap<String, Option<String>> = HashMap::new();
    let mut taken_dirs: HashSet<String> = HashSet::new();

    for p in platforms {
        dir_of_slug.insert(
            p.fs_slug.clone(),
            platform_dir(&p.fs_slug, &mut taken_dirs, &mut warn),
        );
    }

    let mut skipped_unsupported = 0usize;
    let mut entries: Vec<RomEntry> = Vec::new();
    // dir -> lowercase visible names already taken in that dir.
    let mut taken_names: HashMap<String, HashSet<String>> = HashMap::new();

    for rom in roms {
        if rom.files.len() != 1 {
            skipped_unsupported += 1;
            warn(format!(
                "rom {} ({:?}): {} file entries — multi-file/folder games are unsupported, skipped",
                rom.id,
                rom.fs_name,
                rom.files.len()
            ));
            continue;
        }
        let file = &rom.files[0];

        let platform_dir = match dir_of_slug.get(&rom.platform_fs_slug) {
            Some(Some(dir)) => dir.clone(),
            Some(None) => {
                skipped_unsupported += 1;
                warn(format!(
                    "rom {} ({:?}): platform fs_slug {:?} rejected, skipped",
                    rom.id, rom.fs_name, rom.platform_fs_slug
                ));
                continue;
            }
            None => {
                // ROM references a platform absent from the platform list —
                // still derive its dir from the ROM's own fs_slug so the
                // library stays complete.
                let dir = platform_dir(&rom.platform_fs_slug, &mut taken_dirs, &mut warn);
                dir_of_slug.insert(rom.platform_fs_slug.clone(), dir.clone());
                match dir {
                    Some(dir) => dir,
                    None => {
                        skipped_unsupported += 1;
                        warn(format!(
                            "rom {} ({:?}): platform fs_slug {:?} rejected, skipped",
                            rom.id, rom.fs_name, rom.platform_fs_slug
                        ));
                        continue;
                    }
                }
            }
        };

        let file_name = match sanitize_component(&file.file_name) {
            Some(c) => c.name().to_string(),
            None => {
                skipped_unsupported += 1;
                warn(format!(
                    "rom {} ({:?}): file name {:?} rejected, skipped",
                    rom.id, rom.fs_name, file.file_name
                ));
                continue;
            }
        };

        let taken = taken_names.entry(platform_dir.clone()).or_default();
        let file_name = disambiguate(&file_name, &|n: &str| taken.contains(n));
        if file_name != file.file_name {
            warn(format!(
                "rom {} ({:?}): visible name adjusted to {:?}",
                rom.id, rom.fs_name, file_name
            ));
        }
        taken.insert(file_name.to_lowercase());

        entries.push(RomEntry {
            key: RomKey {
                server_id: server_id.to_string(),
                rom_id: rom.id,
                file_id: file.id,
            },
            platform_dir,
            file_name,
            size: file.file_size_bytes,
            content_name: file.file_name.clone(),
            version: version_key(file),
        });
    }

    // Deterministic order: platform dirs sorted, entries grouped per dir and
    // sorted by visible name inside each dir.
    let mut platform_names: Vec<String> = dir_of_slug
        .values()
        .flatten()
        .cloned()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    platform_names.sort();
    entries.sort_by(|a, b| {
        a.platform_dir
            .cmp(&b.platform_dir)
            .then_with(|| a.file_name.cmp(&b.file_name))
    });

    let base = 2 + platform_names.len() as u64;
    let inodes: Vec<(u64, u64, u64)> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let parent = platform_names
                .iter()
                .position(|p| p == &e.platform_dir)
                .map(|p| 2 + p as u64)
                .unwrap_or(1);
            (base + i as u64, parent, i as u64)
        })
        .collect();

    Ok(Catalogue {
        platforms: platform_names,
        entries,
        skipped_unsupported,
        inodes,
    })
}

/// Sanitize one `fs_slug` into a platform dir name, deduplicated
/// case-insensitively against `taken_dirs` (which is updated on success).
fn platform_dir(
    fs_slug: &str,
    taken_dirs: &mut HashSet<String>,
    warn: &mut impl FnMut(String),
) -> Option<String> {
    let name = match sanitize_component(fs_slug) {
        Some(c) => c.name().to_string(),
        None => {
            warn(format!("platform fs_slug {fs_slug:?} rejected"));
            return None;
        }
    };
    let dir = disambiguate(&name, &|n: &str| taken_dirs.contains(n));
    if dir != name {
        warn(format!(
            "platform dir {name:?} collides, adjusted to {dir:?}"
        ));
    }
    taken_dirs.insert(dir.to_lowercase());
    Some(dir)
}

/// Best available content-version fingerprint for one file:
/// sha1, else md5, else crc, else a last-modified+size composite.
fn version_key(file: &RomFileDto) -> Option<VersionKey> {
    if let Some(h) = &file.sha1_hash {
        return Some(VersionKey(format!("sha1:{h}")));
    }
    if let Some(h) = &file.md5_hash {
        return Some(VersionKey(format!("md5:{h}")));
    }
    if let Some(h) = &file.crc_hash {
        return Some(VersionKey(format!("crc:{h}")));
    }
    file.last_modified
        .as_ref()
        .map(|lm| VersionKey(format!("lastmod:{lm}+size:{}", file.file_size_bytes)))
}

/// Derive the server identity used for cache/root scoping: normalized URL
/// string (scheme+authority+base path, no credentials, no trailing slash).
pub fn server_id_of(base_url: &str) -> String {
    // Match RommClient's treatment of the configured base URL. Preserve a
    // reverse-proxy path because it can route to a different RomM instance.
    let s = base_url.trim().trim_end_matches('/');
    let (scheme, rest) = match s.find("://") {
        Some(i) => (s[..i].to_ascii_lowercase(), &s[i + 3..]),
        None => ("http".to_string(), s),
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    // Strip any userinfo.
    let authority = rest[..authority_end].rsplit('@').next().unwrap_or_default();

    let (host, port) = if let Some(stripped) = authority.strip_prefix('[') {
        // Bracketed literal (IPv6): host keeps its brackets.
        match stripped.find(']') {
            Some(e) => {
                let host = &authority[..=e + 1];
                let port = authority[e + 2..]
                    .strip_prefix(':')
                    .and_then(|p| p.parse::<u16>().ok());
                (host, port)
            }
            None => (authority, None),
        }
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => match p.parse::<u16>() {
                Ok(p) => (h, Some(p)),
                Err(_) => (authority, None),
            },
            None => (authority, None),
        }
    };

    let host = host.to_ascii_lowercase();
    let path_end = rest[authority_end..]
        .find(['?', '#'])
        .map(|offset| authority_end + offset)
        .unwrap_or(rest.len());
    let path = rest[authority_end..path_end].trim_end_matches('/');
    let authority = match port {
        Some(p) if !((scheme == "http" && p == 80) || (scheme == "https" && p == 443)) => {
            format!("{scheme}://{host}:{p}")
        }
        _ => format!("{scheme}://{host}"),
    };
    format!("{authority}{path}")
}

/// A file or directory in the projected tree (inode-model for the adapter).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Root,
    PlatformDir { index: usize },
    Rom { index: usize },
}
