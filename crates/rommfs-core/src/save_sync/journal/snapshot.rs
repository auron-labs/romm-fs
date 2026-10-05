use super::super::mapping::SaveMapping;
use super::super::path::{
    ensure_no_rtc_companion, is_reparse_point, open_save_read, MAX_SAVE_BYTES,
};
use super::super::settings::windows_path_key;
use super::mapping::query_local_state;
use super::{
    path_string, private_artifact_path, private_snapshot_path, scope_only_params, sqlite,
    SaveSyncJournal, SnapshotRecord, SnapshotState,
};
use crate::catalog::RomKey;
use crate::error::{Error, Result};
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

impl SaveSyncJournal {
    pub fn capture_snapshot(
        &mut self,
        mapping: &SaveMapping,
        generation: u64,
        expected_hash: &str,
    ) -> Result<SnapshotRecord> {
        self.validate_confirmed_mapping(mapping)?;
        let slot = self
            .slot(&mapping.rom_key)?
            .ok_or_else(|| Error::Unsupported("save mapping is no longer available".into()))?;
        if slot.local_generation != generation
            || slot.current_local_hash.as_deref() != Some(expected_hash)
            || slot.local_removed
        {
            return Err(Error::Unsupported(
                "save generation changed before snapshot capture".into(),
            ));
        }

        let revision = Uuid::new_v4().to_string();
        let temporary_path = self.private_root.join(format!("{revision}.tmp"));
        let snapshot_path = self.private_root.join(format!("{revision}.snapshot"));
        let actual_hash = match copy_save_snapshot(
            &self.scope.effective_saves_root,
            &mapping.relative_path,
            &temporary_path,
        ) {
            Ok(hash) => hash,
            Err(error) => {
                remove_if_present(&temporary_path)?;
                return Err(error);
            }
        };
        if actual_hash != expected_hash {
            remove_if_present(&temporary_path)?;
            return Err(Error::Unsupported(
                "save content changed during snapshot capture; capture remains dirty".into(),
            ));
        }
        if let Err(error) =
            ensure_no_rtc_companion(&self.scope.effective_saves_root, &mapping.relative_path)
        {
            remove_if_present(&temporary_path)?;
            return Err(error);
        }

        if let Err(error) = self.prepare_snapshot(
            mapping,
            generation,
            expected_hash,
            &revision,
            &temporary_path,
            &snapshot_path,
        ) {
            remove_if_present(&temporary_path)?;
            return Err(error);
        }
        self.publish_prepared_snapshot(&revision)?;
        self.snapshot(&revision)?
            .ok_or_else(|| Error::Unsupported("published snapshot journal row disappeared".into()))
    }

