use super::snapshot::copy_save_snapshot;
use super::*;
use crate::catalog::RomKey;
use crate::romm::{PlatformDto, RomDto, RomFileDto};
use crate::save_sync::{map_catalogue, MappingIssue, MappingReport, SaveMapping, UnmappedEntry};
use uuid::Uuid;

fn scope(root: &Path, effective: &Path) -> SaveSyncScope {
    SaveSyncScope {
        server_id: "https://romm.example".into(),
        account_id: "user-42".into(),
        installation_root: root.to_path_buf(),
        effective_saves_root: effective.to_path_buf(),
    }
}

fn mapping() -> SaveMapping {
    let catalogue = crate::catalog::build_catalogue(
        "https://romm.example",
        &[PlatformDto {
            id: 1,
            slug: "gb".into(),
            fs_slug: "gb".into(),
            name: "Game Boy".into(),
            custom_name: None,
            rom_count: 1,
        }],
        &[RomDto {
            id: 12,
            platform_fs_slug: "gb".into(),
            platform_slug: "gb".into(),
            fs_name: "Tetris.gb".into(),
            fs_size_bytes: 16,
            has_simple_single_file: true,
            has_nested_single_file: false,
            has_multiple_files: false,
            missing_from_fs: false,
            is_physical: false,
            updated_at: String::new(),
            files: vec![RomFileDto {
                id: 120,
                file_name: "Tetris.gb".into(),
                file_size_bytes: 16,
                last_modified: None,
                crc_hash: None,
                md5_hash: None,
                sha1_hash: None,
                is_top_level: true,
            }],
        }],
        |_| {},
    )
    .unwrap();
    map_catalogue(&catalogue, Path::new("unused"))
        .unwrap()
        .mappings
        .remove(0)
}

fn open(dir: &Path, effective: &Path) -> SaveSyncJournal {
    SaveSyncJournal::open(
        dir.join("settings").join("save-sync.db"),
        dir.join("private-spool"),
        scope(Path::new("D:/RetroBat"), effective),
    )
    .unwrap()
}

fn plan(mapping: SaveMapping) -> MappingReport {
    MappingReport {
        mappings: vec![mapping],
        ..MappingReport::default()
    }
}

fn confirmed_journal(dir: &Path, saves: &Path, target: &Path) -> (SaveSyncJournal, SaveMapping) {
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(target, b"initial SRAM").unwrap();
    let mapping = mapping();
    let mut journal = open(dir, saves);
    journal.reconcile_mappings(&plan(mapping.clone())).unwrap();
    journal
        .confirm_mapping(&mapping.rom_key, &mapping.relative_path)
        .unwrap();
    (journal, mapping)
}

#[test]
fn confirmed_mapping_survives_restart_and_changed_target_needs_attention() {
    let dir = tempfile::tempdir().unwrap();
    let saves = dir.path().join("retrobat/saves");
    let target = saves.join("gb/Tetris.srm");
    let (mut journal, mapping) = confirmed_journal(dir.path(), &saves, &target);
    journal
        .set_remote_baseline(&mapping.rom_key, Some("remote-7"), Some("sha256:base"))
        .unwrap();
    drop(journal);

    let mut reopened = open(dir.path(), &saves);
    reopened.reconcile_mappings(&plan(mapping.clone())).unwrap();
    let retained = reopened.slot(&mapping.rom_key).unwrap().unwrap();
    assert!(retained.mapping_confirmed);
    assert_eq!(retained.relative_path, PathBuf::from("gb/Tetris.srm"));
    assert_eq!(retained.remote_slot_id.as_deref(), Some("remote-7"));

    let mut changed = mapping.clone();
    changed.relative_path = PathBuf::from("gb/Tetris (2).srm");
    changed.target_path = saves.join(&changed.relative_path);
    reopened.reconcile_mappings(&plan(changed)).unwrap();
    let attention = reopened.slot(&mapping.rom_key).unwrap().unwrap();
    assert!(attention.mapping_confirmed);
    assert!(attention.needs_attention);
    assert_eq!(attention.relative_path, PathBuf::from("gb/Tetris.srm"));
    assert_eq!(
        attention.proposed_relative_path,
        Some(PathBuf::from("gb/Tetris (2).srm"))
    );
}

