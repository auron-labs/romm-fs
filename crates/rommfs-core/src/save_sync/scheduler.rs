use super::journal::{LocalObservation, SaveSyncJournal, SnapshotRecord};
use super::mapping::SaveMapping;
use crate::catalog::RomKey;
use crate::error::Result;
use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObserveResult {
    Unchanged(LocalObservation),
    Scheduled {
        generation: u64,
        content_hash: String,
        due_at: Instant,
    },
    Missing(LocalObservation),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DueSnapshot {
    pub mapping: SaveMapping,
    pub generation: u64,
    pub content_hash: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RestoreReport {
    pub queued: usize,
    pub failures: Vec<(RomKey, String)>,
}

#[derive(Clone, Debug)]
struct PendingSnapshot {
    mapping: SaveMapping,
    generation: Option<u64>,
    content_hash: Option<String>,
    due_at: Instant,
    missing_since: bool,
}

const REMOVAL_SETTLE: Duration = Duration::from_secs(2);

/// Trailing-edge debounce scheduler for already-confirmed save mappings.
/// Callers supply monotonic time, so this state machine is deterministic in
/// tests and doesn't depend on wall-clock adjustments.
pub struct SaveSyncScheduler {
    debounce: Duration,
    pending: HashMap<RomKey, PendingSnapshot>,
}

impl SaveSyncScheduler {
    pub fn new(debounce: Duration) -> Self {
        Self {
            debounce,
            pending: HashMap::new(),
        }
    }

    pub fn with_seconds(seconds: u32) -> Self {
        Self::new(Duration::from_secs(seconds as u64))
    }

    /// Hash the mapped file and update its durable local generation. A
    /// duplicate event with unchanged contents does not move the deadline.
    pub fn observe(
        &mut self,
        journal: &mut SaveSyncJournal,
        mapping: &SaveMapping,
        now: Instant,
    ) -> Result<ObserveResult> {
        let observation = journal.observe_local_save(mapping)?;
        if !observation.changed {
            return Ok(ObserveResult::Unchanged(observation));
        }
        let Some(content_hash) = observation.content_hash.clone() else {
            // Local deletion is journaled for attention, never treated as an
            // uploadable empty save or a request to remove a remote baseline.
            self.pending.remove(&mapping.rom_key);
            return Ok(ObserveResult::Missing(observation));
        };
        let due_at = now + self.debounce;
        self.pending.insert(
            mapping.rom_key.clone(),
            PendingSnapshot {
                mapping: mapping.clone(),
                generation: Some(observation.generation),
                content_hash: Some(content_hash.clone()),
                due_at,
                missing_since: false,
            },
        );
        Ok(ObserveResult::Scheduled {
            generation: observation.generation,
            content_hash,
            due_at,
        })
    }

    /// Treat a filesystem notification as a hint only. Hashing and journal
    /// work happen after the trailing-edge deadline on the scheduler thread.
    pub fn hint(&mut self, mapping: &SaveMapping, now: Instant) {
        let generation = self
            .pending
            .get(&mapping.rom_key)
            .and_then(|pending| pending.generation);
        let content_hash = self
            .pending
            .get(&mapping.rom_key)
            .and_then(|pending| pending.content_hash.clone());
        self.pending.insert(
            mapping.rom_key.clone(),
            PendingSnapshot {
                mapping: mapping.clone(),
                generation,
                content_hash,
                due_at: now + self.debounce,
                missing_since: false,
            },
        );
    }

    /// Drop transient work after reconciliation has determined that this
    /// game's durable mapping is in attention. A later healthy baseline can
    /// re-arm the latest journaled generation with `restore_dirty`.
    pub fn cancel(&mut self, key: &RomKey) {
        self.pending.remove(key);
    }

    /// Re-arm durable local generations after a restart. A file observed
    /// before the prior process exited but not yet published remains dirty in
    /// SQLite and receives a fresh trailing-edge debounce.
    pub fn restore_dirty(
        &mut self,
        journal: &mut SaveSyncJournal,
        now: Instant,
    ) -> Result<RestoreReport> {
        let mappings = journal.dirty_mappings()?;
        self.restore_dirty_mappings(journal, mappings, now)
    }

    /// Re-arm only the reconciled save after an attention state clears.
    pub fn restore_dirty_for(
        &mut self,
        journal: &mut SaveSyncJournal,
        key: &RomKey,
        now: Instant,
    ) -> Result<RestoreReport> {
        let mappings = journal
            .dirty_mappings()?
            .into_iter()
            .filter(|mapping| &mapping.rom_key == key)
            .collect();
        self.restore_dirty_mappings(journal, mappings, now)
    }

    fn restore_dirty_mappings(
        &mut self,
        journal: &mut SaveSyncJournal,
        mappings: Vec<SaveMapping>,
        now: Instant,
    ) -> Result<RestoreReport> {
        let mut report = RestoreReport::default();
        for mapping in mappings {
            let observation = match journal.observe_local_save(&mapping) {
                Ok(observation) => observation,
                Err(error) => {
                    report.failures.push((mapping.rom_key, error.to_string()));
                    continue;
                }
            };
            if !observation.present {
                continue;
            }
            let Some(content_hash) = observation.content_hash else {
                continue;
            };
            if self.pending.get(&mapping.rom_key).is_some_and(|pending| {
                pending.generation == Some(observation.generation)
                    && pending.content_hash.as_deref() == Some(content_hash.as_str())
            }) {
                continue;
            }
            self.pending.insert(
                mapping.rom_key.clone(),
                PendingSnapshot {
                    mapping,
                    generation: Some(observation.generation),
                    content_hash: Some(content_hash),
                    due_at: now + self.debounce,
                    missing_since: false,
                },
            );
            report.queued += 1;
        }
        Ok(report)
    }

    /// Return and remove all deadlines reached at `now`, ordered by stable ROM
    /// identity. Each mapped save has an independent trailing-edge deadline.
    pub fn take_due(&mut self, now: Instant) -> Vec<DueSnapshot> {
        let mut due_keys: Vec<RomKey> = self
            .pending
            .iter()
            .filter(|(_, pending)| pending.due_at <= now && pending.generation.is_some())
            .map(|(key, _)| key.clone())
            .collect();
        due_keys.sort_by(|a, b| {
            a.server_id
                .cmp(&b.server_id)
                .then_with(|| a.rom_id.cmp(&b.rom_id))
                .then_with(|| a.file_id.cmp(&b.file_id))
        });
        due_keys
            .into_iter()
            .filter_map(|key| self.pending.remove(&key))
            .filter_map(|pending| {
                Some((pending.generation?, pending.content_hash?, pending.mapping))
            })
            .map(|(generation, content_hash, mapping)| DueSnapshot {
                mapping,
                generation,
                content_hash,
            })
            .collect()
    }

    /// Capture each due generation into the durable journal. Failures are
    /// returned per save so one locked/invalid file cannot block another.
    pub fn capture_due(
        &mut self,
        journal: &mut SaveSyncJournal,
        now: Instant,
    ) -> Vec<(RomKey, Result<SnapshotRecord>)> {
        let mut due_keys: Vec<RomKey> = self
            .pending
            .iter()
            .filter(|(_, pending)| pending.due_at <= now)
            .map(|(key, _)| key.clone())
            .collect();
        due_keys.sort_by(|a, b| {
            a.server_id
                .cmp(&b.server_id)
                .then_with(|| a.rom_id.cmp(&b.rom_id))
                .then_with(|| a.file_id.cmp(&b.file_id))
        });
        let mut results = Vec::new();
        for key in due_keys {
            let result_key = key.clone();
            let Some(mut pending) = self.pending.remove(&key) else {
                continue;
            };
            let missing = match is_missing(&pending.mapping.target_path) {
                Ok(missing) => missing,
                Err(error) => {
                    self.defer_hint(pending.mapping, now);
                    results.push((result_key, Err(error)));
                    continue;
                }
            };
            if missing {
                if !pending.missing_since {
                    pending.missing_since = true;
                    pending.due_at = now + REMOVAL_SETTLE;
                    self.pending.insert(key, pending);
                    continue;
                }
                match journal.observe_local_save(&pending.mapping) {
                    Ok(observation) if observation.present => {
                        self.pending.insert(
                            key,
                            PendingSnapshot {
                                mapping: pending.mapping,
                                generation: observation
                                    .content_hash
                                    .as_ref()
                                    .map(|_| observation.generation),
                                content_hash: observation.content_hash,
                                due_at: now + self.debounce,
                                missing_since: false,
                            },
                        );
                        continue;
                    }
                    Ok(_) => continue,
                    Err(error) => {
                        self.defer_hint(pending.mapping, now);
                        results.push((result_key, Err(error)));
                        continue;
                    }
                }
            }
            if pending.missing_since {
                pending.generation = None;
                pending.content_hash = None;
                pending.missing_since = false;
                pending.due_at = now + self.debounce;
                self.pending.insert(key, pending);
                continue;
            }

            let observed = match (pending.generation, pending.content_hash) {
                (Some(generation), Some(hash)) => Ok(Some((generation, hash))),
                _ => journal
                    .observe_local_save(&pending.mapping)
                    .map(
                        |observation| match (observation.present, observation.content_hash) {
                            (true, Some(hash)) if observation.changed => {
                                Some((observation.generation, hash))
                            }
                            _ => None,
                        },
                    ),
            };
            let Some((generation, content_hash)) = (match observed {
                Ok(observed) => observed,
                Err(error) => {
                    self.defer_hint(pending.mapping, now);
                    results.push((result_key, Err(error)));
                    continue;
                }
            }) else {
                continue;
            };
            match journal.capture_snapshot(&pending.mapping, generation, &content_hash) {
                Ok(snapshot) => results.push((result_key, Ok(snapshot))),
                Err(error) => {
                    // Even an unchanged generation is re-armed after capture
                    // failure; a sharing violation must not strand it dirty.
                    match journal.observe_local_save(&pending.mapping) {
                        Ok(observation) if observation.present => {
                            self.pending.insert(
                                key,
                                PendingSnapshot {
                                    mapping: pending.mapping,
                                    generation: observation
                                        .content_hash
                                        .as_ref()
                                        .map(|_| observation.generation),
                                    content_hash: observation.content_hash,
                                    due_at: now + self.debounce,
                                    missing_since: false,
                                },
                            );
                        }
                        _ => self.defer_hint(pending.mapping, now),
                    }
                    results.push((result_key, Err(error)));
                }
            }
        }
        results
    }

    fn defer_hint(&mut self, mapping: SaveMapping, now: Instant) {
        self.pending.insert(
            mapping.rom_key.clone(),
            PendingSnapshot {
                mapping,
                generation: None,
                content_hash: None,
                due_at: now + self.debounce,
                missing_since: false,
            },
        );
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

fn is_missing(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save_sync::journal::SaveSyncJournal;
    use crate::save_sync::mapping::{map_catalogue, SaveMapping};
    use crate::save_sync::settings::SaveSyncScope;
    use crate::{romm::*, save_sync::MappingReport};
    use std::fs;
    use std::path::Path;

    fn new_save_mapping(server: &str, rom_id: i64, file_id: i64, name: &str) -> SaveMapping {
        let platform = PlatformDto {
            id: 1,
            slug: "gb".into(),
            fs_slug: "gb".into(),
            name: "Game Boy".into(),
            custom_name: None,
            rom_count: 2,
        };
        let rom = RomDto {
            id: rom_id,
            platform_fs_slug: "gb".into(),
            platform_slug: "gb".into(),
            fs_name: name.into(),
            fs_size_bytes: 64,
            has_simple_single_file: true,
            has_nested_single_file: false,
            has_multiple_files: false,
            missing_from_fs: false,
            is_physical: false,
            updated_at: String::new(),
            files: vec![RomFileDto {
                id: file_id,
                file_name: name.into(),
                file_size_bytes: 64,
                last_modified: None,
                crc_hash: None,
                md5_hash: None,
                sha1_hash: None,
                is_top_level: true,
            }],
        };
        let catalogue =
            crate::catalog::build_catalogue(server, &[platform], &[rom], |_| {}).unwrap();
        map_catalogue(&catalogue, Path::new("saves"))
            .unwrap()
            .mappings
            .remove(0)
    }

    fn setup(dir: &Path) -> (SaveSyncJournal, Vec<SaveMapping>) {
        let saves = dir.join("retrobat/saves");
        let mut first = new_save_mapping("https://example", 1, 101, "One.gb");
        let mut second = new_save_mapping("https://example", 2, 102, "Two.gb");
        first.target_path = saves.join(&first.relative_path);
        second.target_path = saves.join(&second.relative_path);
        for mapping in [&first, &second] {
            let target = saves.join(&mapping.relative_path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, b"same-size-content").unwrap();
        }
        let scope = SaveSyncScope {
            server_id: "https://example".into(),
            account_id: "account".into(),
            installation_root: dir.join("retrobat"),
            effective_saves_root: saves,
        };
        let mut journal = SaveSyncJournal::open(
            dir.join("settings/save-sync.db"),
            dir.join("private"),
            scope,
        )
        .unwrap();
        journal
            .reconcile_mappings(&MappingReport {
                mappings: vec![first.clone(), second.clone()],
                ..MappingReport::default()
            })
            .unwrap();
        for mapping in [&first, &second] {
            journal
                .confirm_mapping(&mapping.rom_key, &mapping.relative_path)
                .unwrap();
        }
        (journal, vec![first, second])
    }

    #[test]
    fn debounce_is_trailing_edge_per_save_and_same_size_writes_are_observed() {
        let dir = tempfile::tempdir().unwrap();
        let (mut journal, mappings) = setup(dir.path());
        let start = Instant::now();
        let mut scheduler = SaveSyncScheduler::new(Duration::from_secs(5));

        let first = scheduler
            .observe(&mut journal, &mappings[0], start)
            .unwrap();
        let second = scheduler
            .observe(&mut journal, &mappings[1], start + Duration::from_secs(2))
            .unwrap();
        let one = match first {
            ObserveResult::Scheduled { due_at, .. } => due_at,
            other => panic!("unexpected observation: {other:?}"),
        };
        let two = match second {
            ObserveResult::Scheduled { due_at, .. } => due_at,
            other => panic!("unexpected observation: {other:?}"),
        };
        let duplicate = scheduler
            .observe(&mut journal, &mappings[0], start + Duration::from_secs(3))
            .unwrap();
        assert!(matches!(duplicate, ObserveResult::Unchanged(_)));
        assert_eq!(
            scheduler.take_due(one),
            vec![DueSnapshot {
                mapping: mappings[0].clone(),
                generation: 1,
                content_hash: journal
                    .slot(&mappings[0].rom_key)
                    .unwrap()
                    .unwrap()
                    .current_local_hash
                    .unwrap(),
            }]
        );
        assert_eq!(scheduler.take_due(two).len(), 1);

        fs::write(
            dir.path()
                .join("retrobat/saves")
                .join(&mappings[0].relative_path),
            b"diff-size-content",
        )
        .unwrap(); // Same length, different bytes.
        let changed = scheduler
            .observe(&mut journal, &mappings[0], start + Duration::from_secs(4))
            .unwrap();
        assert!(matches!(
            changed,
            ObserveResult::Scheduled { generation: 2, .. }
        ));
        assert!(scheduler
            .take_due(start + Duration::from_secs(8))
            .is_empty());
        assert_eq!(scheduler.take_due(start + Duration::from_secs(9)).len(), 1);
    }

    #[test]
    fn reconciling_one_game_does_not_extend_an_unrelated_pending_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let (mut journal, mappings) = setup(dir.path());
        for mapping in &mappings {
            journal.observe_local_save(mapping).unwrap();
        }
        let start = Instant::now();
        let mut scheduler = SaveSyncScheduler::new(Duration::from_secs(10));
        assert_eq!(
            scheduler.restore_dirty(&mut journal, start).unwrap().queued,
            2
        );
        scheduler.hint(&mappings[1], start + Duration::from_secs(3));

        scheduler
            .restore_dirty_for(
                &mut journal,
                &mappings[0].rom_key,
                start + Duration::from_secs(5),
            )
            .unwrap();

        assert!(scheduler
            .take_due(start + Duration::from_secs(9))
            .is_empty());
        let first_deadline = scheduler.take_due(start + Duration::from_secs(10));
        assert_eq!(first_deadline.len(), 1);
        assert_eq!(first_deadline[0].mapping.rom_key, mappings[0].rom_key);
        assert!(scheduler
            .take_due(start + Duration::from_secs(12))
            .is_empty());
        let unrelated_deadline = scheduler.take_due(start + Duration::from_secs(13));
        assert_eq!(unrelated_deadline.len(), 1);
        assert_eq!(unrelated_deadline[0].mapping.rom_key, mappings[1].rom_key);
    }

    #[test]
    fn missing_save_cancels_pending_capture_without_becoming_upload_work() {
        let dir = tempfile::tempdir().unwrap();
        let (mut journal, mappings) = setup(dir.path());
        let start = Instant::now();
        let mut scheduler = SaveSyncScheduler::with_seconds(5);
        scheduler
            .observe(&mut journal, &mappings[0], start)
            .unwrap();
        fs::remove_file(
            dir.path()
                .join("retrobat/saves")
                .join(&mappings[0].relative_path),
        )
        .unwrap();
        assert!(matches!(
            scheduler
                .observe(&mut journal, &mappings[0], start + Duration::from_secs(1))
                .unwrap(),
            ObserveResult::Missing(LocalObservation { changed: true, .. })
        ));
        assert_eq!(scheduler.pending_count(), 0);
        let slot = journal.slot(&mappings[0].rom_key).unwrap().unwrap();
        assert!(slot.local_removed);
        assert!(slot.needs_attention);
    }

    #[test]
    fn restart_requeues_uncaptured_generation_but_not_published_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let saves = dir.path().join("retrobat/saves");
        let database = dir.path().join("settings/save-sync.db");
        let spool = dir.path().join("private");
        let (mut journal, mappings) = setup(dir.path());
        let first = journal.observe_local_save(&mappings[0]).unwrap();
        assert!(first.changed);
        drop(journal);

        let scope = SaveSyncScope {
            server_id: "https://example".into(),
            account_id: "account".into(),
            installation_root: dir.path().join("retrobat"),
            effective_saves_root: saves.clone(),
        };
        let mut reopened = SaveSyncJournal::open(&database, &spool, scope.clone()).unwrap();
        let start = Instant::now();
        let mut scheduler = SaveSyncScheduler::with_seconds(5);
        assert_eq!(
            scheduler
                .restore_dirty(&mut reopened, start)
                .unwrap()
                .queued,
            1
        );
        assert!(scheduler
            .take_due(start + Duration::from_secs(4))
            .is_empty());
        assert_eq!(scheduler.take_due(start + Duration::from_secs(5)).len(), 1);
        let snapshot = reopened
            .capture_snapshot(
                &mappings[0],
                first.generation,
                first.content_hash.as_deref().unwrap(),
            )
            .unwrap();
        assert_eq!(snapshot.state, crate::save_sync::SnapshotState::Ready);
        drop(reopened);

        let mut after_publish = SaveSyncJournal::open(database, spool, scope).unwrap();
        let mut scheduler = SaveSyncScheduler::with_seconds(5);
        assert_eq!(
            scheduler
                .restore_dirty(&mut after_publish, start)
                .unwrap()
                .queued,
            0
        );
        assert_eq!(after_publish.ready_snapshots().unwrap().len(), 1);
    }

    #[test]
    fn capture_race_rehashes_and_requeues_the_new_generation() {
        let dir = tempfile::tempdir().unwrap();
        let saves = dir.path().join("retrobat/saves");
        let (mut journal, mappings) = setup(dir.path());
        let start = Instant::now();
        let mut scheduler = SaveSyncScheduler::with_seconds(5);
        scheduler
            .observe(&mut journal, &mappings[0], start)
            .unwrap();
        fs::write(saves.join(&mappings[0].relative_path), b"diff-size-content").unwrap();

        let result = scheduler.capture_due(&mut journal, start + Duration::from_secs(5));

        assert_eq!(result.len(), 1);
        assert!(result[0].1.is_err());
        assert_eq!(scheduler.pending_count(), 1);
        let current = journal.slot(&mappings[0].rom_key).unwrap().unwrap();
        assert_eq!(current.local_generation, 2);
        assert!(current.snapshot_revision.is_none());
        assert_eq!(scheduler.take_due(start + Duration::from_secs(10)).len(), 1);
    }
}
