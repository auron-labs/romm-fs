use super::super::mapping::SaveMapping;
use super::super::path::{ensure_no_rtc_companion, validate_relative_save_path};
use super::super::settings::windows_path_key;
use super::super::sha256_content_hash;
use super::{
    path_string, scope_key_params, scope_only_params, sqlite, IncomingSaveRecord, SaveSyncJournal,
};
use crate::catalog::RomKey;
use crate::error::{Error, Result};
use rusqlite::OptionalExtension;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

impl SaveSyncJournal {
    /// Keep an already verified remote body in the private spool before any
    /// decision that could expose it to the RetroBat saves directory.
    #[allow(clippy::too_many_arguments)]
    pub fn stage_incoming(
        &mut self,
        mapping: &SaveMapping,
        remote_id: &str,
        bytes: &[u8],
        reason: &str,
        baseline_local_hash: Option<&str>,
        baseline_remote_id: Option<&str>,
        baseline_remote_hash: Option<&str>,
    ) -> Result<IncomingSaveRecord> {
        self.validate_key(&mapping.rom_key)?;
        validate_relative_save_path(&mapping.relative_path).map_err(Error::Unsupported)?;
        if mapping.profile.id != super::super::mapping::RETROBAT_GB_SRM_PROFILE
            || bytes.is_empty()
            || bytes.len() as u64 > super::super::path::MAX_SAVE_BYTES
            || remote_id.parse::<i64>().is_err()
        {
            return Err(Error::Unsupported(
                "incoming save identity, profile, or size is invalid".into(),
            ));
        }
        ensure_no_rtc_companion(&self.scope.effective_saves_root, &mapping.relative_path)?;
        let content_hash = sha256_content_hash(bytes);
        let existing = self
            .connection
            .query_row(
                "SELECT incoming_id, staged_path, size, reason, state FROM save_sync_incoming
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4 AND rom_server_id = ?5 AND rom_id = ?6
                   AND file_id = ?7 AND remote_id = ?8 AND content_hash = ?9
                   AND state IN ('staged', 'publishing', 'interrupted')",
                rusqlite::params![
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    mapping.rom_key.server_id,
                    mapping.rom_key.rom_id,
                    mapping.rom_key.file_id,
                    remote_id,
                    content_hash,
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, u64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(sqlite)?;
        if let Some((id, path, size, reason, state)) = existing {
            let record = IncomingSaveRecord {
                id,
                rom_key: mapping.rom_key.clone(),
                remote_id: remote_id.to_owned(),
                content_hash,
                size,
                reason,
                path: private_incoming_path(&self.private_root, &path)?,
                state,
            };
            if incoming_path_has_hash(&record.path, &record.content_hash)? {
                return Ok(record);
            }
            return Err(Error::Unsupported(
                "durable incoming save stage is missing or failed SHA-256 verification".into(),
            ));
        }

        let id = Uuid::new_v4().to_string();
        let artifact_name = format!("{id}.incoming");
        let stage_path = self.private_root.join(&artifact_name);
        let mut stage = create_private_artifact(&stage_path)?;
        stage.write_all(bytes)?;
        stage.sync_all()?;
        sync_private_directory(&self.private_root)?;
        if !incoming_path_has_hash(&stage_path, &content_hash)? {
            return Err(Error::Unsupported(
                "staged incoming save failed SHA-256 verification".into(),
            ));
        }

        let transaction = self.connection.transaction().map_err(sqlite)?;
        transaction
            .execute(
                "INSERT INTO save_sync_incoming
                    (server_id, account_id, installation_root, effective_saves_root,
                     incoming_id, rom_server_id, rom_id, file_id, remote_id, content_hash,
                     size, reason, state, staged_path, baseline_local_hash,
                     baseline_remote_id, baseline_remote_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                         'staged', ?13, ?14, ?15, ?16)",
                rusqlite::params![
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    id,
                    mapping.rom_key.server_id,
                    mapping.rom_key.rom_id,
                    mapping.rom_key.file_id,
                    remote_id,
                    content_hash,
                    bytes.len() as u64,
                    reason,
                    artifact_name,
                    baseline_local_hash,
                    baseline_remote_id,
                    baseline_remote_hash,
                ],
            )
            .map_err(sqlite)?;
        transaction.commit().map_err(sqlite)?;
        Ok(IncomingSaveRecord {
            id,
            rom_key: mapping.rom_key.clone(),
            remote_id: remote_id.to_owned(),
            content_hash,
            size: bytes.len() as u64,
            reason: reason.to_owned(),
            path: stage_path,
            state: "staged".into(),
        })
    }

    pub fn incoming_saves(&self) -> Result<Vec<IncomingSaveRecord>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT incoming_id, rom_server_id, rom_id, file_id, remote_id, content_hash,
                        size, reason, staged_path, state
                 FROM save_sync_incoming
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4 AND state IN ('staged', 'publishing', 'interrupted')
                 ORDER BY rowid",
            )
            .map_err(sqlite)?;
        let rows = statement
            .query_map(
                rusqlite::params_from_iter(scope_only_params(&self.scope)),
                |row| {
                    let stage_name: String = row.get(8)?;
                    Ok((
                        IncomingSaveRecord {
                            id: row.get(0)?,
                            rom_key: RomKey {
                                server_id: row.get(1)?,
                                rom_id: row.get(2)?,
                                file_id: row.get(3)?,
                            },
                            remote_id: row.get(4)?,
                            content_hash: row.get(5)?,
                            size: row.get(6)?,
                            reason: row.get(7)?,
                            path: PathBuf::new(),
                            state: row.get(9)?,
                        },
                        stage_name,
                    ))
                },
            )
            .map_err(sqlite)?;
        rows.map(|row| {
            let (mut record, stage_name) = row.map_err(sqlite)?;
            record.path = private_incoming_path(&self.private_root, &stage_name)?;
            if !incoming_path_has_hash(&record.path, &record.content_hash)? {
                return Err(Error::Unsupported(format!(
                    "incoming save {} is incomplete or failed SHA-256 verification",
                    record.id
                )));
            }
            Ok(record)
        })
        .collect()
    }

