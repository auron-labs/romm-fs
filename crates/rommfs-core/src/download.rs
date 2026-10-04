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
use crate::catalog::RomKey;
use crate::error::Result;
use crate::events::EventSink;
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

/// Shared, cloneable engine. All `ensure_ready` callers for the same key are
/// coalesced by an internal in-flight map.
pub struct DownloadManager {
    index: parking_lot::Mutex<CacheIndex>,
    live: std::sync::Arc<LiveState>,
    source: Arc<dyn ContentSource>,
    events: EventSink,
    /// Expected sizes by key (from catalogue) for completion verification.
    expected: std::collections::HashMap<RomKey, u64>,
    /// versions by key for invalidation bookkeeping.
    versions: std::collections::HashMap<RomKey, Option<String>>,
}

impl DownloadManager {
    pub fn new(
        index: CacheIndex,
        live: Arc<LiveState>,
        source: Arc<dyn ContentSource>,
        events: EventSink,
        expected: std::collections::HashMap<RomKey, u64>,
        versions: std::collections::HashMap<RomKey, Option<String>>,
    ) -> Self {
        Self { index: parking_lot::Mutex::new(index), live, source, events, expected, versions }
    }

    /// Path of the ready private file for `key`, downloading once if needed.
    /// Subsequent calls return the existing path without downloading.
    /// Errors fail every waiter and leave the entry not-ready.
    pub fn ensure_ready(&self, key: &RomKey) -> Result<PathBuf> {
        let _ = key;
        todo!("fast path: index.ready_if_current; single-flight via shared OnceCell-style slot; stream to .part; verify expected size; rename .bin; mark_ready")
    }

    /// True if `key` currently has a completed file on disk per the index.
    pub fn is_ready(&self, key: &RomKey) -> bool {
        let _ = key;
        todo!()
    }

    /// The live-state handle (active downloads + open guards share it).
    pub fn live(&self) -> Arc<LiveState> {
        Arc::clone(&self.live)
    }

    /// Mutable index access for eviction (short critical sections only).
    pub fn index(&self) -> &parking_lot::Mutex<CacheIndex> {
        &self.index
    }
}
