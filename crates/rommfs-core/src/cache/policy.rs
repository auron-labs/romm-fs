//! Inactivity-threshold eviction. `Evictor::sweep` removes expired Ready
//! entries while protecting: in-flight downloads, currently open files, and
//! entries re-acquired between check and delete.
//!
//! Coordination rule (PRD R4): last-use is checked and the guard held across
//! the whole remove; `ActiveGuard`s block eviction; uncertain lock state
//! defers. A busy file or failed cleanup is logged and retried next sweep.

use crate::cache::clock::Clock;
use crate::cache::{CacheIndex, HydratedRemover};
use crate::catalog::RomKey;
use crate::error::Result;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// What one sweep did — surfaced to logs/status.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EvictionOutcome {
    pub evicted: Vec<RomKey>,
    pub retained_active: Vec<RomKey>,
    pub deferred_failed: Vec<RomKey>,
}

/// In-memory liveness: active downloads + open-file guards per key.
#[derive(Default)]
pub struct LiveState {
    /// Keys currently downloading or with ≥1 open handle.
    active: Mutex<HashMap<RomKey, usize>>,
}

/// Held while a reader/open-file keeps a cache entry alive.
/// Dropping releases one reference.
pub struct ActiveGuard {
    key: RomKey,
    live: Arc<LiveState>,
}

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        let mut map = self.live.active.lock().unwrap();
        if let Some(count) = map.get_mut(&self.key) {
            *count -= 1;
            if *count == 0 {
                map.remove(&self.key);
            }
        }
    }
}

impl LiveState {
    /// Acquire an active reference for `key`; `None` is never returned —
    /// acquisition always succeeds; eviction consults the same map.
    /// `Arc<Self>` receiver: the returned guard owns a clone.
    pub fn acquire(self: &Arc<Self>, key: &RomKey) -> ActiveGuard {
        let mut map = self.active.lock().unwrap();
        *map.entry(key.clone()).or_insert(0) += 1;
        ActiveGuard {
            key: key.clone(),
            live: Arc::clone(self),
        }
    }

    /// Snapshot of keys with a nonzero active count.
    pub fn active_keys(&self) -> HashSet<RomKey> {
        self.active.lock().unwrap().keys().cloned().collect()
    }

    /// Run `action` while holding the active-map lock, but only when `key`
    /// has no active references. Returns `None` when the key is in use —
    /// the caller treats that as "retained", never as a failure.
    ///
    /// This is the check-then-delete race guard: an `acquire` that loses to
    /// a sweep runs only after the whole removal completed and therefore
    /// sees the row already gone (re-download path); an `acquire` that wins
    /// makes the sweep see the key active and skip it. The lock is held for
    /// the whole removal so no acquire can interleave mid-delete.
    fn map_lock_if_inactive<R>(&self, key: &RomKey, action: impl FnOnce() -> R) -> Option<R> {
        let map = self.active.lock().unwrap();
        if map.get(key).copied().unwrap_or(0) > 0 {
            return None;
        }
        Some(action())
    }
}

/// Sweeps on demand (the app calls it on a timer; tests call directly).
pub struct Evictor {
    pub threshold_secs: u64,
    live: Arc<LiveState>,
    hydrated: Arc<dyn HydratedRemover>,
}

impl Evictor {
    pub fn new(
        threshold_secs: u64,
        live: Arc<LiveState>,
        hydrated: Arc<dyn HydratedRemover>,
    ) -> Self {
        Self {
            threshold_secs,
            live,
            hydrated,
        }
    }

    /// One sweep over `index`: evict `last_used < now - threshold`, skipping
    /// active keys; on hydrated removal failure defer the whole entry.
    /// `rel_path_of` maps key -> `platform_dir/file_name` for the hydrated
    /// copy; `remove_file` deletes the private `.bin` (injectable for tests).
    pub fn sweep(
        &self,
        index: &mut CacheIndex,
        clock: &dyn Clock,
        rel_path_of: impl Fn(&RomKey) -> Option<String>,
        remove_file: impl Fn(&PathBuf) -> std::io::Result<()>,
    ) -> Result<EvictionOutcome> {
        let now = clock.unix_secs();
        let cutoff = now.saturating_sub(self.threshold_secs);
        let mut outcome = EvictionOutcome::default();

        for rec in index.ready_entries()? {
            if rec.last_used_unix_secs >= cutoff {
                continue;
            }
            let key = rec.key.clone();
            // The whole remove (hydrated copy, private .bin, index row) runs
            // under the live-map lock: a concurrent acquire either lands
            // first (key active -> retained) or after the row is gone
            // (-> its ensure_ready re-downloads). No interleaving.
            let ran = self.live.map_lock_if_inactive(&key, || {
                let rel = rel_path_of(&key)
                    .or_else(|| (!rec.rel_path.is_empty()).then(|| rec.rel_path.clone()));
                evict_one(
                    &*self.hydrated,
                    rel.as_deref(),
                    &rec.path,
                    &remove_file,
                    index,
                    &key,
                )
            });
            match ran {
                None => outcome.retained_active.push(key),
                Some(Err(e)) => {
                    tracing::warn!(rom_id = key.rom_id, error = %e, "eviction deferred");
                    outcome.deferred_failed.push(key);
                }
                Some(Ok(())) => outcome.evicted.push(key),
            }
        }
        Ok(outcome)
    }
}

/// Remove one evictable entry: the platform-managed hydrated copy first
/// (its failure defers everything else), then the private `.bin` (a missing
/// file is already-evicted, not an error), then the durable row.
fn evict_one(
    hydrated: &dyn HydratedRemover,
    rel_path: Option<&str>,
    bin: &PathBuf,
    remove_file: &dyn Fn(&PathBuf) -> std::io::Result<()>,
    index: &mut CacheIndex,
    key: &RomKey,
) -> Result<()> {
    if let Some(rel) = rel_path {
        hydrated
            .remove_hydrated(rel)
            .map_err(crate::error::Error::Io)?;
    }
    match remove_file(bin) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(crate::error::Error::Io(e)),
    }
    index.remove(key)
}