    /// Persist publication intent before attempting an atomic no-replace
    /// create. Recovery treats an interrupted intent as a removal/attention
    /// state and never retries it as a never-existing target.
    pub fn begin_incoming_publication(
        &mut self,
        incoming_id: &str,
        mapping: &SaveMapping,
    ) -> Result<IncomingSaveRecord> {
        let record = self.incoming_by_id(incoming_id)?;
        if record.rom_key != mapping.rom_key || record.state != "staged" {
            return Err(Error::Unsupported(
                "incoming save is not publishable for this mapping".into(),
            ));
        }
        self.validate_key(&mapping.rom_key)?;
        validate_relative_save_path(&mapping.relative_path).map_err(Error::Unsupported)?;
        ensure_no_rtc_companion(&self.scope.effective_saves_root, &mapping.relative_path)?;
        let transaction = self.connection.transaction().map_err(sqlite)?;
        let mapped_path: Option<String> = transaction
            .query_row(
                "SELECT relative_path FROM save_sync_slots
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4 AND rom_server_id = ?5 AND rom_id = ?6
                   AND file_id = ?7 AND mapping_confirmed = 1 AND candidate_available = 1",
                rusqlite::params_from_iter(scope_key_params(&self.scope, &mapping.rom_key)),
                |row| row.get(0),
            )
            .optional()
            .map_err(sqlite)?;
        if mapped_path.as_deref() != Some(path_string(&mapping.relative_path)?.as_str()) {
            return Err(Error::Unsupported(
                "incoming save mapping changed before publication".into(),
            ));
        }
        transaction
            .execute(
                "UPDATE save_sync_incoming SET state = 'publishing'
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4 AND incoming_id = ?5 AND state = 'staged'",
                rusqlite::params![
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    incoming_id,
                ],
            )
            .map_err(sqlite)?;
        transaction
            .execute(
                "UPDATE save_sync_slots SET local_ever_existed = 1, local_removed = 1,
                    needs_attention = 1, last_failure = 'incoming publication interrupted'
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4 AND rom_server_id = ?5 AND rom_id = ?6
                   AND file_id = ?7",
                rusqlite::params_from_iter(scope_key_params(&self.scope, &mapping.rom_key)),
            )
            .map_err(sqlite)?;
        transaction.commit().map_err(sqlite)?;
        let mut updated = record;
        updated.state = "publishing".into();
        Ok(updated)
    }

