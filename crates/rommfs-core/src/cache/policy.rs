//! Inactivity-threshold eviction. `Evictor::sweep` removes expired Ready
//! entries while protecting: in-flight downloads, currently open files, and
//! entries re-acquired between check and delete.
//!
//! Coordination rule (PRD R4): last-use is checked and the guard held across
//! the whole remove; `ActiveGuard`s block eviction; uncertain lock state
//! defers. A busy file or failed cleanup is logged and retried next sweep.

use crate::cache::{CacheIndex, HydratedRemover};
use crate::cache::clock::Clock;
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
        todo!("decrement; remove at 0")
    }
}

impl LiveState {
    /// Acquire an active reference for `key`; `None` is never returned —
    /// acquisition always succeeds; eviction consults the same map.
    pub fn acquire(&self, key: &RomKey) -> ActiveGuard {
        let _ = key;
        todo!()
    }

    /// Snapshot of keys with a nonzero active count.
    pub fn active_keys(&self) -> HashSet<RomKey> {
        todo!()
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
        Self { threshold_secs, live, hydrated }
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
        let _ = (index, clock, rel_path_of, remove_file);
        todo!()
    }
}
