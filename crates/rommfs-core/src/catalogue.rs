//! The mounted catalogue: validated ROM entries grouped under platform dirs.
//! Built once at mount start from the real client; immutable for the session
//! (PRD R1: no polling/live sync — stop/start reloads).

use crate::error::Result;
use crate::layout::{relative_file_path, RetroBatSystems};
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
#[derive(Clone)]
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
    directories: Vec<String>,
    launchable: HashSet<RomKey>,
    inodes: Vec<(u64, u64, u64)>, // (inode, parent_inode, rom_index)
}

impl Catalogue {
    pub fn directories(&self) -> &[String] {
        &self.directories
    }
    pub fn directory_inode(&self, index: usize) -> Option<u64> {
        (index < self.directories.len()).then(|| 2 + index as u64)
    }
    pub fn platform_inode(&self, index: usize) -> Option<u64> {
        let path = self.platforms.get(index)?;
        self.directories
            .iter()
            .position(|d| d == path)
            .and_then(|i| self.directory_inode(i))
    }
    pub fn rom_inode(&self, index: usize) -> Option<u64> {
        (index < self.entries.len()).then(|| 2 + self.directories.len() as u64 + index as u64)
    }
    pub fn inode_kind(&self, inode: u64) -> Option<NodeKind> {
        if inode == 1 {
            return Some(NodeKind::Root);
        }
        let index = usize::try_from(inode.checked_sub(2)?).ok()?;
        if index < self.directories.len() {
            return Some(NodeKind::PlatformDir { index });
        }
        let index = index - self.directories.len();
        (index < self.entries.len()).then_some(NodeKind::Rom { index })
    }
    pub fn inode_plan(&self) -> &[(u64, u64, u64)] {
        &self.inodes
    }
    pub fn is_launchable(&self, key: &RomKey) -> bool {
        self.launchable.contains(key)
    }
    /// Distinct ROM identities with at least one visible file — the "games"
    /// count shown beside the file total (WS6.1).
    pub fn games(&self) -> usize {
        self.entries
            .iter()
            .map(|entry| entry.key.rom_id)
            .collect::<HashSet<_>>()
            .len()
    }
}

/// Generic platform folders with original files, including safe nested paths.
pub fn build_catalogue(
    server_id: &str,
    platforms: &[PlatformDto],
    roms: &[RomDto],
    warn: impl FnMut(String),
) -> Result<Catalogue> {
    build_catalogue_for_layout(server_id, platforms, roms, None, warn)
}

