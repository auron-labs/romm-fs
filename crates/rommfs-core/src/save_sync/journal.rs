use super::path::ensure_no_reparse_components;
use super::settings::{validate_scope, windows_path_key, SaveSyncScope};
use crate::catalogue::RomKey;
use crate::error::{Error, Result};
use std::fs;
use std::path::{Path, PathBuf};

mod incoming;
mod mapping;
mod snapshot;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotState {
    Preparing,
    Ready,
    Failed,
    RemoteInFlight,
    RemoteAmbiguous,
    RemoteComplete,
    Superseded,
}

impl SnapshotState {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Preparing => "preparing",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::RemoteInFlight => "remote_in_flight",
            Self::RemoteAmbiguous => "remote_ambiguous",
            Self::RemoteComplete => "remote_complete",
            Self::Superseded => "superseded",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self> {
        match value {
            "preparing" => Ok(Self::Preparing),
            "ready" => Ok(Self::Ready),
            "failed" => Ok(Self::Failed),
            "remote_in_flight" => Ok(Self::RemoteInFlight),
            "remote_ambiguous" => Ok(Self::RemoteAmbiguous),
            "remote_complete" => Ok(Self::RemoteComplete),
            "superseded" => Ok(Self::Superseded),
            _ => Err(Error::Unsupported(format!(
                "unknown save snapshot state {value:?}"
            ))),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalObservation {
    pub generation: u64,
    pub content_hash: Option<String>,
    pub changed: bool,
    pub present: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalSlot {
    pub rom_key: RomKey,
    pub profile_id: String,
    pub relative_path: PathBuf,
    pub proposed_relative_path: Option<PathBuf>,
    pub mapping_confirmed: bool,
    pub candidate_available: bool,
    pub needs_attention: bool,
    pub current_local_hash: Option<String>,
    pub local_generation: u64,
    pub local_ever_existed: bool,
    pub local_removed: bool,
    /// Local bytes accepted at the last three-way reconciliation. This is
    /// deliberately distinct from `current_local_hash`, which keeps moving
    /// as RetroBat writes the live SRAM.
    pub local_baseline_hash: Option<String>,
    pub remote_baseline_hash: Option<String>,
    pub remote_slot_id: Option<String>,
    /// Exact IDs from the last verified inventory. Older owned revisions in
    /// RomM history stay recognized without trusting array order.
    pub remote_history_ids: Vec<String>,
    pub conflict_resolution: Option<String>,
    pub conflict_local_hash: Option<String>,
    pub conflict_history_ids: Vec<String>,
    pub remote_history_fingerprint: String,
    pub conflict_history_fingerprint: String,
    pub snapshot_revision: Option<String>,
    pub snapshot_path: Option<PathBuf>,
    pub snapshot_hash: Option<String>,
    pub snapshot_generation: Option<u64>,
    pub remote_outcome: Option<String>,
    pub last_failure: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncomingSaveRecord {
    pub id: String,
    pub rom_key: RomKey,
    pub remote_id: String,
    pub content_hash: String,
    pub size: u64,
    pub reason: String,
    pub path: PathBuf,
    pub state: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotRecord {
    pub revision: String,
    pub rom_key: RomKey,
    pub generation: u64,
    pub content_hash: String,
    pub state: SnapshotState,
    pub path: PathBuf,
    pub remote_slot_id: Option<String>,
    pub remote_outcome: Option<String>,
    pub failure: Option<String>,
}

/// A private SQLite journal and immutable snapshot store. It is deliberately
/// independent of the ROM content cache and exposes no network operations.
pub struct SaveSyncJournal {
    connection: rusqlite::Connection,
    scope: SaveSyncScope,
    private_root: PathBuf,
}

impl SaveSyncJournal {
    pub fn open(
        database_path: impl AsRef<Path>,
        private_root: impl AsRef<Path>,
        scope: SaveSyncScope,
    ) -> Result<Self> {
        validate_scope(&scope)?;
        let database_path = database_path.as_ref();
        if let Some(parent) = database_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        let private_root = private_root.as_ref().to_path_buf();
        ensure_no_reparse_components(&private_root)?;
        fs::create_dir_all(&private_root)?;
        ensure_no_reparse_components(&private_root)?;
        set_private_directory_mode(&private_root)?;

        let connection = rusqlite::Connection::open(database_path).map_err(sqlite)?;
        set_private_file_mode(database_path)?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS save_sync_slots (
                    server_id TEXT NOT NULL,
                    account_id TEXT NOT NULL,
                    installation_root TEXT NOT NULL,
                    effective_saves_root TEXT NOT NULL,
                    rom_server_id TEXT NOT NULL,
                    rom_id INTEGER NOT NULL,
                    file_id INTEGER NOT NULL,
                    profile_id TEXT NOT NULL,
                    relative_path TEXT NOT NULL,
                    proposed_relative_path TEXT,
                    mapping_confirmed INTEGER NOT NULL DEFAULT 0 CHECK (mapping_confirmed IN (0, 1)),
                    candidate_available INTEGER NOT NULL DEFAULT 1 CHECK (candidate_available IN (0, 1)),
                    needs_attention INTEGER NOT NULL DEFAULT 0 CHECK (needs_attention IN (0, 1)),
                    current_local_hash TEXT,
                    local_baseline_hash TEXT,
                    local_generation INTEGER NOT NULL DEFAULT 0 CHECK (local_generation >= 0),
                    local_ever_existed INTEGER NOT NULL DEFAULT 0 CHECK (local_ever_existed IN (0, 1)),
                    local_removed INTEGER NOT NULL DEFAULT 0 CHECK (local_removed IN (0, 1)),
                    remote_baseline_hash TEXT,
                    remote_slot_id TEXT,
                    remote_history_ids TEXT NOT NULL DEFAULT '',
                    conflict_resolution TEXT,
                    conflict_local_hash TEXT,
                    conflict_history_ids TEXT NOT NULL DEFAULT '',
                    remote_history_fingerprint TEXT NOT NULL DEFAULT '',
                    conflict_history_fingerprint TEXT NOT NULL DEFAULT '',
                    snapshot_revision TEXT,
                    snapshot_path TEXT,
                    snapshot_hash TEXT,
                    snapshot_generation INTEGER,
                    remote_outcome TEXT,
                    last_failure TEXT,
                    PRIMARY KEY (server_id, account_id, installation_root, effective_saves_root,
                                rom_server_id, rom_id, file_id)
                );
                CREATE TABLE IF NOT EXISTS save_sync_snapshots (
                    server_id TEXT NOT NULL,
                    account_id TEXT NOT NULL,
                    installation_root TEXT NOT NULL,
                    effective_saves_root TEXT NOT NULL,
                    revision TEXT NOT NULL,
                    rom_server_id TEXT NOT NULL,
                    rom_id INTEGER NOT NULL,
                    file_id INTEGER NOT NULL,
                    generation INTEGER NOT NULL CHECK (generation >= 0),
                    content_hash TEXT NOT NULL,
                    state TEXT NOT NULL,
                    temporary_path TEXT,
                    snapshot_path TEXT NOT NULL,
                    remote_slot_id TEXT,
                    remote_outcome TEXT,
                    failure TEXT,
                    PRIMARY KEY (server_id, account_id, installation_root, effective_saves_root, revision)
                );
                CREATE INDEX IF NOT EXISTS save_sync_snapshot_state
                    ON save_sync_snapshots (server_id, account_id, installation_root,
                                            effective_saves_root, state);",
            )
            .map_err(sqlite)?;

        // The P3 journal predates local three-way baselines. Additive migration
        // keeps existing upload and mapping history intact.
        ensure_column(
            &connection,
            "save_sync_slots",
            "local_baseline_hash",
            "TEXT",
        )?;
        ensure_column(
            &connection,
            "save_sync_slots",
            "remote_history_ids",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(
            &connection,
            "save_sync_slots",
            "conflict_resolution",
            "TEXT",
        )?;
        ensure_column(
            &connection,
            "save_sync_slots",
            "conflict_local_hash",
            "TEXT",
        )?;
        ensure_column(
            &connection,
            "save_sync_slots",
            "conflict_history_ids",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(
            &connection,
            "save_sync_slots",
            "remote_history_fingerprint",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(
            &connection,
            "save_sync_slots",
            "conflict_history_fingerprint",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS save_sync_incoming (
                    server_id TEXT NOT NULL,
                    account_id TEXT NOT NULL,
                    installation_root TEXT NOT NULL,
                    effective_saves_root TEXT NOT NULL,
                    incoming_id TEXT NOT NULL,
                    rom_server_id TEXT NOT NULL,
                    rom_id INTEGER NOT NULL,
                    file_id INTEGER NOT NULL,
                    remote_id TEXT NOT NULL,
                    content_hash TEXT NOT NULL,
                     size INTEGER NOT NULL CHECK (size >= 0),
                    reason TEXT NOT NULL,
                    state TEXT NOT NULL,
                    staged_path TEXT NOT NULL,
                    baseline_local_hash TEXT,
                    baseline_remote_id TEXT,
                    baseline_remote_hash TEXT,
                    PRIMARY KEY (server_id, account_id, installation_root,
                                 effective_saves_root, incoming_id)
                );
                CREATE INDEX IF NOT EXISTS save_sync_incoming_state
                    ON save_sync_incoming (server_id, account_id, installation_root,
                                           effective_saves_root, state);",
            )
            .map_err(sqlite)?;

        // Additive ordinary-file migration: keep all incoming records and
        // baselines, but allow an API-valid empty file. SQLite cannot alter a
        // CHECK constraint in place. Rebuild atomically, preserving row order.
        let incoming_schema: String = connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='save_sync_incoming'",
                [],
                |r| r.get(0),
            )
            .map_err(sqlite)?;
        if incoming_schema.contains("CHECK (size > 0)") {
            let replacement = incoming_schema.replace("CHECK (size > 0)", "CHECK (size >= 0)");
            connection.execute_batch(&format!("BEGIN IMMEDIATE;
                DROP INDEX save_sync_incoming_state;
                ALTER TABLE save_sync_incoming RENAME TO save_sync_incoming_v1;
                {replacement};
                INSERT INTO save_sync_incoming SELECT * FROM save_sync_incoming_v1;
                DROP TABLE save_sync_incoming_v1;
                CREATE INDEX save_sync_incoming_state ON save_sync_incoming (server_id, account_id, installation_root, effective_saves_root, state);
                COMMIT;")).map_err(sqlite)?;
        }
        ensure_column(
            &connection,
            "save_sync_incoming",
            "replacement_backup",
            "TEXT",
        )?;

        let mut journal = Self {
            connection,
            scope,
            private_root,
        };
        // Relative paths use one persisted separator convention. Old Windows
        // journals used backslashes; slots arriving from RomM use forward
        // slashes. Normalize before recovery so the same file is not mistaken
        // for a proposed destination change. Absolute roots/backups are untouched.
        journal.connection.execute("UPDATE save_sync_slots SET relative_path=replace(relative_path,char(92),'/'),proposed_relative_path=replace(proposed_relative_path,char(92),'/') WHERE server_id=?1 AND account_id=?2 AND installation_root=?3 AND effective_saves_root=?4",rusqlite::params_from_iter(scope_only_params(&journal.scope))).map_err(sqlite)?;
        journal.recover_incoming_publications()?;
        journal.recover_interrupted_publications()?;
        Ok(journal)
    }

    pub(super) fn validate_key(&self, key: &RomKey) -> Result<()> {
        if key.server_id != self.scope.server_id {
            return Err(Error::Unsupported(
                "ROM key belongs to a different server scope".into(),
            ));
        }
        Ok(())
    }
}

fn ensure_column(
    connection: &rusqlite::Connection,
    table: &str,
    column: &str,
    declaration: &str,
) -> Result<()> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(sqlite)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(sqlite)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sqlite)?;
    if !columns.iter().any(|existing| existing == column) {
        connection
            .execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} {declaration}"
            ))
            .map_err(sqlite)?;
    }
    Ok(())
}
fn set_private_directory_mode(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn set_private_file_mode(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn sqlite(error: rusqlite::Error) -> Error {
    Error::Io(std::io::Error::other(error))
}

fn scope_only_params(scope: &SaveSyncScope) -> Vec<rusqlite::types::Value> {
    vec![
        scope.server_id.clone().into(),
        scope.account_id.clone().into(),
        windows_path_key(&scope.installation_root).into(),
        windows_path_key(&scope.effective_saves_root).into(),
    ]
}

fn scope_key_params(scope: &SaveSyncScope, key: &RomKey) -> Vec<rusqlite::types::Value> {
    vec![
        scope.server_id.clone().into(),
        scope.account_id.clone().into(),
        windows_path_key(&scope.installation_root).into(),
        windows_path_key(&scope.effective_saves_root).into(),
        key.server_id.clone().into(),
        key.rom_id.into(),
        key.file_id.into(),
    ]
}

fn path_string(path: &Path) -> Result<String> {
    if path.is_relative() {
        return super::portable_save_path(path);
    }
    path.to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| Error::Unsupported("save-sync path is not valid Unicode".into()))
}

pub(super) fn private_snapshot_path(root: &Path, name: &str) -> Result<PathBuf> {
    let path = private_artifact_path(root, name)?;
    if path
        .extension()
        .is_none_or(|extension| extension != "snapshot")
    {
        return Err(Error::Unsupported(
            "journal snapshot path is not a published snapshot".into(),
        ));
    }
    Ok(path)
}

pub(super) fn private_artifact_path(root: &Path, name: &str) -> Result<PathBuf> {
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
        || name.contains(['/', '\\'])
    {
        return Err(Error::Unsupported(
            "journal snapshot path is not a private artifact name".into(),
        ));
    }
    Ok(root.join(name))
}
