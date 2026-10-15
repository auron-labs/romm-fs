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
use std::time::{Duration, Instant};

const MAX_ARTWORK_BYTES: u64 = 32 * 1024 * 1024;
/// Artwork workers, kept in step with the named constant in
/// `docs/architecture.md` (WS4.5).
const CONCURRENCY: usize = 8;
/// Minimum interval between artwork progress callbacks (~4 Hz).
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// Throttles progress callbacks so a large library reports a few times a
/// second rather than once per downloaded image. The final tick always fires,
/// and callbacks are globally monotonic: several workers can finish out of
/// order, so the highest count is tracked under the same lock.
struct ProgressReporter<'a> {
    progress: &'a (dyn Fn(usize, usize) + Sync),
    total: usize,
    /// (last emit time, highest count already reported).
    state: Mutex<(Instant, usize)>,
}

impl ProgressReporter<'_> {
    fn tick(&self, completed: usize) {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let (last, reported) = &mut *state;
        let completed = completed.max(*reported);
        if completed >= self.total || now.duration_since(*last) >= PROGRESS_INTERVAL {
            *last = now;
            *reported = completed;
            (self.progress)(completed, self.total);
        }
    }
}

#[derive(Default)]
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

/// Cache supplied cover/screenshot resources with eight bounded workers. Files
/// are addressed by resource/version, and ROM bodies/detail endpoints are never
/// involved. Failed/missing images do not prevent complete gamelist generation.
pub fn cache_artwork(
    client: &RommClient,
    catalogue: &Catalogue,
    roms: &[RomDto],
    cache: &Path,
) -> Result<ArtworkBatch> {
    cache_artwork_cancellable(
        client,
        catalogue,
        roms,
        cache,
        &AtomicBool::new(true),
        &|_, _| {},
    )
}

pub fn cache_artwork_cancellable(
    client: &RommClient,
    catalogue: &Catalogue,
    roms: &[RomDto],
    cache: &Path,
    active: &AtomicBool,
    progress: &(dyn Fn(usize, usize) + Sync),
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
    let completed = AtomicUsize::new(0);
    let results = Mutex::new(Vec::new());
    let reporter = ProgressReporter {
        progress,
        total: tasks.len(),
        state: Mutex::new((Instant::now(), 0)),
    };
    // Report the starting point so the UI shows "0 / N" while the first
    // downloads are still in flight.
    progress(0, tasks.len());
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
                let result = cache_one(client, resource, &cache.join(key), active);
                results
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push((index, result));
                reporter.tick(completed.fetch_add(1, Ordering::AcqRel) + 1);
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
    // One entry per artless ROM, with the first download error that caused it.
    let mut artless: BTreeMap<i64, Option<String>> = BTreeMap::new();
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
            Err(error) => {
                artless
                    .entry(assignments[0].0)
                    .or_insert(Some(error.to_string()));
            }
        }
    }
    for rom in roms.iter().filter(|r| platforms.contains_key(&r.id)) {
        if !batch.references.contains_key(&(rom.id, "image")) {
            if let Some(cover) = batch.references.get(&(rom.id, "thumbnail")).cloned() {
                batch.references.insert((rom.id, "image"), cover);
            }
        }
        if !batch.references.contains_key(&(rom.id, "image")) {
            artless.entry(rom.id).or_insert(None);
        }
    }
    // Aggregate rather than one warning per ROM and role: a library with no
    // server-side artwork should cost one log line, not thousands.
    if !artless.is_empty() {
        let count = artless.len();
        let examples: Vec<String> = artless
            .keys()
            .take(3)
            .map(|id| format!("ROM {id}"))
            .collect();
        let mut message = format!(
            "{count} {} no artwork on the server (e.g. {})",
            if count == 1 { "ROM has" } else { "ROMs have" },
            examples.join(", ")
        );
        if let Some(error) = artless.values().flatten().next() {
            message.push_str(&format!("; first error: {error}"));
        }
        batch.warnings.push(message);
    }
    Ok(batch)
}

fn cache_one(
    client: &RommClient,
    resource: &str,
    base: &Path,
    active: &AtomicBool,
) -> Result<(PathBuf, &'static str)> {
    ensure_no_reparse_components(base)?;
    if let Some(cached) = cached_artwork(base)? {
        return Ok(cached);
    }
    let mut staged = tempfile::NamedTempFile::new_in(
        base.parent()
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
    let extension = raster_extension(&fs::read(staged.path())?)?;
    // The detected extension is part of the cache filename, so a later hit
    // never re-reads (or re-validates) the image bytes.
    let path = base.with_extension(extension);
    ensure_no_reparse_components(&path)?;
    if let Err(error) = staged.persist_noclobber(&path) {
        if error.error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(error.error.into());
        }
    }
    Ok(cached_artwork(base)?.unwrap_or((path, extension)))
}

/// A previously cached artwork file, found by its stored extension. A cache
/// hit is validated by size only — the extension proves the bytes were already
/// raster-checked when written. The actual file path is reparse-checked, not
/// just its parent.
fn cached_artwork(base: &Path) -> Result<Option<(PathBuf, &'static str)>> {
    for extension in ["png", "jpg", "gif", "webp"] {
        let path = base.with_extension(extension);
        ensure_no_reparse_components(&path)?;
        if !path.try_exists()? {
            continue;
        }
        let metadata = fs::metadata(&path)?;
        if !metadata.is_file() || metadata.len() > MAX_ARTWORK_BYTES {
            return Err(Error::Unsupported(
                "invalid or oversized cached artwork".into(),
            ));
        }
        return Ok(Some((path, extension)));
    }
    Ok(None)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_reporter_emits_monotonic_counts_under_out_of_order_completion() {
        let seen = Mutex::new(Vec::new());
        let record = |completed: usize, _total: usize| seen.lock().unwrap().push(completed);
        let past = Instant::now()
            .checked_sub(PROGRESS_INTERVAL)
            .unwrap_or_else(Instant::now);
        let reporter = ProgressReporter {
            progress: &record,
            total: 10,
            state: Mutex::new((past, 0)),
        };
        // Two workers can finish out of order; the reporter must still report a
        // non-decreasing count and end at the total.
        reporter.tick(2);
        reporter.tick(1);
        reporter.tick(5);
        reporter.tick(3);
        reporter.tick(10);
        let seen = seen.into_inner().unwrap();
        assert!(seen.windows(2).all(|pair| pair[0] <= pair[1]), "{seen:?}");
        assert_eq!(seen.last(), Some(&10));
    }
}
