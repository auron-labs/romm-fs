//! SQLite-backed cache index at `<cache_dir>/cache-index.db` (rusqlite,
//! bundled). Durable facts only: completion state, size, version, last_use.
//! In-flight downloads and active-use tracking live in memory.

use crate::catalog::RomKey;
use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

const DB_FILE: &str = "cache-index.db";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryState {
    /// Bytes complete and verified on disk.
    Ready,
    /// Download failed; a later read may retry. (Never served.)
    Failed,
}

impl EntryState {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "ready" => Some(EntryState::Ready),
            "failed" => Some(EntryState::Failed),
            _ => None,
        }
    }
}

/// Public view of one durable row.
#[derive(Clone, Debug)]
pub struct CacheRecord {
    pub key: RomKey,
    pub state: EntryState,
    /// Private cache file path (`<cache_dir>/<cache_stem>.bin`).
    pub path: PathBuf,
    /// Projected relative path recorded at publish time
    /// (`platform_dir/file_name`); empty when unknown. Fallback for the
    /// hydrated-removal path when the live tree cannot map the key.
    pub rel_path: String,
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
        let cache_dir = cache_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&cache_dir)?;
        let conn = rusqlite::Connection::open(cache_dir.join(DB_FILE)).map_err(sqlite)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS cache_entries (
                server_id            TEXT NOT NULL,
                rom_id               INTEGER NOT NULL,
                file_id              INTEGER NOT NULL,
                rel_path             TEXT NOT NULL DEFAULT '',
                version              TEXT,
                state                TEXT NOT NULL,
                size_bytes           INTEGER NOT NULL,
                last_used_unix_secs  INTEGER NOT NULL,
                PRIMARY KEY (server_id, rom_id, file_id)
            );",
        )
        .map_err(sqlite)?;
        let index = Self { cache_dir, conn };
        index.recover()?;
        Ok(index)
    }

    /// Startup recovery: remove stale `.part` staging files and any row that
    /// is not `Ready`. A crash between `.part` write and rename must leave no
    /// partial data servable; non-ready rows are never trusted anyway.
    fn recover(&self) -> Result<()> {
        self.conn
            .execute("DELETE FROM cache_entries WHERE state != 'ready'", [])
            .map_err(sqlite)?;
        if let Ok(read_dir) = std::fs::read_dir(&self.cache_dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "part") {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        Ok(())
    }

    /// Expected private file path for an entry (`.bin` published name).
    pub fn bin_path(&self, key: &RomKey) -> PathBuf {
        self.cache_dir.join(format!("{}.bin", key.cache_stem()))
    }

    /// In-progress staging path (never returned to readers).
    pub fn part_path(&self, key: &RomKey) -> PathBuf {
        self.cache_dir.join(format!("{}.part", key.cache_stem()))
    }

    /// Persist `Ready` + size + version + rel path for `key` (called after
    /// atomic publish). `last_used` is stamped with wall-clock time only so a
    /// fresh entry is not instantly eviction-eligible; real access tracking
    /// always goes through `touch` (PRD: download time is not last-use).
    pub fn mark_ready(
        &mut self,
        key: &RomKey,
        size: u64,
        version: Option<&str>,
        rel_path: &str,
    ) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO cache_entries
                 (server_id, rom_id, file_id, rel_path, version, state,
                  size_bytes, last_used_unix_secs)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'ready', ?6, ?7)",
                rusqlite::params![
                    key.server_id,
                    key.rom_id,
                    key.file_id,
                    rel_path,
                    version,
                    size as i64,
                    crate::cache::now_unix_secs() as i64,
                ],
            )
            .map_err(sqlite)?;
        Ok(())
    }

    /// Record a failure (transient); keeps the row but never `Ready`.
    /// Other fields (rel path, last use) are preserved when a row exists.
    pub fn mark_failed(&mut self, key: &RomKey) -> Result<()> {
        let changed = self
            .conn
            .execute(
                "UPDATE cache_entries SET state = 'failed'
                 WHERE server_id = ?1 AND rom_id = ?2 AND file_id = ?3",
                rusqlite::params![key.server_id, key.rom_id, key.file_id],
            )
            .map_err(sqlite)?;
        if changed == 0 {
            self.conn
                .execute(
                    "INSERT INTO cache_entries
                     (server_id, rom_id, file_id, rel_path, version, state,
                      size_bytes, last_used_unix_secs)
                     VALUES (?1, ?2, ?3, '', NULL, 'failed', 0, 0)",
                    rusqlite::params![key.server_id, key.rom_id, key.file_id],
                )
                .map_err(sqlite)?;
        }
        Ok(())
    }

    /// Update last-use timestamp (access tracking — NOT download time).
    pub fn touch(&mut self, key: &RomKey, unix_secs: u64) -> Result<()> {
        self.conn
            .execute(
                "UPDATE cache_entries SET last_used_unix_secs = ?4
                 WHERE server_id = ?1 AND rom_id = ?2 AND file_id = ?3",
                rusqlite::params![key.server_id, key.rom_id, key.file_id, unix_secs as i64],
            )
            .map_err(sqlite)?;
        Ok(())
    }

    /// Completed record whose stored version matches `version` (a `None`
    /// stored/provided pair compares equal only when both are `None` or the
    /// new catalogue supplies no version — see policy tests).
    pub fn ready_if_current(
        &self,
        key: &RomKey,
        version: Option<&str>,
    ) -> Result<Option<CacheRecord>> {
        let record = self.record_for(key)?;
        match record {
            Some(rec) if rec.state == EntryState::Ready => {
                match std::fs::metadata(&rec.path) {
                    Ok(metadata) if metadata.is_file() => {}
                    Ok(_) => {
                        self.remove(key)?;
                        return Ok(None);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        self.remove(key)?;
                        return Ok(None);
                    }
                    Err(e) => return Err(Error::Io(e)),
                }
                // No version supplied by the current catalogue cannot
                // invalidate: the stored bytes are the best we have.
                match version {
                    None => Ok(Some(rec)),
                    Some(v) if rec.version.as_deref() == Some(v) => Ok(Some(rec)),
                    Some(_) => Ok(None),
                }
            }
            _ => Ok(None),
        }
    }

    /// Remove the durable row (after files were removed).
    pub fn remove(&self, key: &RomKey) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM cache_entries
                 WHERE server_id = ?1 AND rom_id = ?2 AND file_id = ?3",
                rusqlite::params![key.server_id, key.rom_id, key.file_id],
            )
            .map_err(sqlite)?;
        Ok(())
    }

    /// Rows eligible for eviction consideration (Ready, oldest first).
    pub fn ready_entries(&self) -> Result<Vec<CacheRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT server_id, rom_id, file_id, rel_path, version, state,
                        size_bytes, last_used_unix_secs
                 FROM cache_entries WHERE state = 'ready'
                 ORDER BY last_used_unix_secs ASC",
            )
            .map_err(sqlite)?;
        let rows = stmt
            .query_map([], |row| self.record_from(row))
            .map_err(sqlite)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(sqlite)?);
        }
        Ok(out)
    }

    fn record_for(&self, key: &RomKey) -> Result<Option<CacheRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT server_id, rom_id, file_id, rel_path, version, state,
                        size_bytes, last_used_unix_secs
                 FROM cache_entries
                 WHERE server_id = ?1 AND rom_id = ?2 AND file_id = ?3",
            )
            .map_err(sqlite)?;
        let mut rows = stmt
            .query(rusqlite::params![key.server_id, key.rom_id, key.file_id])
            .map_err(sqlite)?;
        match rows.next().map_err(sqlite)? {
            Some(row) => Ok(Some(self.record_from(row).map_err(sqlite)?)),
            None => Ok(None),
        }
    }

    fn record_from(&self, row: &rusqlite::Row<'_>) -> rusqlite::Result<CacheRecord> {
        let server_id: String = row.get(0)?;
        let rom_id: i64 = row.get(1)?;
        let file_id: i64 = row.get(2)?;
        let rel_path: String = row.get(3)?;
        let version: Option<String> = row.get(4)?;
        let state: String = row.get(5)?;
        let size_bytes: i64 = row.get(6)?;
        let last_used: i64 = row.get(7)?;
        let key = RomKey {
            server_id,
            rom_id,
            file_id,
        };
        Ok(CacheRecord {
            path: self.bin_path(&key),
            key,
            state: EntryState::from_str(&state).unwrap_or(EntryState::Failed),
            rel_path,
            size_bytes: size_bytes.max(0) as u64,
            version,
            last_used_unix_secs: last_used.max(0) as u64,
        })
    }
}

fn sqlite(e: rusqlite::Error) -> crate::error::Error {
    crate::error::Error::Io(std::io::Error::other(e))
}