    pub fn finish_incoming_publication(
        &mut self,
        incoming_id: &str,
        mapping: &SaveMapping,
        remote_history_ids: &[String],
    ) -> Result<()> {
        let record = self.incoming_by_id(incoming_id)?;
        if record.state != "publishing" || record.rom_key != mapping.rom_key {
            return Err(Error::Unsupported(
                "incoming publication intent is no longer active".into(),
            ));
        }
        let transaction = self.connection.transaction().map_err(sqlite)?;
        let generation: i64 = transaction
            .query_row(
                "SELECT local_generation FROM save_sync_slots
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4 AND rom_server_id = ?5 AND rom_id = ?6
                   AND file_id = ?7",
                rusqlite::params_from_iter(scope_key_params(&self.scope, &mapping.rom_key)),
                |row| row.get(0),
            )
            .map_err(sqlite)?;
        let generation = generation
            .checked_add(1)
            .ok_or_else(|| Error::Unsupported("save generation exhausted".into()))?;
        transaction
            .execute(
                "UPDATE save_sync_slots SET current_local_hash = ?1, local_baseline_hash = ?1,
                    local_generation = ?2, local_ever_existed = 1, local_removed = 0,
                 remote_baseline_hash = ?3, remote_slot_id = ?4,
                    remote_history_ids = ?5, needs_attention = 0,
                    remote_outcome = 'incoming_installed', last_failure = NULL
                 WHERE server_id = ?6 AND account_id = ?7 AND installation_root = ?8
                   AND effective_saves_root = ?9 AND rom_server_id = ?10 AND rom_id = ?11
                   AND file_id = ?12",
                rusqlite::params![
                    record.content_hash,
                    generation,
                    record.content_hash,
                    record.remote_id,
                    remote_history_ids.join(","),
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
        transaction
            .execute(
                "UPDATE save_sync_incoming SET state = 'installed'
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4 AND incoming_id = ?5",
                rusqlite::params![
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    incoming_id,
                ],
            )
            .map_err(sqlite)?;
        transaction.commit().map_err(sqlite)
    }

    pub fn cancel_incoming_publication(&mut self, incoming_id: &str, reason: &str) -> Result<()> {
        self.connection
            .execute(
                "UPDATE save_sync_incoming SET state = 'interrupted'
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4 AND incoming_id = ?5 AND state = 'publishing'",
                rusqlite::params![
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    incoming_id,
                ],
            )
            .map_err(sqlite)?;
        let record = self.incoming_by_id(incoming_id)?;
        self.mark_reconciliation_attention(&record.rom_key, reason)
    }

    pub fn record_reconciled_baseline(
        &mut self,
        key: &RomKey,
        local_hash: Option<&str>,
        remote_id: Option<&str>,
        remote_hash: Option<&str>,
        remote_history_ids: &[String],
    ) -> Result<()> {
        self.validate_key(key)?;
        self.connection
            .execute(
                "UPDATE save_sync_slots SET local_baseline_hash = ?1,
                    remote_slot_id = ?2, remote_baseline_hash = ?3, remote_history_ids = ?4,
                    needs_attention = 0, remote_outcome = 'reconciled', last_failure = NULL
                 WHERE server_id = ?5 AND account_id = ?6 AND installation_root = ?7
                   AND effective_saves_root = ?8 AND rom_server_id = ?9 AND rom_id = ?10
                   AND file_id = ?11",
                rusqlite::params![
                    local_hash,
                    remote_id,
                    remote_hash,
                    remote_history_ids.join(","),
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
        self.resolve_incoming_for_game(key)
    }

    pub fn mark_reconciliation_attention(&mut self, key: &RomKey, reason: &str) -> Result<()> {
        self.validate_key(key)?;
        self.connection
            .execute(
                "UPDATE save_sync_slots SET needs_attention = 1, last_failure = ?1,
                    remote_outcome = 'incoming_attention'
                 WHERE server_id = ?2 AND account_id = ?3 AND installation_root = ?4
                   AND effective_saves_root = ?5 AND rom_server_id = ?6 AND rom_id = ?7
                   AND file_id = ?8",
                rusqlite::params![
                    reason,
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

    fn resolve_incoming_for_game(&mut self, key: &RomKey) -> Result<()> {
        self.connection
            .execute(
                "UPDATE save_sync_incoming SET state = 'resolved'
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4 AND rom_server_id = ?5 AND rom_id = ?6
                   AND file_id = ?7 AND state IN ('staged', 'interrupted')",
                rusqlite::params_from_iter(scope_key_params(&self.scope, key)),
            )
            .map_err(sqlite)?;
        Ok(())
    }

    fn incoming_by_id(&self, incoming_id: &str) -> Result<IncomingSaveRecord> {
        let (mut record, path) = self
            .connection
            .query_row(
                "SELECT incoming_id, rom_server_id, rom_id, file_id, remote_id, content_hash,
                        size, reason, staged_path, state
                 FROM save_sync_incoming
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4 AND incoming_id = ?5",
                rusqlite::params![
                    self.scope.server_id,
                    self.scope.account_id,
                    windows_path_key(&self.scope.installation_root),
                    windows_path_key(&self.scope.effective_saves_root),
                    incoming_id,
                ],
                |row| {
                    let path: String = row.get(8)?;
                    Ok((
                        IncomingSaveRecord {
                            id: row.get(0)?,
                            rom_key: RomKey {
                                server_id: row.get(1)?,
                                rom_id: row.get(2)?,
                                file_id: row.get(3)?,
                            },
                            remote_id: row.get(4)?,
                            content_hash: row.get(5)?,
                            size: row.get(6)?,
                            reason: row.get(7)?,
                            path: PathBuf::new(),
                            state: row.get(9)?,
                        },
                        path,
                    ))
                },
            )
            .map_err(sqlite)?;
        record.path = private_incoming_path(&self.private_root, &path)?;
        if !incoming_path_has_hash(&record.path, &record.content_hash)? {
            return Err(Error::Unsupported(
                "incoming save is incomplete or failed SHA-256 verification".into(),
            ));
        }
        Ok(record)
    }

    pub(super) fn recover_incoming_publications(&mut self) -> Result<()> {
        let transaction = self.connection.transaction().map_err(sqlite)?;
        transaction
            .execute(
                "UPDATE save_sync_slots SET local_ever_existed = 1, local_removed = 1,
                    needs_attention = 1, last_failure = 'incoming publication was interrupted'
                 WHERE (server_id, account_id, installation_root, effective_saves_root,
                        rom_server_id, rom_id, file_id) IN (
                    SELECT server_id, account_id, installation_root, effective_saves_root,
                           rom_server_id, rom_id, file_id FROM save_sync_incoming
                    WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                      AND effective_saves_root = ?4 AND state = 'publishing')",
                rusqlite::params_from_iter(scope_only_params(&self.scope)),
            )
            .map_err(sqlite)?;
        transaction
            .execute(
                "UPDATE save_sync_incoming SET state = 'interrupted'
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4 AND state = 'publishing'",
                rusqlite::params_from_iter(scope_only_params(&self.scope)),
            )
            .map_err(sqlite)?;
        transaction.commit().map_err(sqlite)
    }
}

fn private_incoming_path(root: &Path, name: &str) -> Result<PathBuf> {
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
        || !name.ends_with(".incoming")
        || name.contains(['/', '\\', '\0'])
    {
        return Err(Error::Unsupported("incoming stage path is invalid".into()));
    }
    Ok(root.join(name))
}

fn create_private_artifact(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

fn incoming_path_has_hash(path: &Path, expected_hash: &str) -> Result<bool> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > super::super::path::MAX_SAVE_BYTES
    {
        return Ok(false);
    }
    let bytes = fs::read(path)?;
    Ok(sha256_content_hash(&bytes) == expected_hash)
}

fn sync_private_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