#[test]
fn ambiguous_rescan_cannot_be_reconfirmed_as_the_previous_first_match() {
    let dir = tempfile::tempdir().unwrap();
    let saves = dir.path().join("retrobat/saves");
    let target = saves.join("gb/Tetris.srm");
    let (mut journal, mapping) = confirmed_journal(dir.path(), &saves, &target);
    journal
        .reconcile_mappings(&MappingReport {
            unmapped: vec![UnmappedEntry {
                rom_key: mapping.rom_key.clone(),
                visible_rom_name: mapping.visible_rom_name.clone(),
                issue: MappingIssue::AmbiguousAlias,
            }],
            ..MappingReport::default()
        })
        .unwrap();

    assert!(journal
        .confirm_mapping(&mapping.rom_key, &mapping.relative_path)
        .is_err());
    assert!(journal.observe_local_save(&mapping).is_err());
    let slot = journal.slot(&mapping.rom_key).unwrap().unwrap();
    assert!(slot.mapping_confirmed);
    assert!(slot.needs_attention);
    assert_eq!(slot.relative_path, mapping.relative_path);
}

#[test]
fn catalogue_removal_pauses_a_confirmed_mapping_until_it_is_reconfirmed() {
    let dir = tempfile::tempdir().unwrap();
    let saves = dir.path().join("retrobat/saves");
    let target = saves.join("gb/Tetris.srm");
    let (mut journal, mapping) = confirmed_journal(dir.path(), &saves, &target);

    journal
        .reconcile_mappings(&MappingReport::default())
        .unwrap();
    let missing = journal.slot(&mapping.rom_key).unwrap().unwrap();
    assert!(!missing.candidate_available);
    assert!(missing.needs_attention);
    assert!(journal
        .confirm_mapping(&mapping.rom_key, &mapping.relative_path)
        .is_err());
    assert!(journal.observe_local_save(&mapping).is_err());

    journal.reconcile_mappings(&plan(mapping.clone())).unwrap();
    let returned = journal.slot(&mapping.rom_key).unwrap().unwrap();
    assert!(returned.candidate_available);
    assert!(returned.needs_attention);
    journal
        .confirm_mapping(&mapping.rom_key, &mapping.relative_path)
        .unwrap();
    let confirmed = journal.slot(&mapping.rom_key).unwrap().unwrap();
    assert!(confirmed.candidate_available);
    assert!(!confirmed.needs_attention);
    assert!(journal.observe_local_save(&mapping).is_ok());
}

#[test]
fn absent_local_sram_is_a_valid_mapping_and_not_a_removal() {
    let dir = tempfile::tempdir().unwrap();
    let saves = dir.path().join("saves");
    fs::create_dir_all(&saves).unwrap();
    let mapping = mapping();
    let mut journal = open(dir.path(), &saves);
    journal.reconcile_mappings(&plan(mapping.clone())).unwrap();
    journal
        .confirm_mapping(&mapping.rom_key, &mapping.relative_path)
        .unwrap();

    let result = journal.observe_local_save(&mapping).unwrap();
    let slot = journal.slot(&mapping.rom_key).unwrap().unwrap();
    assert!(!result.present);
    assert_eq!(result.generation, 0);
    assert!(!slot.local_ever_existed);
    assert!(!slot.local_removed);
}

#[test]
fn empty_and_oversized_sram_are_rejected_without_snapshotting() {
    let dir = tempfile::tempdir().unwrap();
    let saves = dir.path().join("retrobat/saves");
    let target = saves.join("gb/Tetris.srm");
    let (mut journal, mapping) = confirmed_journal(dir.path(), &saves, &target);

    fs::write(&target, []).unwrap();
    assert!(journal.observe_local_save(&mapping).is_err());
    fs::write(
        &target,
        vec![0u8; super::super::MAX_SAVE_BYTES as usize + 1],
    )
    .unwrap();
    assert!(journal.observe_local_save(&mapping).is_err());
    assert!(journal.snapshots().unwrap().is_empty());
}

