//! Download engine (PRD R3): first content read -> one complete download into
//! the private cache, atomic publish, single-flight per ROM, waiters released
//! on failure, retry allowed on the next read.
//!
//! - Streams to `<key>.part`, verifies expected length, renames to `.bin`.
//! - `ensure_ready` never returns a partial path; `.part` is never visible.
//! - Concurrent `ensure_ready` for the same key share one transfer via a
//!   shared result slot; no global lock is held across network IO.
//! - Progress events go to the `EventSink`; stop releases pending work.

use crate::cache::{CacheIndex, LiveState};
use crate::catalog::{RomEntry, RomKey};
use crate::error::{Error, Result};
use crate::events::{AppEvent, EventSink};
use crate::romm::RommClient;
use parking_lot::{Condvar, Mutex};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Fetch one complete ROM file: streams bytes into the staging writer.
/// Implemented by the RomM client adapter; injectable in tests.
pub trait ContentSource: Send + Sync {
    /// Download the content for `key` into `writer`, reporting progress via
    /// `progress(received, total)`. Must Err on failure — never partial-ok.
    fn fetch(
        &self,
        key: &RomKey,
        writer: &mut dyn std::io::Write,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<u64>;
}

/// The real client is a `ContentSource`. Per the verified RomM 5.3.1
/// contract, `GET /api/roms/{id}/content/{file_name}` selects the ROM's
/// files server-side — `{file_name}` is only the zip output name — so for
/// the single-file ROMs this PoC projects, the stable `file_id` is passed
/// as a harmless deterministic name.
impl ContentSource for RommClient {
    fn fetch(
        &self,
        key: &RomKey,
        writer: &mut dyn std::io::Write,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<u64> {
        self.download_file(key.rom_id, &key.file_id.to_string(), writer, progress)
    }
}

/// One shared transfer: waiters block on `done` until the leader stores a
/// terminal `state` and notifies. The error is shared as text (the
/// `Error` enum is not `Clone`); the leader still returns the real error.
struct Flight {
    state: Mutex<FlightState>,
    done: Condvar,
}

enum FlightState {
    Running,
    Finished(std::result::Result<PathBuf, String>),
}

impl Flight {
    fn new() -> Self {
        Self {
            state: Mutex::new(FlightState::Running),
            done: Condvar::new(),
        }
    }
}

/// Shared, cloneable engine. All `ensure_ready` callers for the same key are
/// coalesced by an internal in-flight map.
pub struct DownloadManager {
    index: Mutex<CacheIndex>,
    live: std::sync::Arc<LiveState>,
    source: Arc<dyn ContentSource>,
    events: EventSink,
    /// Expected sizes by key (from catalogue) for completion verification.
    expected: HashMap<RomKey, u64>,
    /// versions by key for invalidation bookkeeping.
    versions: HashMap<RomKey, Option<String>>,
    /// key -> shared transfer slot for single-flight coalescing.
    in_flight: Mutex<HashMap<RomKey, Arc<Flight>>>,
}

impl DownloadManager {
    pub fn new(
        index: CacheIndex,
        live: Arc<LiveState>,
        source: Arc<dyn ContentSource>,
        events: EventSink,
        expected: HashMap<RomKey, u64>,
        versions: HashMap<RomKey, Option<String>>,
    ) -> Self {
        Self {
            index: Mutex::new(index),
            live,
            source,
            events,
            expected,
            versions,
            in_flight: Mutex::new(HashMap::new()),
        }
    }

    /// Path of the ready private file for `entry`, downloading once if
    /// needed. Subsequent calls return the existing path without
    /// downloading. Errors fail every waiter and leave the entry not-ready.
    pub fn ensure_ready(&self, entry: &RomEntry) -> Result<PathBuf> {
        let key = &entry.key;
        let version = entry.version.as_ref().map(|v| v.0.as_str());

        // Fast path: completed bytes whose stored version still matches the
        // current catalogue — no download, no lock held past the lookup.
        if let Some(rec) = self.index.lock().ready_if_current(key, version)? {
            return Ok(rec.path);
        }

        // Single-flight: first caller becomes the leader and fetches;
        // concurrent callers block on the shared slot and share the result.
        let (flight, leader) = {
            let mut map = self.in_flight.lock();
            match map.get(key) {
                Some(f) => (Arc::clone(f), false),
                None => {
                    let f = Arc::new(Flight::new());
                    map.insert(key.clone(), Arc::clone(&f));
                    (f, true)
                }
            }
        };

        if !leader {
            let mut state = flight.state.lock();
            while matches!(*state, FlightState::Running) {
                flight.done.wait(&mut state);
            }
            return match &*state {
                FlightState::Finished(Ok(path)) => Ok(path.clone()),
                FlightState::Finished(Err(msg)) => Err(Error::Transport(format!(
                    "download for rom {} failed: {msg}",
                    key.rom_id
                ))),
                FlightState::Running => unreachable!(),
            };
        }

        // Leader: hold an active reference across the whole transfer so a
        // sweep can never evict the entry being produced (PRD R4).
        let _activity = self.live.acquire(key);
        let outcome = self.download_one(entry, version);
        {
            let mut state = flight.state.lock();
            *state = match &outcome {
                Ok(path) => FlightState::Finished(Ok(path.clone())),
                Err(e) => FlightState::Finished(Err(e.to_string())),
            };
            flight.done.notify_all();
        }
        self.in_flight.lock().remove(key);
        outcome
    }

