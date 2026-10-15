//! Complete source snapshots, separate from evictable ROM bytes.
use crate::{
    romm::{PlatformDto, RomDto},
    Error, Result,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;

const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatalogueSnapshot {
    pub platforms: Vec<PlatformDto>,
    pub roms: Vec<RomDto>,
    /// Start time of the refresh that produced this snapshot.
    pub refreshed_at: u64,
    /// Start time of the last *full* refresh. A snapshot older than the full
    /// refresh interval gets a full refresh again, because the server's
    /// `updated_at` delta may not cover a rescan that only changed files.
    /// Absent (0) on a snapshot written before this field existed, which forces
    /// one full refresh.
    #[serde(default)]
    pub full_refreshed_at: u64,
}

pub struct CatalogueStore(Connection);

impl CatalogueStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = Connection::open(path).map_err(sqlite)?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS catalogue_snapshots (
                server TEXT NOT NULL, account TEXT NOT NULL, version INTEGER NOT NULL,
                payload TEXT NOT NULL, PRIMARY KEY (server, account));",
        )
        .map_err(sqlite)?;
        Ok(Self(db))
    }

    pub fn load(&self, server: &str, account: &str) -> Result<Option<CatalogueSnapshot>> {
        let row: Option<(u32, String)> = self
            .0
            .query_row(
                "SELECT version, payload FROM catalogue_snapshots WHERE server=? AND account=?",
                params![server, account],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(sqlite)?;
        match row {
            Some((SCHEMA_VERSION, payload)) => serde_json::from_str(&payload)
                .map(Some)
                .map_err(|e| Error::InvalidCatalogue(format!("cached catalogue: {e}"))),
            // An incompatible schema is a cache miss, never an empty catalogue.
            _ => Ok(None),
        }
    }

    /// Called only after every catalogue page succeeds. One atomic SQLite write
    /// retains the previous complete payload if persistence fails.
    pub fn save(&self, server: &str, account: &str, snapshot: &CatalogueSnapshot) -> Result<()> {
        let payload =
            serde_json::to_string(snapshot).map_err(|e| Error::InvalidCatalogue(e.to_string()))?;
        self.0.execute("INSERT INTO catalogue_snapshots VALUES (?, ?, ?, ?)
            ON CONFLICT(server, account) DO UPDATE SET version=excluded.version, payload=excluded.payload",
            params![server, account, SCHEMA_VERSION, payload]).map_err(sqlite)?;
        Ok(())
    }
}

fn sqlite(error: rusqlite::Error) -> Error {
    Error::Io(std::io::Error::other(error))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_snapshots_survive_restart_and_are_account_and_server_scoped() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("catalogue.db");
        let snapshot = CatalogueSnapshot {
            platforms: vec![],
            roms: vec![],
            refreshed_at: 42,
            full_refreshed_at: 42,
        };
        CatalogueStore::open(&path)
            .unwrap()
            .save("server", "account", &snapshot)
            .unwrap();
        let store = CatalogueStore::open(&path).unwrap();
        assert_eq!(
            store
                .load("server", "account")
                .unwrap()
                .unwrap()
                .refreshed_at,
            42
        );
        assert!(store.load("other", "account").unwrap().is_none());
        assert!(store.load("server", "other").unwrap().is_none());
        store.0.execute_batch("CREATE TRIGGER reject_update BEFORE UPDATE ON catalogue_snapshots BEGIN SELECT RAISE(ABORT, 'disk failure'); END;").unwrap();
        let changed = CatalogueSnapshot {
            refreshed_at: 99,
            ..snapshot
        };
        assert!(store.save("server", "account", &changed).is_err());
        assert_eq!(
            store
                .load("server", "account")
                .unwrap()
                .unwrap()
                .refreshed_at,
            42
        );
    }

    #[test]
    fn a_snapshot_written_before_full_refreshed_at_loads_with_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("catalogue.db");
        let store = CatalogueStore::open(&path).unwrap();
        // A v1 payload has no `full_refreshed_at`; it must still load, with a
        // zero that forces one full refresh.
        store
            .0
            .execute(
                "INSERT INTO catalogue_snapshots VALUES ('s', 'a', 1, ?1)",
                params![r#"{"platforms":[],"roms":[],"refreshed_at":7}"#],
            )
            .unwrap();
        let loaded = store.load("s", "a").unwrap().unwrap();
        assert_eq!(loaded.refreshed_at, 7);
        assert_eq!(loaded.full_refreshed_at, 0);
    }
}
