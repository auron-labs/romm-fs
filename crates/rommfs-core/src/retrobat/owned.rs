//! "Hide RomM copies of games you already have": match RomM's files against the
//! user's own files in `roms/<system>/`, read-only.
//!
//! A cheap pass matches a RomM file to a local file by name (case-insensitive,
//! without its extension) and exact size. Under a size cap the match is
//! confirmed by whichever of CRC32, MD5 or SHA-1 RomM provides; above the cap
//! name and size are enough. Local hashes are cached by path, size and a
//! nanosecond modification time so each file is hashed once. A file the user
//! can't read never matches; the `RomM` folder and any other link or reparse
//! point is never the user's own; a system that can't be read is skipped and
//! reported, never aborting the others.

use crate::catalogue::Catalogue;
use crate::romm::{RomDto, RomFileDto};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use super::setup::ROMM_FOLDER;

/// Local files are hashed read-only up to this size; above it a name and size
/// match is enough.
pub const HASH_CAP_BYTES: u64 = 512 * 1024 * 1024;

/// The user's own files for the linked systems, indexed for matching.
pub struct LocalFiles {
    systems: HashMap<String, Vec<LocalFile>>,
    cache: HashCache,
    cache_path: Option<PathBuf>,
    cache_dirty: bool,
    problems: Vec<(String, String)>,
}

struct LocalFile {
    /// Lowercased file name without its extension.
    stem: String,
    size: u64,
    /// Modification time in nanoseconds since the epoch; a coarser value from
    /// an older cache simply won't match and the file is hashed again.
    mtime_nanos: Option<u64>,
    path: PathBuf,
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct LocalHashes {
    crc: String,
    md5: String,
    sha1: String,
}

#[derive(Default, Deserialize, Serialize)]
struct HashCache {
    #[serde(default)]
    entries: HashMap<String, CachedHashes>,
}

#[derive(Clone, Deserialize, Serialize)]
struct CachedHashes {
    size: u64,
    #[serde(default)]
    mtime_nanos: Option<u64>,
    #[serde(flatten)]
    hashes: LocalHashes,
}

impl LocalFiles {
    /// Index `installation/roms/<system>/` for the given systems, loading the
    /// hash cache if configured. The RomM folder, any other link and non-file
    /// entries are left out; subfolders are walked so a multi-file game's
    /// nested launch file is found.
    pub fn scan(installation: &Path, systems: &[String], cache_path: Option<PathBuf>) -> Self {
        let cache = cache_path
            .as_deref()
            .and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let mut indexed = HashMap::new();
        let mut problems = Vec::new();
        // A schema-1 install made `roms` itself a link to the managed tree;
        // nothing under it is the user's own.
        let roms = installation.join("roms");
        if fs::symlink_metadata(&roms)
            .is_ok_and(|metadata| crate::save_sync::path_is_reparse_point(&metadata))
        {
            return Self {
                systems: indexed,
                cache,
                cache_path,
                cache_dirty: false,
                problems,
            };
        }
        for system in systems {
            if crate::save_sync::validate_windows_path_component(system).is_err() {
                continue;
            }
            let folder = installation.join("roms").join(system);
            // Never follow a linked or non-folder system root: a schema-2 link
            // and the CfAPI-managed tree belong to RomMFS, not the user.
            match fs::symlink_metadata(&folder) {
                Ok(metadata) if crate::save_sync::path_is_reparse_point(&metadata) => continue,
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    problems.push((
                        system.clone(),
                        format!("couldn't read roms\\{system}: {error}"),
                    ));
                    continue;
                }
            }
            let entries = match fs::read_dir(&folder) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    problems.push((
                        system.clone(),
                        format!("couldn't read roms\\{system}: {error}"),
                    ));
                    continue;
                }
            };
            let mut files = Vec::new();
            for entry in entries.flatten() {
                collect_local_files(&entry, &mut files);
            }
            indexed.insert(system.clone(), files);
        }
        Self {
            systems: indexed,
            cache,
            cache_path,
            cache_dirty: false,
            problems,
        }
    }

    /// Systems whose folder couldn't be read, with the reason.
    pub fn problems(&self) -> &[(String, String)] {
        &self.problems
    }

    /// The `(rom_id, file_id)` identities the user already has a local copy of.
    pub fn owned(&mut self, catalogue: &Catalogue, roms: &[RomDto]) -> HashSet<(i64, i64)> {
        let files: HashMap<i64, &RomDto> = roms.iter().map(|rom| (rom.id, rom)).collect();
        let mut owned = HashSet::new();
        for entry in &catalogue.entries {
            let Some(file) = files
                .get(&entry.key.rom_id)
                .and_then(|rom| rom.files.iter().find(|file| file.id == entry.key.file_id))
            else {
                continue;
            };
            if self.matches(&entry.platform_dir, file) {
                owned.insert((entry.key.rom_id, entry.key.file_id));
            }
        }
        owned
    }

    /// Persist newly computed hashes. Best-effort: a cache is never required.
    pub fn save_cache(&self) {
        let (Some(path), true) = (self.cache_path.as_deref(), self.cache_dirty) else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(bytes) = serde_json::to_vec(&self.cache) {
            let _ = fs::write(path, bytes);
        }
    }

    /// True when the user has a local file matching this RomM file.
    fn matches(&mut self, system: &str, file: &RomFileDto) -> bool {
        let Some(stem) = stem(&file.file_name) else {
            return false;
        };
        let Some(candidates) = self.systems.get(system) else {
            return false;
        };
        let candidates: Vec<&LocalFile> = candidates
            .iter()
            .filter(|local| local.stem == stem && local.size == file.file_size_bytes)
            .collect();
        if candidates.is_empty() {
            return false;
        }
        let has_remote_hash =
            file.crc_hash.is_some() || file.md5_hash.is_some() || file.sha1_hash.is_some();
        for local in candidates {
            // A file the user can't read never counts, even above the cap or
            // with no hash to compare: nothing is inferred from an unreadable
            // path. The open is cheap and also guards a stale cache entry.
            if fs::File::open(&local.path).is_err() {
                continue;
            }
            if file.file_size_bytes > HASH_CAP_BYTES {
                return true;
            }
            // Nothing to confirm with: name and size are all RomM gives.
            if !has_remote_hash {
                return true;
            }
            let Some(local) = cached_hashes(&mut self.cache, &mut self.cache_dirty, local) else {
                continue;
            };
            if confirm(
                &local,
                file.crc_hash.as_deref(),
                file.md5_hash.as_deref(),
                file.sha1_hash.as_deref(),
            ) {
                return true;
            }
        }
        false
    }
}