    /// Fetch + publish for the leader. Runs only when the entry is not
    /// ready for the current version, so a pre-existing `.bin` is stale
    /// and safe to discard on failure.
    fn download_one(&self, entry: &RomEntry, version: Option<&str>) -> Result<PathBuf> {
        let key = &entry.key;
        // Re-check: a completed transfer may have landed between the fast
        // path and leadership — never fetch bytes already on disk.
        if let Some(rec) = self.index.lock().ready_if_current(key, version)? {
            return Ok(rec.path);
        }

        let (part, bin) = {
            let index = self.index.lock();
            (index.part_path(key), index.bin_path(key))
        };
        let expected = self.expected.get(key).copied().unwrap_or(entry.size);
        let rom_id = key.rom_id.max(0) as u64;
        self.events.emit(AppEvent::DownloadStarted {
            rom_id,
            file_name: entry.file_name.clone(),
            total: Some(expected),
        });

        let rel_path = format!("{}/{}", entry.platform_dir, entry.file_name);
        let result = self.fetch_and_publish(key, &part, &bin, expected, version, &rel_path);
        match &result {
            Ok(_) => self.events.emit(AppEvent::DownloadFinished {
                rom_id,
                file_name: entry.file_name.clone(),
            }),
            Err(e) => {
                let _ = std::fs::remove_file(&part);
                // Any leftover .bin is stale/unservable at this point.
                let _ = std::fs::remove_file(&bin);
                let _ = self.index.lock().mark_failed(key);
                self.events.emit(AppEvent::DownloadFailed {
                    rom_id,
                    file_name: entry.file_name.clone(),
                    reason: e.to_string(),
                });
            }
        }
        result
    }

    /// Stream into `.part`, verify the expected length, atomically publish
    /// as `.bin`, then persist the Ready row (rel path kept for the
    /// hydrated-removal fallback on eviction).
    fn fetch_and_publish(
        &self,
        key: &RomKey,
        part: &PathBuf,
        bin: &PathBuf,
        expected: u64,
        version: Option<&str>,
        rel_path: &str,
    ) -> Result<PathBuf> {
        let mut file = std::fs::File::create(part).map_err(Error::Io)?;
        let rom_id = key.rom_id.max(0) as u64;
        let events = self.events.clone();
        let mut progress = move |received: u64, total: Option<u64>| {
            events.emit(AppEvent::DownloadProgress {
                rom_id,
                received,
                total,
            });
        };
        let written = self.source.fetch(key, &mut file, &mut progress);
        // The file handle must be released before the publish rename —
        // renaming an open file is an error on Windows.
        let written = match written {
            Ok(w) => {
                use std::io::Write;
                file.flush().map_err(Error::Io)?;
                w
            }
            Err(e) => {
                drop(file);
                return Err(e);
            }
        };
        drop(file);

        if written < expected {
            return Err(Error::Truncated {
                expected,
                received: written,
            });
        }
        std::fs::rename(part, bin).map_err(Error::Io)?;
        self.index
            .lock()
            .mark_ready(key, written, version, rel_path)?;
        Ok(bin.clone())
    }

    /// True if `key` currently has a completed file on disk per the index.
    pub fn is_ready(&self, key: &RomKey) -> bool {
        let version = self.versions.get(key).cloned().flatten();
        self.index
            .lock()
            .ready_if_current(key, version.as_deref())
            .map(|r| r.is_some())
            .unwrap_or(false)
    }

    /// The live-state handle (active downloads + open guards share it).
    pub fn live(&self) -> Arc<LiveState> {
        Arc::clone(&self.live)
    }

    /// Mutable index access for eviction (short critical sections only).
    pub fn index(&self) -> &Mutex<CacheIndex> {
        &self.index
    }
}