/// RetroBat uses the selected installation's supported systems and extensions.
/// Unsupported categories are diagnosed, never turned into games or ZIPs.
pub fn build_catalogue_for_layout(
    server_id: &str,
    platforms: &[PlatformDto],
    roms: &[RomDto],
    retrobat: Option<&RetroBatSystems>,
    mut warn: impl FnMut(String),
) -> Result<Catalogue> {
    let mut dir_of_slug = HashMap::new();
    let mut taken_dirs = HashSet::from([".rommfs-metadata".to_string()]);
    let mut sorted_platforms: Vec<_> = platforms.iter().collect();
    sorted_platforms.sort_by_key(|p| p.id);
    for p in sorted_platforms {
        let directory = match retrobat {
            Some(systems) => systems.folder(&p.slug, &p.fs_slug).map(str::to_owned),
            None => platform_dir(&p.fs_slug, &mut taken_dirs, &mut warn),
        };
        dir_of_slug.insert(p.fs_slug.clone(), directory);
    }
    let mut directories = HashSet::new();
    let mut taken: HashMap<String, HashSet<String>> = HashMap::new();
    let mut directory_mapping = HashMap::new();
    let mut entries = Vec::new();
    let mut launchable = HashSet::new();
    let mut skipped_unsupported = 0;
    let mut identities = HashSet::new();
    let mut sorted_roms: Vec<_> = roms.iter().collect();
    sorted_roms.sort_by_key(|r| r.id);
    for rom in sorted_roms {
        if rom.missing_from_fs || rom.files.is_empty() {
            skipped_unsupported += 1;
            warn(format!(
                "rom {}: {}",
                rom.id,
                if rom.missing_from_fs {
                    "missing from server filesystem"
                } else {
                    "no original file records"
                }
            ));
            continue;
        }
        let directory = dir_of_slug
            .entry(rom.platform_fs_slug.clone())
            .or_insert_with(|| match retrobat {
                Some(systems) => systems
                    .folder(&rom.platform_slug, &rom.platform_fs_slug)
                    .map(str::to_owned),
                None => platform_dir(&rom.platform_fs_slug, &mut taken_dirs, &mut warn),
            });
        let Some(directory) = directory.clone() else {
            skipped_unsupported += 1;
            warn(format!(
                "rom {}: platform {:?} has no supported folder",
                rom.id, rom.platform_slug
            ));
            continue;
        };
        let mut files: Vec<_> = rom.files.iter().collect();
        files.sort_by_key(|f| f.id);
        for file in files {
            if rom.id <= 0 || file.id <= 0 || !identities.insert((rom.id, file.id)) {
                return Err(crate::Error::InvalidCatalogue(
                    "invalid or duplicate original file identity".into(),
                ));
            }
            if sanitize_component(&file.file_name).is_none() {
                skipped_unsupported += 1;
                warn(format!(
                    "rom {} file {}: invalid file name",
                    rom.id, file.id
                ));
                continue;
            }
            let Some(relative) = relative_file_path(rom, file) else {
                skipped_unsupported += 1;
                warn(format!(
                    "rom {} file {}: path is missing or outside its platform directory",
                    rom.id, file.id
                ));
                continue;
            };
            let category = file.category.as_deref().unwrap_or("game");
            let game = matches!(category, "game" | "hack" | "demo" | "translation");
            let (platform, relative) = if game {
                (directory.clone(), relative)
            } else if let Some(_systems) = retrobat {
                if directory == "switch"
                    && matches!(category, "update" | "dlc")
                    && file.file_name.to_ascii_lowercase().ends_with(".nsp")
                {
                    ("switchupdates".into(), file.file_name.clone())
                } else {
                    skipped_unsupported += 1;
                    if category == "cheat" && file.file_name.to_ascii_lowercase().ends_with(".cht")
                    {
                        warn(format!("rom {} file {}: RetroArch cheat belongs outside the ROM tree; use Settings → Install library BIOS, keys and cheats",rom.id,file.id));
                    } else {
                        warn(format!(
                            "rom {} file {}: category {:?} has no verified RetroBat destination",
                            rom.id, file.id, category
                        ));
                    }
                    continue;
                }
            } else {
                let Some(category_name) = sanitize_component(category) else {
                    skipped_unsupported += 1;
                    warn(format!("file {}: invalid category", file.id));
                    continue;
                };
                (
                    directory.clone(),
                    format!("_library/{}/{}/{relative}", category_name.name(), rom.id),
                )
            };
            directories.insert(platform.clone());
            let file_name = allocate_path(
                &platform,
                &relative,
                &mut directories,
                &mut directory_mapping,
                &mut taken,
            )
            .ok_or_else(|| {
                crate::Error::InvalidCatalogue(format!(
                    "file {} has an unsafe relative path",
                    file.id
                ))
            })?;
            if file_name != relative {
                warn(format!(
                    "rom {} file {}: visible path adjusted to {:?}",
                    rom.id, file.id, file_name
                ));
            }
            let key = RomKey {
                server_id: server_id.into(),
                rom_id: rom.id,
                file_id: file.id,
            };
            if game && retrobat.is_none_or(|systems| systems.supports_file(&platform, &file_name)) {
                launchable.insert(key.clone());
            } else if game {
                warn(format!("rom {} file {}: original file retained but its extension is not launchable by this frontend", rom.id, file.id));
            }
            entries.push(RomEntry {
                key,
                platform_dir: platform,
                file_name,
                size: file.file_size_bytes,
                content_name: file.file_name.clone(),
                version: version_key(file),
            });
        }
    }
    for directory in dir_of_slug.values().flatten() {
        directories.insert(directory.clone());
    }
    let mut directories: Vec<_> = directories.into_iter().collect();
    directories.sort();
    let platforms = directories
        .iter()
        .filter(|d| !d.contains('/'))
        .cloned()
        .collect();
    entries.sort_by(|a, b| {
        a.platform_dir
            .cmp(&b.platform_dir)
            .then(a.file_name.cmp(&b.file_name))
    });
    let directory_inodes: HashMap<_, _> = directories
        .iter()
        .enumerate()
        .map(|(i, d)| (d.as_str(), 2 + i as u64))
        .collect();
    let base = 2 + directories.len() as u64;
    let inodes = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let path = format!("{}/{}", e.platform_dir, e.file_name);
            let parent = path
                .rsplit_once('/')
                .and_then(|(p, _)| directory_inodes.get(p))
                .copied()
                .unwrap_or(1);
            (base + i as u64, parent, i as u64)
        })
        .collect();
    Ok(Catalogue {
        platforms,
        entries,
        skipped_unsupported,
        directories,
        launchable,
        inodes,
    })
}

/// Reserve whole path components so files and directories cannot alias each
/// other after Windows sanitization. Source directories are reused exactly.
fn allocate_path(
    platform: &str,
    relative: &str,
    directories: &mut HashSet<String>,
    mapping: &mut HashMap<(String, String), String>,
    taken: &mut HashMap<String, HashSet<String>>,
) -> Option<String> {
    let parts: Vec<_> = relative.split('/').collect();
    let mut parent = platform.to_owned();
    let mut source = String::new();
    for (i, part) in parts.iter().enumerate() {
        if !source.is_empty() {
            source.push('/');
        }
        source.push_str(part);
        let component = sanitize_component(part)?.name().to_owned();
        let names = taken.entry(parent.clone()).or_insert_with(|| {
            ["gamelist.xml", ".rommfs-artwork"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        });
        let name = if i + 1 < parts.len() {
            let key = (platform.to_owned(), source.clone());
            if let Some(path) = mapping.get(&key) {
                parent = path.clone();
                continue;
            }
            let name = disambiguate(&component, &|n| names.contains(n));
            names.insert(name.to_lowercase());
            let path = format!("{parent}/{name}");
            mapping.insert(key, path.clone());
            directories.insert(path);
            name
        } else {
            let name = disambiguate(&component, &|n| names.contains(n));
            names.insert(name.to_lowercase());
            name
        };
        parent = format!("{parent}/{name}");
    }
    parent
        .strip_prefix(&format!("{platform}/"))
        .map(str::to_owned)
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