    pub fn snapshots(&self) -> Result<Vec<SnapshotRecord>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT revision, rom_server_id, rom_id, file_id, generation, content_hash,
                        state, snapshot_path, remote_slot_id, remote_outcome, failure
                 FROM save_sync_snapshots
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4
                 ORDER BY rowid",
            )
            .map_err(sqlite)?;
        let rows = statement
            .query_map(
                rusqlite::params_from_iter(scope_only_params(&self.scope)),
                |row| read_snapshot(row, &self.private_root),
            )
            .map_err(sqlite)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sqlite)
    }

    /// Only journaled `ready` artifacts are surfaced to the next transfer
    /// stage. Temporary files are never enumerated as upload candidates.
    pub fn ready_snapshots(&self) -> Result<Vec<SnapshotRecord>> {
        let mut ready = Vec::new();
        for snapshot in self.snapshots()? {
            if !matches!(
                snapshot.state,
                SnapshotState::Ready
                    | SnapshotState::RemoteInFlight
                    | SnapshotState::RemoteAmbiguous
            ) || snapshot
                .path
                .extension()
                .is_none_or(|extension| extension != "snapshot")
                || !path_has_hash(&snapshot.path, &snapshot.content_hash)?
            {
                continue;
            }
            let is_transferable: bool = self
                .connection
                .query_row(
                    "SELECT EXISTS (
                        SELECT 1 FROM save_sync_slots
                        WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                          AND effective_saves_root = ?4 AND rom_server_id = ?5
                          AND rom_id = ?6 AND file_id = ?7 AND mapping_confirmed = 1
                            AND candidate_available = 1 AND needs_attention = 0
                    )",
                    rusqlite::params![
                        self.scope.server_id,
                        self.scope.account_id,
                        windows_path_key(&self.scope.installation_root),
                        windows_path_key(&self.scope.effective_saves_root),
                        snapshot.rom_key.server_id,
                        snapshot.rom_key.rom_id,
                        snapshot.rom_key.file_id,
                    ],
                    |row| row.get(0),
                )
                .map_err(sqlite)?;
            if is_transferable {
                ready.push(snapshot);
            }
        }
        Ok(ready)
    }

    pub fn set_remote_baseline(
        &mut self,
        key: &RomKey,
        remote_slot_id: Option<&str>,
        baseline_hash: Option<&str>,
    ) -> Result<()> {
        self.validate_key(key)?;
        self.connection
            .execute(
                "UPDATE save_sync_slots SET remote_slot_id = ?1, remote_baseline_hash = ?2
                 WHERE server_id = ?3 AND account_id = ?4 AND installation_root = ?5
                   AND effective_saves_root = ?6 AND rom_server_id = ?7
                   AND rom_id = ?8 AND file_id = ?9",
                rusqlite::params![
                    remote_slot_id,
                    baseline_hash,
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    key.server_id,
                    key.rom_id,
                    key.file_id,
                ],
            )
            .map_err(sqlite)?;
        Ok(())
    }

    /// Record the outcome of a future remote handoff, including ambiguous
    /// server results. No network request is made by this method.
    pub fn record_remote_outcome(
        &mut self,
        revision: &str,
        state: SnapshotState,
        outcome: Option<&str>,
        failure: Option<&str>,
    ) -> Result<()> {
        if !matches!(
            state,
            SnapshotState::Ready
                | SnapshotState::RemoteInFlight
                | SnapshotState::RemoteAmbiguous
                | SnapshotState::RemoteComplete
                | SnapshotState::Failed
        ) {
            return Err(Error::Unsupported(
                "remote outcome requires a terminal or in-flight state".into(),
            ));
        }
        // Ambiguous is an unresolved HTTP outcome, not a permanent content
        // conflict. The retry path inventories by revision UUID before it can
        // safely decide whether to verify or repeat the POST.
        let needs_attention = matches!(state, SnapshotState::Failed);
        let transaction = self.connection.transaction().map_err(sqlite)?;
        let key: Option<RomKey> = transaction
            .query_row(
                "SELECT rom_server_id, rom_id, file_id FROM save_sync_snapshots
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4 AND revision = ?5",
                rusqlite::params![
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    revision,
                ],
                |row| {
                    Ok(RomKey {
                        server_id: row.get(0)?,
                        rom_id: row.get(1)?,
                        file_id: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(sqlite)?;
        let key =
            key.ok_or_else(|| Error::Unsupported("snapshot revision was not found".into()))?;
        transaction
            .execute(
                "UPDATE save_sync_snapshots SET state = ?1, remote_outcome = ?2, failure = ?3
                 WHERE server_id = ?4 AND account_id = ?5 AND installation_root = ?6
                   AND effective_saves_root = ?7 AND revision = ?8",
                rusqlite::params![
                    state.as_str(),
                    outcome,
                    failure,
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    revision,
                ],
            )
            .map_err(sqlite)?;
        transaction
            .execute(
                "UPDATE save_sync_slots SET remote_outcome = ?1, last_failure = ?2,
                    needs_attention = CASE WHEN ?3 THEN 1 ELSE needs_attention END
                 WHERE server_id = ?4 AND account_id = ?5 AND installation_root = ?6
                   AND effective_saves_root = ?7 AND rom_server_id = ?8
                   AND rom_id = ?9 AND file_id = ?10",
                rusqlite::params![
                    outcome,
                    failure,
                    needs_attention,
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    key.server_id,
                    key.rom_id,
                    key.file_id,
                ],
            )
            .map_err(sqlite)?;
        transaction.commit().map_err(sqlite)
    }

    /// Persist a verified API acknowledgement and advance the accepted remote
    /// baseline without clearing a newer local generation's dirty state.
    pub fn record_remote_complete(
        &mut self,
        revision: &str,
        remote_slot_id: &str,
        content_hash: &str,
    ) -> Result<()> {
        let transaction = self.connection.transaction().map_err(sqlite)?;
        let (key, generation): (RomKey, i64) = transaction
            .query_row(
                "SELECT rom_server_id, rom_id, file_id, generation FROM save_sync_snapshots
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                    AND effective_saves_root = ?4 AND revision = ?5 AND content_hash = ?6",
                rusqlite::params![
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    revision,
                    content_hash,
                ],
                |row| {
                    Ok((
                        RomKey {
                            server_id: row.get(0)?,
                            rom_id: row.get(1)?,
                            file_id: row.get(2)?,
                        },
                        row.get(3)?,
                    ))
                },
            )
            .map_err(sqlite)?;
        transaction
            .execute(
                "UPDATE save_sync_snapshots SET state = 'remote_complete', remote_slot_id = ?1,
                    remote_outcome = 'readback_verified', failure = NULL
                 WHERE server_id = ?2 AND account_id = ?3 AND installation_root = ?4
                   AND effective_saves_root = ?5 AND revision = ?6",
                rusqlite::params![
                    remote_slot_id,
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    revision,
                ],
            )
            .map_err(sqlite)?;
        transaction
            .execute(
                "UPDATE save_sync_slots SET remote_slot_id = ?1, remote_baseline_hash = ?2,
                    local_baseline_hash = CASE
                        WHEN local_generation = ?10 AND current_local_hash = ?2
                            AND local_removed = 0 AND needs_attention = 0 THEN ?2
                        ELSE local_baseline_hash
                    END,
                    remote_history_ids = CASE
                        WHEN remote_history_ids = '' THEN ?1
                        WHEN instr(',' || remote_history_ids || ',', ',' || ?1 || ',') > 0
                            THEN remote_history_ids
                        ELSE remote_history_ids || ',' || ?1
                    END,
                    remote_outcome = 'readback_verified',
                    last_failure = CASE
                        WHEN local_generation = ?10 AND current_local_hash = ?2
                            AND local_removed = 0 AND needs_attention = 0 THEN NULL
                        ELSE last_failure
                    END
                 WHERE server_id = ?3 AND account_id = ?4 AND installation_root = ?5
                   AND effective_saves_root = ?6 AND rom_server_id = ?7
                   AND rom_id = ?8 AND file_id = ?9",
                rusqlite::params![
                    remote_slot_id,
                    content_hash,
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    key.server_id,
                    key.rom_id,
                    key.file_id,
                    generation,
                ],
            )
            .map_err(sqlite)?;
        transaction.commit().map_err(sqlite)
    }

    /// A verified remote version can satisfy already-captured local snapshots
    /// with the same bytes. This is a no-op baseline adoption, not an upload.
    pub fn mark_matching_snapshots_satisfied(
        &mut self,
        key: &RomKey,
        content_hash: &str,
        remote_slot_id: &str,
    ) -> Result<()> {
        self.validate_key(key)?;
        self.connection
            .execute(
                "UPDATE save_sync_snapshots SET state = 'remote_complete', remote_slot_id = ?1,
                    remote_outcome = 'identical_bytes_verified', failure = NULL
                 WHERE server_id = ?2 AND account_id = ?3 AND installation_root = ?4
                   AND effective_saves_root = ?5 AND rom_server_id = ?6 AND rom_id = ?7
                   AND file_id = ?8 AND content_hash = ?9
                   AND state IN ('ready', 'remote_in_flight', 'remote_ambiguous')",
                rusqlite::params![
                    remote_slot_id,
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    key.server_id,
                    key.rom_id,
                    key.file_id,
                    content_hash,
                ],
            )
            .map_err(sqlite)?;
        Ok(())
    }

    pub fn mark_remote_attention(&mut self, revision: &str, reason: &str) -> Result<()> {
        self.record_remote_outcome(
            revision,
            SnapshotState::Failed,
            Some("attention"),
            Some(reason),
        )
    }

    pub(super) fn prepare_snapshot(
        &mut self,
        mapping: &SaveMapping,
        generation: u64,
        content_hash: &str,
        revision: &str,
        temporary_path: &Path,
        snapshot_path: &Path,
    ) -> Result<()> {
        let generation = i64::try_from(generation)
            .map_err(|_| Error::Unsupported("save generation exceeds journal range".into()))?;
        let transaction = self.connection.transaction().map_err(sqlite)?;
        let slot = query_local_state(&transaction, &self.scope, &mapping.rom_key)?
            .ok_or_else(|| Error::Unsupported("save mapping is no longer available".into()))?;
        if !slot.confirmed
            || !slot.candidate_available
            || slot.needs_attention
            || slot.relative_path != path_string(&mapping.relative_path)?
            || slot.generation != generation
            || slot.hash.as_deref() != Some(content_hash)
            || slot.removed
        {
            return Err(Error::Unsupported(
                "save mapping or generation changed before snapshot publication".into(),
            ));
        }
        transaction
            .execute(
                "INSERT INTO save_sync_snapshots
                    (server_id, account_id, installation_root, effective_saves_root,
                     revision, rom_server_id, rom_id, file_id, generation, content_hash,
                     state, temporary_path, snapshot_path, remote_slot_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'preparing', ?11, ?12,
                    (SELECT remote_slot_id FROM save_sync_slots WHERE server_id = ?1
                     AND account_id = ?2 AND installation_root = ?3 AND effective_saves_root = ?4
                     AND rom_server_id = ?6 AND rom_id = ?7 AND file_id = ?8))",
                rusqlite::params![
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    revision,
                    mapping.rom_key.server_id,
                    mapping.rom_key.rom_id,
                    mapping.rom_key.file_id,
                    generation,
                    content_hash,
                    artifact_name(temporary_path, revision, "tmp")?,
                    artifact_name(snapshot_path, revision, "snapshot")?,
                ],
            )
            .map_err(sqlite)?;
        transaction
            .execute(
                "UPDATE save_sync_slots SET snapshot_revision = ?1, snapshot_path = ?2,
                    snapshot_hash = ?3, snapshot_generation = ?4
                 WHERE server_id = ?5 AND account_id = ?6 AND installation_root = ?7
                   AND effective_saves_root = ?8 AND rom_server_id = ?9
                   AND rom_id = ?10 AND file_id = ?11",
                rusqlite::params![
                    revision,
                    artifact_name(snapshot_path, revision, "snapshot")?,
                    content_hash,
                    generation,
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    mapping.rom_key.server_id,
                    mapping.rom_key.rom_id,
                    mapping.rom_key.file_id,
                ],
            )
            .map_err(sqlite)?;
        transaction.commit().map_err(sqlite)
    }

    fn publish_prepared_snapshot(&mut self, revision: &str) -> Result<()> {
        let (temporary, destination, expected_hash) = self.prepared_paths(revision)?;
        if destination.exists() {
            return Err(Error::Unsupported(
                "snapshot revision already has a published artifact".into(),
            ));
        }
        fs::rename(&temporary, &destination)?;
        make_snapshot_immutable(&destination)?;
        sync_private_directory(&self.private_root)?;
        if !path_has_hash(&destination, &expected_hash)? {
            self.mark_snapshot_failed(revision, "published snapshot failed SHA-256 verification")?;
            return Err(Error::Unsupported(
                "published snapshot failed SHA-256 verification".into(),
            ));
        }
        self.mark_snapshot_ready(revision, &expected_hash)
    }

    pub(super) fn recover_interrupted_publications(&mut self) -> Result<()> {
        let prepared: Vec<(String, String, String, String)> = {
            let mut statement = self
                .connection
                .prepare(
                    "SELECT revision, temporary_path, snapshot_path, content_hash
                     FROM save_sync_snapshots
                     WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                       AND effective_saves_root = ?4 AND state = 'preparing'",
                )
                .map_err(sqlite)?;
            let rows = statement
                .query_map(
                    rusqlite::params_from_iter(scope_only_params(&self.scope)),
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .map_err(sqlite)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(sqlite)?
        };
        let mut referenced_temps: HashSet<PathBuf> = HashSet::new();
        for (revision, temporary_name, destination_name, expected_hash) in prepared {
            let temporary = private_artifact_path(&self.private_root, &temporary_name)?;
            let destination = private_artifact_path(&self.private_root, &destination_name)?;
            if temporary.file_name().and_then(|name| name.to_str())
                != Some(&format!("{revision}.tmp"))
                || destination.file_name().and_then(|name| name.to_str())
                    != Some(&format!("{revision}.snapshot"))
            {
                self.mark_snapshot_failed(&revision, "prepared snapshot path is invalid")?;
                continue;
            }
            referenced_temps.insert(temporary.clone());
            if path_has_hash(&destination, &expected_hash)? {
                remove_if_present(&temporary)?;
                make_snapshot_immutable(&destination)?;
                self.mark_snapshot_ready(&revision, &expected_hash)?;
            } else if path_has_hash(&temporary, &expected_hash)? {
                fs::rename(&temporary, &destination)?;
                make_snapshot_immutable(&destination)?;
                sync_private_directory(&self.private_root)?;
                self.mark_snapshot_ready(&revision, &expected_hash)?;
            } else {
                self.mark_snapshot_failed(
                    &revision,
                    "prepared snapshot is missing or failed SHA-256 verification",
                )?;
            }
        }
        self.verify_ready_artifacts()?;
        // A crash before the preparation transaction can leave a scratch file
        // with no journal row. It is never eligible for transfer; remove only
        // such unreferenced `.tmp` artifacts from this private spool.
        for entry in fs::read_dir(&self.private_root)? {
            let entry = entry?;
            if entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "tmp")
                && !referenced_temps.contains(&entry.path())
            {
                remove_if_present(&entry.path())?;
            }
        }
        Ok(())
    }

    fn verify_ready_artifacts(&mut self) -> Result<()> {
        let ready: Vec<(String, String, String)> = {
            let mut statement = self
                .connection
                .prepare(
                    "SELECT revision, snapshot_path, content_hash FROM save_sync_snapshots
                     WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                       AND effective_saves_root = ?4 AND state = 'ready'",
                )
                .map_err(sqlite)?;
            let rows = statement
                .query_map(
                    rusqlite::params_from_iter(scope_only_params(&self.scope)),
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(sqlite)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(sqlite)?
        };
        for (revision, path_name, expected_hash) in ready {
            let path = match private_snapshot_path(&self.private_root, &path_name) {
                Ok(path) => path,
                Err(_) => {
                    self.mark_snapshot_failed(&revision, "ready snapshot path is invalid")?;
                    continue;
                }
            };
            if !path_has_hash(&path, &expected_hash)? {
                self.mark_snapshot_failed(
                    &revision,
                    "ready snapshot is missing or failed SHA-256 verification",
                )?;
            }
        }
        Ok(())
    }

    fn prepared_paths(&self, revision: &str) -> Result<(PathBuf, PathBuf, String)> {
        let (temporary_name, snapshot_name, content_hash) = self
            .connection
            .query_row(
                "SELECT temporary_path, snapshot_path, content_hash
                 FROM save_sync_snapshots
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4 AND revision = ?5 AND state = 'preparing'",
                rusqlite::params![
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    revision,
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get(2)?,
                    ))
                },
            )
            .map_err(sqlite)?;
        Ok((
            private_artifact_path(&self.private_root, &temporary_name)?,
            private_artifact_path(&self.private_root, &snapshot_name)?,
            content_hash,
        ))
    }

    fn mark_snapshot_ready(&mut self, revision: &str, expected_hash: &str) -> Result<()> {
        let transaction = self.connection.transaction().map_err(sqlite)?;
        let published_hash: Option<String> = transaction
            .query_row(
                "SELECT content_hash FROM save_sync_snapshots
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4 AND revision = ?5 AND state = 'preparing'",
                rusqlite::params![
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    revision,
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(sqlite)?;
        if published_hash.as_deref() != Some(expected_hash) {
            return Err(Error::Unsupported(
                "snapshot journal hash changed before publication".into(),
            ));
        }
        transaction
            .execute(
                "UPDATE save_sync_snapshots SET state = 'ready', temporary_path = NULL
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4 AND revision = ?5",
                rusqlite::params![
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    revision,
                ],
            )
            .map_err(sqlite)?;
        transaction.commit().map_err(sqlite)
    }

    fn mark_snapshot_failed(&mut self, revision: &str, failure: &str) -> Result<()> {
        let transaction = self.connection.transaction().map_err(sqlite)?;
        let key: Option<RomKey> = transaction
            .query_row(
                "SELECT rom_server_id, rom_id, file_id FROM save_sync_snapshots
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4 AND revision = ?5",
                rusqlite::params![
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    revision,
                ],
                |row| {
                    Ok(RomKey {
                        server_id: row.get(0)?,
                        rom_id: row.get(1)?,
                        file_id: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(sqlite)?;
        transaction
            .execute(
                "UPDATE save_sync_snapshots SET state = 'failed', failure = ?1
                 WHERE server_id = ?2 AND account_id = ?3 AND installation_root = ?4
                   AND effective_saves_root = ?5 AND revision = ?6",
                rusqlite::params![
                    failure,
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    revision,
                ],
            )
            .map_err(sqlite)?;
        if let Some(key) = key {
            transaction
                .execute(
                    "UPDATE save_sync_slots SET needs_attention = 1, last_failure = ?1
                     WHERE server_id = ?2 AND account_id = ?3 AND installation_root = ?4
                       AND effective_saves_root = ?5 AND rom_server_id = ?6
                       AND rom_id = ?7 AND file_id = ?8",
                    rusqlite::params![
                        failure,
                        self.scope.server_id,
                        self.scope.account_id,
                        windows_path_key(&self.scope.installation_root),
                        windows_path_key(&self.scope.effective_saves_root),
                        key.server_id,
                        key.rom_id,
                        key.file_id,
                    ],
                )
                .map_err(sqlite)?;
        }
        transaction.commit().map_err(sqlite)
    }

    fn snapshot(&self, revision: &str) -> Result<Option<SnapshotRecord>> {
        self.connection
            .query_row(
                "SELECT revision, rom_server_id, rom_id, file_id, generation, content_hash,
                        state, snapshot_path, remote_slot_id, remote_outcome, failure
                 FROM save_sync_snapshots
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4 AND revision = ?5",
                rusqlite::params![
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    revision,
                ],
                |row| read_snapshot(row, &self.private_root),
            )
            .optional()
            .map_err(sqlite)
    }
}

fn read_snapshot(row: &rusqlite::Row<'_>, private_root: &Path) -> rusqlite::Result<SnapshotRecord> {
    let state_text: String = row.get(6)?;
    Ok(SnapshotRecord {
        revision: row.get(0)?,
        rom_key: RomKey {
            server_id: row.get(1)?,
            rom_id: row.get(2)?,
            file_id: row.get(3)?,
        },
        generation: row.get::<_, i64>(4)? as u64,
        content_hash: row.get(5)?,
        // This schema is created by this crate. An unknown state is treated as
        // a row conversion error rather than presented as an upload candidate.
        state: SnapshotState::parse(&state_text).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                6,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        path: private_snapshot_path(private_root, &row.get::<_, String>(7)?).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                7,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        remote_slot_id: row.get(8)?,
        remote_outcome: row.get(9)?,
        failure: row.get(10)?,
    })
}

fn artifact_name(path: &Path, revision: &str, extension: &str) -> Result<String> {
    let expected = format!("{revision}.{extension}");
    if path.file_name().and_then(|name| name.to_str()) != Some(expected.as_str()) {
        return Err(Error::Unsupported(
            "invalid private snapshot artifact name".into(),
        ));
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
        .ok_or_else(|| Error::Unsupported("snapshot artifact name is not valid Unicode".into()))
}

pub(super) fn hash_reader(reader: &mut impl Read) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > MAX_SAVE_BYTES {
            return Err(Error::Unsupported(format!(
                "Game Boy SRAM exceeds the {MAX_SAVE_BYTES}-byte capture limit"
            )));
        }
        hasher.update(&buffer[..read]);
    }
    if total == 0 {
        return Err(Error::Unsupported("Game Boy SRAM is empty".into()));
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

pub(super) fn copy_save_snapshot(
    root: &Path,
    relative_path: &Path,
    temporary: &Path,
) -> Result<String> {
    let mut source = open_save_read(root, relative_path)?;
    let mut destination = private_create_new(temporary)?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > MAX_SAVE_BYTES {
            return Err(Error::Unsupported(format!(
                "Game Boy SRAM exceeds the {MAX_SAVE_BYTES}-byte capture limit"
            )));
        }
        destination.write_all(&buffer[..read])?;
        hasher.update(&buffer[..read]);
    }
    if total == 0 {
        return Err(Error::Unsupported("Game Boy SRAM is empty".into()));
    }
    destination.sync_all()?;
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn private_create_new(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

fn path_has_hash(path: &Path, expected_hash: &str) -> Result<bool> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file()
        || is_reparse_point(&metadata)
        || metadata.len() == 0
        || metadata.len() > MAX_SAVE_BYTES
    {
        return Ok(false);
    }
    let mut file = match open_private_snapshot(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let opened_metadata = file.metadata()?;
    if !opened_metadata.is_file() || is_reparse_point(&opened_metadata) {
        return Ok(false);
    }
    Ok(hash_reader(&mut file)? == expected_hash)
}

fn open_private_snapshot(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0x0000_0001).custom_flags(0x0020_0000);
    }
    options.open(path)
}

fn make_snapshot_immutable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o400))?;
    }
    #[cfg(windows)]
    {
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_readonly(true);
        fs::set_permissions(path, permissions)?;
    }
    #[cfg(not(any(unix, windows)))]
    let _ = path;
    Ok(())
}

fn remove_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn sync_private_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
