use super::super::mapping::{SaveMapping, SaveProfile, RETROBAT_GB_SRM_PROFILE};
use super::super::path::open_save_read;
use super::super::settings::windows_path_key;
use super::private_snapshot_path;
use super::snapshot::hash_reader;
use super::{
    path_string, scope_key_params, scope_only_params, sqlite, JournalSlot, LocalObservation,
    SaveSyncJournal, SaveSyncScope,
};
use crate::catalog::RomKey;
use crate::error::{Error, Result};
use crate::save_sync::mapping::{validate_mapping_path, MappingReport};
use rusqlite::{OptionalExtension, Transaction};
use std::path::{Path, PathBuf};

impl SaveSyncJournal {
    /// Persist current visible mappings. Confirmed paths are retained when a
    /// later catalogue proposes a different path; that change becomes an
    /// attention item instead of silently redirecting a save.
    pub fn reconcile_mappings(&mut self, report: &MappingReport) -> Result<()> {
        for mapping in &report.mappings {
            self.validate_key(&mapping.rom_key)?;
            if mapping.profile.id != RETROBAT_GB_SRM_PROFILE {
                return Err(Error::Unsupported("unrecognized save profile".into()));
            }
            validate_mapping_path(&mapping.relative_path)?;
        }
        for unmapped in &report.unmapped {
            self.validate_key(&unmapped.rom_key)?;
        }
        let transaction = self.connection.transaction().map_err(sqlite)?;
        transaction
            .execute(
                "UPDATE save_sync_slots SET candidate_available = 0
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4",
                rusqlite::params_from_iter(scope_only_params(&self.scope)),
            )
            .map_err(sqlite)?;
        for mapping in &report.mappings {
            let relative = path_string(&mapping.relative_path)?;
            transaction
                .execute(
                    "UPDATE save_sync_slots SET candidate_available = 1
                     WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                       AND effective_saves_root = ?4 AND rom_server_id = ?5
                       AND rom_id = ?6 AND file_id = ?7",
                    rusqlite::params![
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
            let existing = query_mapping(&transaction, &self.scope, &mapping.rom_key)?;
            match existing {
                None => insert_mapping(&transaction, &self.scope, mapping, &relative)?,
                Some((stored, confirmed, _)) if confirmed && stored != relative => {
                    transaction
                        .execute(
                            "UPDATE save_sync_slots SET proposed_relative_path = ?1,
                                needs_attention = 1
                             WHERE server_id = ?2 AND account_id = ?3 AND installation_root = ?4
                               AND effective_saves_root = ?5 AND rom_server_id = ?6
                               AND rom_id = ?7 AND file_id = ?8",
                            rusqlite::params![
                                relative,
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
                }
                Some((_, false, _)) => {
                    transaction
                        .execute(
                            "UPDATE save_sync_slots SET profile_id = ?1, relative_path = ?2,
                                proposed_relative_path = NULL, needs_attention = 0
                             WHERE server_id = ?3 AND account_id = ?4 AND installation_root = ?5
                               AND effective_saves_root = ?6 AND rom_server_id = ?7
                               AND rom_id = ?8 AND file_id = ?9",
                            rusqlite::params![
                                mapping.profile.id,
                                relative,
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
                }
                Some((_, true, _)) => {}
            }
        }
        for unmapped in &report.unmapped {
            transaction
                .execute(
                    "UPDATE save_sync_slots SET candidate_available = 0,
                        needs_attention = CASE WHEN mapping_confirmed = 1 THEN 1 ELSE needs_attention END
                     WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                       AND effective_saves_root = ?4 AND rom_server_id = ?5
                       AND rom_id = ?6 AND file_id = ?7",
                    rusqlite::params![
                        self.scope.server_id,
                        self.scope.account_id,
                        windows_path_key(&self.scope.installation_root),
                        windows_path_key(&self.scope.effective_saves_root),
                        unmapped.rom_key.server_id,
                        unmapped.rom_key.rom_id,
                        unmapped.rom_key.file_id,
                    ],
                )
                .map_err(sqlite)?;
        }
        transaction
            .execute(
                "UPDATE save_sync_slots SET needs_attention = 1
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4 AND mapping_confirmed = 1
                   AND candidate_available = 0",
                rusqlite::params_from_iter(scope_only_params(&self.scope)),
            )
            .map_err(sqlite)?;
        transaction.commit().map_err(sqlite)
    }

    /// Confirm only the path currently presented by a mapping plan. This
    /// avoids turning a stale UI preview into a new writable target.
    pub fn confirm_mapping(&mut self, key: &RomKey, relative_path: &Path) -> Result<()> {
        self.validate_key(key)?;
        validate_mapping_path(relative_path)?;
        let relative = path_string(relative_path)?;
        let transaction = self.connection.transaction().map_err(sqlite)?;
        let current = query_mapping(&transaction, &self.scope, key)?
            .ok_or_else(|| Error::Unsupported("save mapping is no longer available".into()))?;
        let proposed: Option<String> = transaction
            .query_row(
                "SELECT proposed_relative_path FROM save_sync_slots
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4 AND rom_server_id = ?5
                   AND rom_id = ?6 AND file_id = ?7",
                rusqlite::params_from_iter(scope_key_params(&self.scope, key)),
                |row| row.get(0),
            )
            .map_err(sqlite)?;
        if !current.2 {
            return Err(Error::Unsupported(
                "save mapping is currently ambiguous or unsupported".into(),
            ));
        }
        if current.0 != relative && proposed.as_deref() != Some(&relative) {
            return Err(Error::Unsupported(
                "save mapping changed since its preview; refresh before confirming".into(),
            ));
        }
        transaction
            .execute(
                "UPDATE save_sync_slots SET relative_path = ?1, proposed_relative_path = NULL,
                    mapping_confirmed = 1, needs_attention = 0
                 WHERE server_id = ?2 AND account_id = ?3 AND installation_root = ?4
                   AND effective_saves_root = ?5 AND rom_server_id = ?6
                   AND rom_id = ?7 AND file_id = ?8",
                rusqlite::params![
                    relative,
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

    pub fn slot(&self, key: &RomKey) -> Result<Option<JournalSlot>> {
        self.validate_key(key)?;
        self.connection
            .query_row(
                "SELECT rom_server_id, rom_id, file_id, profile_id, relative_path,
                    proposed_relative_path, mapping_confirmed, candidate_available, needs_attention,
                    current_local_hash, local_generation, local_ever_existed, local_removed,
                    remote_baseline_hash, remote_slot_id, snapshot_revision, snapshot_path,
                    snapshot_hash, snapshot_generation, remote_outcome, last_failure,
                    local_baseline_hash, remote_history_ids
                 FROM save_sync_slots
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4 AND rom_server_id = ?5
                   AND rom_id = ?6 AND file_id = ?7",
                rusqlite::params_from_iter(scope_key_params(&self.scope, key)),
                |row| read_journal_slot(row, &self.private_root),
            )
            .optional()
            .map_err(sqlite)
    }

    /// Return confirmed local files whose latest generation has no durable
    /// snapshot yet. The scheduler uses this at startup to recover a debounce
    /// that was interrupted before publication.
    pub fn dirty_mappings(&self) -> Result<Vec<SaveMapping>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT rom_server_id, rom_id, file_id FROM save_sync_slots
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                    AND effective_saves_root = ?4 AND mapping_confirmed = 1
                    AND candidate_available = 1 AND needs_attention = 0 AND local_removed = 0
                   AND current_local_hash IS NOT NULL
                 ORDER BY rom_id, file_id",
            )
            .map_err(sqlite)?;
        let rows = statement
            .query_map(
                rusqlite::params_from_iter(scope_only_params(&self.scope)),
                |row| {
                    Ok(RomKey {
                        server_id: row.get(0)?,
                        rom_id: row.get(1)?,
                        file_id: row.get(2)?,
                    })
                },
            )
            .map_err(sqlite)?;
        let keys = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sqlite)?;
        let mut dirty = Vec::new();
        for key in keys {
            let Some(slot) = self.slot(&key)? else {
                continue;
            };
            let saved_current: bool = self
                .connection
                .query_row(
                    "SELECT EXISTS (
                        SELECT 1 FROM save_sync_snapshots
                        WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                          AND effective_saves_root = ?4 AND revision = ?5
                          AND generation = ?6 AND content_hash = ?7
                          AND state IN ('ready', 'remote_in_flight', 'remote_ambiguous', 'remote_complete')
                    )",
                    rusqlite::params![
                        self.scope.server_id,
                        self.scope.account_id,
                        windows_path_key(&self.scope.installation_root),
                        windows_path_key(&self.scope.effective_saves_root),
                        slot.snapshot_revision,
                        i64::try_from(slot.local_generation).unwrap_or(i64::MAX),
                        slot.current_local_hash,
                    ],
                    |row| row.get(0),
                )
                .map_err(sqlite)?;
            if saved_current {
                continue;
            }
            if slot.remote_slot_id.is_some()
                && slot.current_local_hash == slot.local_baseline_hash
                && !slot.local_removed
            {
                continue;
            }
            let stem = slot
                .relative_path
                .file_stem()
                .and_then(|name| name.to_str())
                .ok_or_else(|| Error::Unsupported("journaled save name is invalid".into()))?;
            let visible_rom_name = format!("{stem}.gb");
            dirty.push(SaveMapping {
                rom_key: slot.rom_key,
                profile: SaveProfile::gameboy(),
                target_path: self.scope.effective_saves_root.join(&slot.relative_path),
                relative_path: slot.relative_path,
                visible_rom_name,
            });
        }
        Ok(dirty)
    }

    /// Hash a mapped local SRAM on every event, so same-size rewrites are
    /// detected and duplicate stat/read notifications do not reschedule work.
    pub fn observe_local_save(&mut self, mapping: &SaveMapping) -> Result<LocalObservation> {
        self.validate_confirmed_mapping(mapping)?;
        self.observe_local_save_inner(mapping, false)
    }

    /// Reconciliation must continue observing an already-attended mapping so
    /// a real external edit can resolve a conflict. This does not authorize
    /// capture or upload; those paths still require `needs_attention == false`.
    pub fn observe_local_save_for_reconciliation(
        &mut self,
        mapping: &SaveMapping,
    ) -> Result<LocalObservation> {
        self.validate_key(&mapping.rom_key)?;
        validate_mapping_path(&mapping.relative_path)?;
        let slot = self
            .slot(&mapping.rom_key)?
            .ok_or_else(|| Error::Unsupported("save mapping is not journaled".into()))?;
        if !slot.mapping_confirmed
            || !slot.candidate_available
            || slot.relative_path != mapping.relative_path
        {
            return Err(Error::Unsupported(
                "save mapping is no longer confirmed for reconciliation".into(),
            ));
        }
        self.observe_local_save_inner(mapping, true)
    }

    fn observe_local_save_inner(
        &mut self,
        mapping: &SaveMapping,
        allow_attention: bool,
    ) -> Result<LocalObservation> {
        let content_hash =
            match open_save_read(&self.scope.effective_saves_root, &mapping.relative_path) {
                Ok(mut file) => Some(hash_reader(&mut file)?),
                Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            };
        self.record_local_observation(mapping, content_hash, allow_attention)
    }

    /// Copy one observed generation into the private immutable spool. A stale
    /// generation is refused; the caller keeps the newer local generation
    /// dirty and can schedule it after its own debounce period.
    fn record_local_observation(
        &mut self,
        mapping: &SaveMapping,
        hash: Option<String>,
        allow_attention: bool,
    ) -> Result<LocalObservation> {
        let transaction = self.connection.transaction().map_err(sqlite)?;
        let current = query_local_state(&transaction, &self.scope, &mapping.rom_key)?
            .ok_or_else(|| Error::Unsupported("save mapping disappeared".into()))?;
        if !current.confirmed
            || !current.candidate_available
            || (current.needs_attention && !allow_attention)
            || current.relative_path != path_string(&mapping.relative_path)?
        {
            return Err(Error::Unsupported(
                "save mapping changed during local observation".into(),
            ));
        }
        let present = hash.is_some();
        if let Some(hash) = &hash {
            if current.hash.as_ref() == Some(hash) && !current.removed {
                transaction.commit().map_err(sqlite)?;
                return Ok(LocalObservation {
                    generation: current.generation as u64,
                    content_hash: Some(hash.clone()),
                    changed: false,
                    present: true,
                });
            }
        } else if !current.ever_existed || current.removed {
            transaction.commit().map_err(sqlite)?;
            return Ok(LocalObservation {
                generation: current.generation as u64,
                content_hash: None,
                changed: false,
                present: false,
            });
        }
        let generation = current
            .generation
            .checked_add(1)
            .ok_or_else(|| Error::Unsupported("save generation exhausted".into()))?;
        let removed = !present;
        transaction
            .execute(
                "UPDATE save_sync_slots SET current_local_hash = ?1, local_generation = ?2,
                    local_ever_existed = CASE WHEN ?3 THEN 1 ELSE local_ever_existed END,
                    local_removed = ?4,
                    needs_attention = CASE WHEN ?4 THEN 1 ELSE needs_attention END
                 WHERE server_id = ?5 AND account_id = ?6 AND installation_root = ?7
                   AND effective_saves_root = ?8 AND rom_server_id = ?9
                   AND rom_id = ?10 AND file_id = ?11",
                rusqlite::params![
                    hash,
                    generation,
                    present,
                    removed,
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
        transaction.commit().map_err(sqlite)?;
        Ok(LocalObservation {
            generation: generation as u64,
            content_hash: hash,
            changed: true,
            present,
        })
    }

    pub(super) fn validate_confirmed_mapping(&self, mapping: &SaveMapping) -> Result<()> {
        self.validate_key(&mapping.rom_key)?;
        validate_mapping_path(&mapping.relative_path)?;
        let slot = self
            .slot(&mapping.rom_key)?
            .ok_or_else(|| Error::Unsupported("save mapping is not journaled".into()))?;
        if !slot.mapping_confirmed || !slot.candidate_available || slot.needs_attention {
            return Err(Error::Unsupported(
                "save mapping needs explicit confirmation or attention".into(),
            ));
        }
        if slot.relative_path != mapping.relative_path {
            return Err(Error::Unsupported(
                "save mapping no longer matches its confirmed path".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct LocalState {
    pub(super) relative_path: String,
    pub(super) confirmed: bool,
    pub(super) candidate_available: bool,
    pub(super) needs_attention: bool,
    pub(super) generation: i64,
    pub(super) hash: Option<String>,
    pub(super) ever_existed: bool,
    pub(super) removed: bool,
}

pub(super) fn query_local_state(
    transaction: &Transaction<'_>,
    scope: &SaveSyncScope,
    key: &RomKey,
) -> Result<Option<LocalState>> {
    transaction
        .query_row(
            "SELECT relative_path, mapping_confirmed, candidate_available, needs_attention, local_generation,
                    current_local_hash, local_ever_existed, local_removed
             FROM save_sync_slots
             WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
               AND effective_saves_root = ?4 AND rom_server_id = ?5
               AND rom_id = ?6 AND file_id = ?7",
            rusqlite::params_from_iter(scope_key_params(scope, key)),
            |row| {
                Ok(LocalState {
                    relative_path: row.get(0)?,
                    confirmed: row.get(1)?,
                    candidate_available: row.get(2)?,
                    needs_attention: row.get(3)?,
                    generation: row.get(4)?,
                    hash: row.get(5)?,
                    ever_existed: row.get(6)?,
                    removed: row.get(7)?,
                })
            },
        )
        .optional()
        .map_err(sqlite)
}

fn query_mapping(
    transaction: &Transaction<'_>,
    scope: &SaveSyncScope,
    key: &RomKey,
) -> Result<Option<(String, bool, bool)>> {
    transaction
        .query_row(
            "SELECT relative_path, mapping_confirmed, candidate_available FROM save_sync_slots
             WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
               AND effective_saves_root = ?4 AND rom_server_id = ?5
               AND rom_id = ?6 AND file_id = ?7",
            rusqlite::params_from_iter(scope_key_params(scope, key)),
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(sqlite)
}

fn insert_mapping(
    transaction: &Transaction<'_>,
    scope: &SaveSyncScope,
    mapping: &SaveMapping,
    relative: &str,
) -> Result<()> {
    transaction
        .execute(
            "INSERT INTO save_sync_slots
                (server_id, account_id, installation_root, effective_saves_root,
                 rom_server_id, rom_id, file_id, profile_id, relative_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                scope.server_id,
                scope.account_id,
                windows_path_key(&scope.installation_root),
                windows_path_key(&scope.effective_saves_root),
                mapping.rom_key.server_id,
                mapping.rom_key.rom_id,
                mapping.rom_key.file_id,
                mapping.profile.id,
                relative,
            ],
        )
        .map_err(sqlite)?;
    Ok(())
}

fn read_journal_slot(
    row: &rusqlite::Row<'_>,
    private_root: &Path,
) -> rusqlite::Result<JournalSlot> {
    let snapshot_generation: Option<i64> = row.get(18)?;
    Ok(JournalSlot {
        rom_key: RomKey {
            server_id: row.get(0)?,
            rom_id: row.get(1)?,
            file_id: row.get(2)?,
        },
        profile_id: row.get(3)?,
        relative_path: PathBuf::from(row.get::<_, String>(4)?),
        proposed_relative_path: row.get::<_, Option<String>>(5)?.map(PathBuf::from),
        mapping_confirmed: row.get(6)?,
        candidate_available: row.get(7)?,
        needs_attention: row.get(8)?,
        current_local_hash: row.get(9)?,
        local_generation: row.get::<_, i64>(10)? as u64,
        local_ever_existed: row.get(11)?,
        local_removed: row.get(12)?,
        remote_baseline_hash: row.get(13)?,
        remote_slot_id: row.get(14)?,
        snapshot_revision: row.get(15)?,
        snapshot_path: row
            .get::<_, Option<String>>(16)?
            .map(|name| private_snapshot_path(private_root, &name))
            .transpose()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    16,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
        snapshot_hash: row.get(17)?,
        snapshot_generation: snapshot_generation.map(|generation| generation as u64),
        remote_outcome: row.get(19)?,
        last_failure: row.get(20)?,
        local_baseline_hash: row.get(21)?,
        remote_history_ids: row
            .get::<_, String>(22)?
            .split(',')
            .filter(|id| !id.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
    })
}