#[test]
fn rtc_sidecar_added_after_mapping_blocks_snapshot_capture() {
    let dir = tempfile::tempdir().unwrap();
    let saves = dir.path().join("retrobat/saves");
    let target = saves.join("gb/Tetris.srm");
    let (mut journal, mapping) = confirmed_journal(dir.path(), &saves, &target);
    let observation = journal.observe_local_save(&mapping).unwrap();
    let original_sram = fs::read(&target).unwrap();
    fs::write(target.with_extension("rtc"), b"RTC data").unwrap();

    assert!(journal
        .capture_snapshot(
            &mapping,
            observation.generation,
            observation.content_hash.as_deref().unwrap(),
        )
        .is_err());
    assert_eq!(fs::read(&target).unwrap(), original_sram);
    assert!(journal.snapshots().unwrap().is_empty());
}

#[test]
fn capture_reads_the_local_save_and_keeps_rom_cache_outside_the_spool() {
    let dir = tempfile::tempdir().unwrap();
    let saves = dir.path().join("retrobat/saves");
    let target = saves.join("gb/Tetris.srm");
    let (mut journal, mapping) = confirmed_journal(dir.path(), &saves, &target);
    let cache_file = dir.path().join("rom-cache/rom-content");
    fs::create_dir_all(cache_file.parent().unwrap()).unwrap();
    fs::write(&cache_file, b"ROM cache content").unwrap();
    let save_before = fs::read(&target).unwrap();

    let observation = journal.observe_local_save(&mapping).unwrap();
    let snapshot = journal
        .capture_snapshot(
            &mapping,
            observation.generation,
            observation.content_hash.as_deref().unwrap(),
        )
        .unwrap();

    assert_eq!(fs::read(&target).unwrap(), save_before);
    assert_eq!(fs::read(cache_file).unwrap(), b"ROM cache content");
    assert_eq!(fs::read(snapshot.path).unwrap(), save_before);
    assert!(!dir.path().join("retrobat/.rommfs").exists());
}

#[test]
fn interrupted_publication_recovers_only_hash_verified_prep_and_keeps_old_rows() {
    let dir = tempfile::tempdir().unwrap();
    let saves = dir.path().join("saves");
    let target = saves.join("gb/Tetris.srm");
    let (mut journal, mapping) = confirmed_journal(dir.path(), &saves, &target);
    let observation = journal.observe_local_save(&mapping).unwrap();
    let revision = Uuid::new_v4().to_string();
    let temporary = journal.private_root.join(format!("{revision}.tmp"));
    let final_path = journal.private_root.join(format!("{revision}.snapshot"));
    let copied_hash = copy_save_snapshot(&saves, &mapping.relative_path, &temporary).unwrap();
    assert_eq!(copied_hash, observation.content_hash.unwrap());
    journal
        .prepare_snapshot(
            &mapping,
            observation.generation,
            &copied_hash,
            &revision,
            &temporary,
            &final_path,
        )
        .unwrap();
    fs::write(journal.private_root.join("orphan.tmp"), b"scratch only").unwrap();
    drop(journal);

    let reopened = open(dir.path(), &saves);
    let ready = reopened.ready_snapshots().unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].revision, revision);
    assert_eq!(fs::read(&ready[0].path).unwrap(), b"initial SRAM");
    assert!(!reopened.private_root.join("orphan.tmp").exists());
    let slot = reopened.slot(&mapping.rom_key).unwrap().unwrap();
    assert_eq!(
        slot.snapshot_revision.as_deref(),
        Some(ready[0].revision.as_str())
    );
    assert_eq!(slot.snapshot_hash.as_deref(), Some(copied_hash.as_str()));
}

