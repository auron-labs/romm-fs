use crate::catalogue::Catalogue;
use crate::romm::{RomDto, RommClient};
use crate::save_sync::ensure_no_reparse_components;
use crate::{Error, Result};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

const MAX_ARTWORK_BYTES: u64 = 32 * 1024 * 1024;
const CONCURRENCY: usize = 4;

pub struct ArtworkBatch {
    references: HashMap<(i64, &'static str), String>,
    files: BTreeMap<String, PathBuf>,
    warnings: Vec<String>,
}
impl ArtworkBatch {
    pub fn references(&self) -> &HashMap<(i64, &'static str), String> {
        &self.references
    }
    pub fn files(&self) -> &BTreeMap<String, PathBuf> {
        &self.files
    }
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
}

/// Cache supplied cover/screenshot resources with four bounded workers. Files
/// are addressed by resource/version, and ROM bodies/detail endpoints are never
/// involved. Failed/missing images do not prevent complete gamelist generation.
pub fn cache_artwork(
    client: &RommClient,
    catalogue: &Catalogue,
    roms: &[RomDto],
    cache: &Path,
) -> Result<ArtworkBatch> {
    cache_artwork_cancellable(client, catalogue, roms, cache, &AtomicBool::new(true))
}

pub fn cache_artwork_cancellable(
    client: &RommClient,
    catalogue: &Catalogue,
    roms: &[RomDto],
    cache: &Path,
    active: &AtomicBool,
) -> Result<ArtworkBatch> {
    ensure_no_reparse_components(cache)?;
    fs::create_dir_all(cache)?;
    let mut platforms: HashMap<i64, HashSet<String>> = HashMap::new();
    for entry in catalogue
        .entries
        .iter()
        .filter(|e| catalogue.is_launchable(&e.key))
    {
        platforms
            .entry(entry.key.rom_id)
            .or_default()
            .insert(entry.platform_dir.clone());
    }
    let mut tasks = BTreeMap::new();
    for rom in roms.iter().filter(|r| platforms.contains_key(&r.id)) {
        let cover = rom
            .metadata
            .path_cover_large
            .as_deref()
            .filter(|s| !s.is_empty())
            .or(rom
                .metadata
                .path_cover_small
                .as_deref()
                .filter(|s| !s.is_empty()));
        let screenshot = rom
            .metadata
            .merged_screenshots
            .first()
            .map(String::as_str)
            .filter(|s| !s.is_empty());
        for (role, resource) in [("image", screenshot.or(cover)), ("thumbnail", cover)] {
            let Some(resource) = resource else {
                continue;
            };
            let key = format!(
                "{:x}",
                Sha256::digest(format!("{}:{resource}", rom.updated_at))
            );
            let task = tasks
                .entry(key)
                .or_insert_with(|| (resource.to_owned(), Vec::new()));
            task.1.push((rom.id, role));
        }
    }
    let tasks: Vec<_> = tasks.into_iter().collect();
    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..CONCURRENCY.min(tasks.len()) {
            scope.spawn(|| loop {
                if !active.load(Ordering::Acquire) {
                    break;
                }
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some((key, (resource, _))) = tasks.get(index) else {
                    break;
                };
                let result = cache_one(client, resource, &cache.join(format!("{key}.img")), active);
                results
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push((index, result));
            });
        }
    });
    if !active.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    let mut batch = ArtworkBatch {
        references: HashMap::new(),
        files: BTreeMap::new(),
        warnings: Vec::new(),
    };
    let mut results = results.into_inner().unwrap_or_else(|e| e.into_inner());
    results.sort_by_key(|(index, _)| *index);
    for (index, result) in results {
        let (key, (_, assignments)) = &tasks[index];
        match result {
            Ok((source, extension)) => {
                let relative = format!(".rommfs-artwork/{key}.{extension}");
                for &(rom_id, role) in assignments {
                    batch.references.insert((rom_id, role), relative.clone());
                    for platform in &platforms[&rom_id] {
                        batch
                            .files
                            .insert(format!("{platform}/{relative}"), source.clone());
                    }
                }
            }
            Err(error) if error.needs_sign_in() => return Err(error),
            Err(error) => batch.warnings.push(format!(
                "Artwork unavailable for ROM {}: {error}",
                assignments[0].0
            )),
        }
    }
    for rom in roms.iter().filter(|r| platforms.contains_key(&r.id)) {
        if !batch.references.contains_key(&(rom.id, "image")) {
            if let Some(cover) = batch.references.get(&(rom.id, "thumbnail")).cloned() {
                batch.references.insert((rom.id, "image"), cover);
            }
        }
        if !batch.references.contains_key(&(rom.id, "image")) {
            batch.warnings.push(format!(
                "ROM {} has no available server-local cover or screenshot",
                rom.id
            ));
        }
    }
    Ok(batch)
}

fn cache_one(
    client: &RommClient,
    resource: &str,
    path: &Path,
    active: &AtomicBool,
) -> Result<(PathBuf, &'static str)> {
    ensure_no_reparse_components(path)?;
    if !path.try_exists()? {
        let mut staged = tempfile::NamedTempFile::new_in(
            path.parent()
                .ok_or_else(|| Error::Unsupported("artwork cache has no parent".into()))?,
        )?;
        client.download_artwork(
            resource,
            &mut LimitedWriter {
                file: staged.as_file_mut(),
                remaining: MAX_ARTWORK_BYTES,
                active,
            },
        )?;
        staged.as_file().sync_all()?;
        // Validate before any cache publication. A 200 HTML login page is not art.
        raster_extension(&fs::read(staged.path())?)?;
        if let Err(error) = staged.persist_noclobber(path) {
            if error.error.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(error.error.into());
            }
        }
    }
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() > MAX_ARTWORK_BYTES {
        return Err(Error::Unsupported(
            "invalid or oversized cached artwork".into(),
        ));
    }
    Ok((path.into(), raster_extension(&fs::read(path)?)?))
}

fn raster_extension(bytes: &[u8]) -> Result<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok("png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Ok("jpg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Ok("gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Ok("webp")
    } else {
        Err(Error::Unsupported(
            "artwork is not a supported raster image".into(),
        ))
    }
}

struct LimitedWriter<'a> {
    file: &'a mut fs::File,
    remaining: u64,
    active: &'a AtomicBool,
}
impl Write for LimitedWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if !self.active.load(Ordering::Acquire) {
            return Err(std::io::Error::other("artwork download cancelled"));
        }
        if bytes.len() as u64 > self.remaining {
            return Err(std::io::Error::other(
                "artwork exceeds the 32 MiB resource limit",
            ));
        }
        let written = self.file.write(bytes)?;
        self.remaining -= written as u64;
        Ok(written)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}
