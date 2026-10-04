//! SQLite-backed cache index at `<cache_dir>/cache-index.db` (rusqlite,
//! bundled). Durable facts only: completion state, size, version, last_use.
//! In-flight downloads and active-use tracking live in memory.

use crate::catalog::RomKey;
use crate::error::Result;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryState {
    /// Bytes complete and verified on disk.
    Ready,
    /// Download failed; a later read may retry. (Never served.)
    Failed,
}

/// Public view of one durable row.
#[derive(Clone, Debug)]
pub struct CacheRecord {
    pub key: RomKey,
    pub state: EntryState,
    /// Private cache file path (`<cache_dir>/<cache_stem>.bin`).
    pub path: PathBuf,
    pub size_bytes: u64,
    pub version: Option<String>,
    pub last_used_unix_secs: u64,
}

/// The index. `open` also runs recovery: any leftover `.part` files are
/// deleted and rows not `Ready` are dropped — incomplete data is never
/// treated as complete after a restart.
pub struct CacheIndex {
    cache_dir: PathBuf,
    conn: rusqlite::Connection,
}

impl CacheIndex {
    /// Open or create the index under `cache_dir` and recover from any
    /// interrupted state.
    pub fn open(cache_dir: impl AsRef<Path>) -> Result<Self> {
        let _ = cache_dir.as_ref();
        todo!("create dir, open sqlite, migrate schema, delete *.part, drop non-ready rows")
    }

    /// Expected private file path for an entry (`.bin` published name).
    pub fn bin_path(&self, key: &RomKey) -> PathBuf {
        self.cache_dir.join(format!("{}.bin", key.cache_stem()))
    }

    /// In-progress staging path (never returned to readers).
    pub fn part_path(&self, key: &RomKey) -> PathBuf {
        self.cache_dir.join(format!("{}.part", key.cache_stem()))
    }

    /// Persist `Ready` + size + version for `key` (called after atomic publish).
    pub fn mark_ready(&mut self, key: &RomKey, size: u64, version: Option<&str>) -> Result<()> {
        let _ = (key, size, version);
        todo!()
    }

    /// Record a failure (transient); keeps the row but never `Ready`.
    pub fn mark_failed(&mut self, key: &RomKey) -> Result<()> {
        let _ = key;
        todo!()
    }

    /// Update last-use timestamp (access tracking — NOT download time).
    pub fn touch(&mut self, key: &RomKey, unix_secs: u64) -> Result<()> {
        let _ = (key, unix_secs);
        todo!()
    }

    /// Completed record whose stored version matches `version` (a `None`
    /// stored/provided pair compares equal only when both are `None` or the
    /// new catalogue supplies no version — see policy tests).
    pub fn ready_if_current(&self, key: &RomKey, version: Option<&str>) -> Result<Option<CacheRecord>> {
        let _ = (key, version);
        todo!()
    }

    /// Remove the durable row (after files were removed).
    pub fn remove(&mut self, key: &RomKey) -> Result<()> {
        let _ = key;
        todo!()
    }

    /// Rows eligible for eviction consideration (Ready, oldest first).
    pub fn ready_entries(&self) -> Result<Vec<CacheRecord>> {
        todo!()
    }
}