#[test]
fn incoming_stages_are_isolated_by_server_account_installation_and_save_root() {
    let dir = tempfile::tempdir().unwrap();
    let saves = dir.path().join("retrobat/saves");
    let target = saves.join("gb/Tetris.srm");
    let (mut journal, mapping) = confirmed_journal(dir.path(), &saves, &target);
    journal
        .stage_incoming(
            &mapping,
            "31",
            b"private incoming bytes",
            "first_sync_conflict",
            None,
            None,
            None,
        )
        .unwrap();
    drop(journal);

    let original_scope = scope(Path::new("D:/RetroBat"), &saves);
    let mut isolated_scopes = Vec::new();
    let mut other_server = original_scope.clone();
    other_server.server_id = "https://other.example".into();
    isolated_scopes.push(other_server);
    let mut other_account = original_scope.clone();
    other_account.account_id = "user-43".into();
    isolated_scopes.push(other_account);
    let mut other_installation = original_scope.clone();
    other_installation.installation_root = PathBuf::from("D:/OtherRetroBat");
    isolated_scopes.push(other_installation);
    let mut other_saves_root = original_scope;
    other_saves_root.effective_saves_root = dir.path().join("other-saves");
    isolated_scopes.push(other_saves_root);

    for isolated_scope in isolated_scopes {
        let journal = SaveSyncJournal::open(
            dir.path().join("settings/save-sync.db"),
            dir.path().join("private-spool"),
            isolated_scope,
        )
        .unwrap();
        assert!(journal.incoming_saves().unwrap().is_empty());
    }
}

#[test]
fn tampered_preparation_is_retained_as_failed_and_never_becomes_ready() {
    let dir = tempfile::tempdir().unwrap();
    let saves = dir.path().join("retrobat/saves");
    let target = saves.join("gb/Tetris.srm");
    let (mut journal, mapping) = confirmed_journal(dir.path(), &saves, &target);
    let observation = journal.observe_local_save(&mapping).unwrap();
    let revision = Uuid::new_v4().to_string();
    let temporary = journal.private_root.join(format!("{revision}.tmp"));
    let final_path = journal.private_root.join(format!("{revision}.snapshot"));
    let hash = copy_save_snapshot(&saves, &mapping.relative_path, &temporary).unwrap();
    journal
        .prepare_snapshot(
            &mapping,
            observation.generation,
            &hash,
            &revision,
            &temporary,
            &final_path,
        )
        .unwrap();
    fs::write(&temporary, b"changed after preparation").unwrap();
    drop(journal);

    let reopened = open(dir.path(), &saves);
    let rows = reopened.snapshots().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].revision, revision);
    assert_eq!(rows[0].state, SnapshotState::Failed);
    assert!(rows[0].failure.is_some());
    assert!(reopened.ready_snapshots().unwrap().is_empty());
    assert!(
        reopened
            .slot(&mapping.rom_key)
            .unwrap()
            .unwrap()
            .needs_attention
    );
}

#[test]
fn later_generation_does_not_delete_an_unresolved_completed_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let saves = dir.path().join("retrobat/saves");
    let target = saves.join("gb/Tetris.srm");
    let (mut journal, mapping) = confirmed_journal(dir.path(), &saves, &target);
    let first = journal.observe_local_save(&mapping).unwrap();
    let previous = journal
        .capture_snapshot(
            &mapping,
            first.generation,
            first.content_hash.as_deref().unwrap(),
        )
        .unwrap();
    fs::write(&target, b"newer SRAM bytes").unwrap();
    let second = journal.observe_local_save(&mapping).unwrap();
    let latest = journal
        .capture_snapshot(
            &mapping,
            second.generation,
            second.content_hash.as_deref().unwrap(),
        )
        .unwrap();

    let rows = journal.snapshots().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|row| row.revision == previous.revision));
    assert!(rows.iter().any(|row| row.revision == latest.revision));
    assert_eq!(fs::read(&previous.path).unwrap(), b"initial SRAM");
    assert_eq!(fs::read(&latest.path).unwrap(), b"newer SRAM bytes");
    assert_eq!(journal.ready_snapshots().unwrap(), vec![previous, latest]);
}

#[cfg(unix)]
#[test]
fn ready_snapshot_is_reverified_on_restart_before_it_can_be_offered() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let saves = dir.path().join("retrobat/saves");
    let target = saves.join("gb/Tetris.srm");
    let (mut journal, mapping) = confirmed_journal(dir.path(), &saves, &target);
    let observation = journal.observe_local_save(&mapping).unwrap();
    let snapshot = journal
        .capture_snapshot(
            &mapping,
            observation.generation,
            observation.content_hash.as_deref().unwrap(),
        )
        .unwrap();
    drop(journal);
    fs::set_permissions(&snapshot.path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&snapshot.path, b"tampered snapshot").unwrap();

    let reopened = open(dir.path(), &saves);

    assert!(reopened.ready_snapshots().unwrap().is_empty());
    let persisted = reopened.snapshots().unwrap();
    assert_eq!(persisted[0].state, SnapshotState::Failed);
    assert!(
        reopened
            .slot(&mapping.rom_key)
            .unwrap()
            .unwrap()
            .needs_attention
    );
}