/// Hash one local file, reusing the cache when path, size and mtime match.
fn cached_hashes(cache: &mut HashCache, dirty: &mut bool, file: &LocalFile) -> Option<LocalHashes> {
    let key = file.path.to_string_lossy().into_owned();
    if let Some(cached) = cache.entries.get(&key) {
        if cached.size == file.size && cached.mtime_nanos == file.mtime_nanos {
            return Some(cached.hashes.clone());
        }
    }
    let hashes = hash_file(&file.path).ok()?;
    cache.entries.insert(
        key,
        CachedHashes {
            size: file.size,
            mtime_nanos: file.mtime_nanos,
            hashes: hashes.clone(),
        },
    );
    *dirty = true;
    Some(hashes)
}

/// The first hash RomM provides, compared case-insensitively.
fn confirm(local: &LocalHashes, crc: Option<&str>, md5: Option<&str>, sha1: Option<&str>) -> bool {
    if let Some(crc) = crc {
        return local.crc.eq_ignore_ascii_case(crc.trim());
    }
    if let Some(md5) = md5 {
        return local.md5.eq_ignore_ascii_case(md5.trim());
    }
    match sha1 {
        Some(sha1) => local.sha1.eq_ignore_ascii_case(sha1.trim()),
        None => true,
    }
}

/// CRC32, MD5 and SHA-1 of a local file, read once.
fn hash_file(path: &Path) -> std::io::Result<LocalHashes> {
    let mut crc = crc32fast::Hasher::new();
    let mut md5 = Md5::new();
    let mut sha1 = Sha1::new();
    let mut file = fs::File::open(path)?.take(HASH_CAP_BYTES);
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        crc.update(&buffer[..read]);
        md5.update(&buffer[..read]);
        sha1.update(&buffer[..read]);
    }
    Ok(LocalHashes {
        crc: format!("{:08x}", crc.finalize()),
        md5: format!("{:x}", md5.finalize()),
        sha1: format!("{:x}", sha1.finalize()),
    })
}

fn stem(name: &str) -> Option<String> {
    let stem = Path::new(name).file_stem()?.to_str()?;
    (!stem.is_empty()).then(|| stem.to_lowercase())
}

/// Index one entry, walking real subfolders but never a link: the `RomM`
/// junction and any other reparse point are RomMFS's or elsewhere, not the
/// user's own file.
fn collect_local_files(entry: &fs::DirEntry, files: &mut Vec<LocalFile>) {
    let Ok(metadata) = entry.metadata() else {
        return;
    };
    if crate::save_sync::path_is_reparse_point(&metadata) {
        return;
    }
    if metadata.is_dir() {
        // The RomM folder is always RomMFS's own, even when it is a plain
        // directory rather than a link: never treat its contents as the user's.
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.eq_ignore_ascii_case(ROMM_FOLDER))
        {
            return;
        }
        if let Ok(children) = fs::read_dir(entry.path()) {
            for child in children.flatten() {
                collect_local_files(&child, files);
            }
        }
        return;
    }
    if !metadata.is_file() {
        return;
    }
    let path = entry.path();
    let Some(stem) = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(stem)
    else {
        return;
    };
    files.push(LocalFile {
        stem,
        size: metadata.len(),
        mtime_nanos: metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|elapsed| u64::try_from(elapsed.as_nanos()).ok()),
        path,
    });
}
