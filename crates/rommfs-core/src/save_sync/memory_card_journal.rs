//! Durable per-card baseline: the accepted RomM version id and the local card
//! hash at the last reconciliation. Stored in the same scoped SQLite file as
//! the save journal so a restart does not re-upload or re-conflict a card.

use super::memory_cards::CardBaseline;
use super::settings::{validate_scope, windows_path_key, SaveSyncScope};
use crate::error::{Error, Result};
use rusqlite::OptionalExtension;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryCardSlot {
    pub emulator: String,
    pub card_name: String,
    pub relative_path: String,
    pub baseline: CardBaseline,
    pub last_failure: Option<String>,
}

pub struct MemoryCardJournal {
    connection: rusqlite::Connection,
    scope: SaveSyncScope,
}

impl MemoryCardJournal {
    pub fn open(path: impl AsRef<Path>, scope: SaveSyncScope) -> Result<Self> {
        validate_scope(&scope)?;
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let connection = rusqlite::Connection::open(path).map_err(sqlite)?;
        // The save actor holds its own connection to this file; a short busy
        // timeout keeps a concurrent write from failing outright.
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(sqlite)?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS memory_card_slots (
                    server_id TEXT NOT NULL,
                    account_id TEXT NOT NULL,
                    installation_root TEXT NOT NULL,
                    effective_saves_root TEXT NOT NULL,
                    emulator TEXT NOT NULL,
                    card_name TEXT NOT NULL,
                    relative_path TEXT NOT NULL,
                    local_baseline_hash TEXT,
                    remote_version_id INTEGER,
                    remote_content_hash TEXT,
                    last_failure TEXT,
                    PRIMARY KEY (server_id, account_id, installation_root,
                                 effective_saves_root, emulator, card_name)
                );",
            )
            .map_err(sqlite)?;
        Ok(Self { connection, scope })
    }

    pub fn baseline(&self, emulator: &str, card_name: &str) -> Result<Option<CardBaseline>> {
        self.connection
            .query_row(
                "SELECT local_baseline_hash, remote_version_id, remote_content_hash
                 FROM memory_card_slots
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4 AND emulator = ?5 AND card_name = ?6",
                rusqlite::params![
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    emulator,
                    card_name,
                ],
                |row| {
                    Ok(CardBaseline {
                        local_hash: row.get(0)?,
                        version_id: row.get(1)?,
                        remote_hash: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(sqlite)
    }

    /// Accept a new baseline and clear any prior failure for the card.
    pub fn record(
        &self,
        emulator: &str,
        card_name: &str,
        relative_path: &str,
        baseline: &CardBaseline,
    ) -> Result<()> {
        self.upsert(emulator, card_name, relative_path, baseline, None)
    }

    /// Keep the baselines and note why the card needs review.
    pub fn record_failure(
        &self,
        emulator: &str,
        card_name: &str,
        relative_path: &str,
        message: &str,
    ) -> Result<()> {
        let baseline = self.baseline(emulator, card_name)?.unwrap_or_default();
        self.upsert(emulator, card_name, relative_path, &baseline, Some(message))
    }

    fn upsert(
        &self,
        emulator: &str,
        card_name: &str,
        relative_path: &str,
        baseline: &CardBaseline,
        failure: Option<&str>,
    ) -> Result<()> {
        self.connection
            .execute(
                "INSERT INTO memory_card_slots
                    (server_id, account_id, installation_root, effective_saves_root,
                     emulator, card_name, relative_path, local_baseline_hash,
                     remote_version_id, remote_content_hash, last_failure)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT (server_id, account_id, installation_root,
                              effective_saves_root, emulator, card_name)
                 DO UPDATE SET relative_path = excluded.relative_path,
                    local_baseline_hash = excluded.local_baseline_hash,
                    remote_version_id = excluded.remote_version_id,
                    remote_content_hash = excluded.remote_content_hash,
                    last_failure = excluded.last_failure",
                rusqlite::params![
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    emulator,
                    card_name,
                    relative_path,
                    baseline.local_hash,
                    baseline.version_id,
                    baseline.remote_hash,
                    failure,
                ],
            )
            .map_err(sqlite)?;
        Ok(())
    }

    pub fn slots(&self) -> Result<Vec<MemoryCardSlot>> {
        self.connection
            .prepare(
                "SELECT emulator, card_name, relative_path, local_baseline_hash,
                        remote_version_id, remote_content_hash, last_failure
                 FROM memory_card_slots
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4",
            )
            .map_err(sqlite)?
            .query_map(
                rusqlite::params![
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                ],
                |row| {
                    Ok(MemoryCardSlot {
                        emulator: row.get(0)?,
                        card_name: row.get(1)?,
                        relative_path: row.get(2)?,
                        baseline: CardBaseline {
                            local_hash: row.get(3)?,
                            version_id: row.get(4)?,
                            remote_hash: row.get(5)?,
                        },
                        last_failure: row.get(6)?,
                    })
                },
            )
            .map_err(sqlite)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sqlite)
    }
}

fn sqlite(error: rusqlite::Error) -> Error {
    Error::Io(std::io::Error::other(error))
}