#[test]
fn stale_generation_cannot_replace_newer_local_state_and_snapshots_are_scoped() {
    let dir = tempfile::tempdir().unwrap();
    let saves = dir.path().join("saves");
    let target = saves.join("gb/Tetris.srm");
    let (mut journal, mapping) = confirmed_journal(dir.path(), &saves, &target);
    let first = journal.observe_local_save(&mapping).unwrap();
    fs::write(&target, b"changed bytes").unwrap();
    let second = journal.observe_local_save(&mapping).unwrap();
    assert!(second.generation > first.generation);
    assert!(journal
        .capture_snapshot(
            &mapping,
            first.generation,
            first.content_hash.as_deref().unwrap(),
        )
        .is_err());
    let current = journal.slot(&mapping.rom_key).unwrap().unwrap();
    assert_eq!(current.local_generation, second.generation);
    assert_eq!(current.current_local_hash, second.content_hash);
    assert!(journal.ready_snapshots().unwrap().is_empty());

    let other_scope = SaveSyncScope {
        effective_saves_root: dir.path().join("redirected-saves"),
        ..scope(Path::new("D:/RetroBat"), &saves)
    };
    let isolated = SaveSyncJournal::open(
        dir.path().join("settings/save-sync.db"),
        dir.path().join("private-other"),
        other_scope,
    )
    .unwrap();
    assert!(isolated.slot(&mapping.rom_key).unwrap().is_none());
}

#[test]
fn same_size_content_changes_are_hashed_and_duplicate_events_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let saves = dir.path().join("saves");
    let target = saves.join("gb/Tetris.srm");
    let (mut journal, mapping) = confirmed_journal(dir.path(), &saves, &target);
    let first = journal.observe_local_save(&mapping).unwrap();
    let duplicate = journal.observe_local_save(&mapping).unwrap();
    fs::write(&target, b"changed SRAM").unwrap(); // Same byte length.
    let second = journal.observe_local_save(&mapping).unwrap();

    assert!(first.changed);
    assert!(!duplicate.changed);
    assert_eq!(first.generation, duplicate.generation);
    assert!(second.changed);
    assert_ne!(first.content_hash, second.content_hash);
}

#[test]
fn journal_rejects_wrong_rom_server_and_remote_ambiguous_result_is_durable() {
    let dir = tempfile::tempdir().unwrap();
    let saves = dir.path().join("saves");
    let target = saves.join("gb/Tetris.srm");
    let (mut journal, mapping) = confirmed_journal(dir.path(), &saves, &target);
    let wrong_key = RomKey {
        server_id: "other-server".into(),
        ..mapping.rom_key.clone()
    };
    assert!(journal.slot(&wrong_key).is_err());

    let observation = journal.observe_local_save(&mapping).unwrap();
    let snapshot = journal
        .capture_snapshot(
            &mapping,
            observation.generation,
            observation.content_hash.as_deref().unwrap(),
        )
        .unwrap();
    journal
        .record_remote_outcome(
            &snapshot.revision,
            SnapshotState::RemoteAmbiguous,
            Some("timeout-after-submit"),
            Some("server result could not be confirmed"),
        )
        .unwrap();
    let persisted = journal.snapshots().unwrap();
    assert_eq!(persisted[0].state, SnapshotState::RemoteAmbiguous);
    assert_eq!(
        persisted[0].remote_outcome.as_deref(),
        Some("timeout-after-submit")
    );
    assert_eq!(
        persisted[0].failure.as_deref(),
        Some("server result could not be confirmed")
    );
    assert!(
        !journal
            .slot(&mapping.rom_key)
            .unwrap()
            .unwrap()
            .needs_attention
    );
    assert_eq!(
        journal.ready_snapshots().unwrap(),
        vec![persisted[0].clone()]
    );
}
