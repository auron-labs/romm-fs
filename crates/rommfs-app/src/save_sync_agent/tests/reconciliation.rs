// Behavior-focused tests for the save-sync actor.
use super::*;

#[test]
fn remote_history_fingerprint_changes_when_same_id_metadata_changes() {
    let original = rommfs_core::romm::RemoteSave {
        id: 31,
        rom_id: ROM_ID,
        user_id: ACCOUNT_ID,
        file_name: "rommfs-revision.srm".into(),
        file_size_bytes: 16,
        missing_from_fs: false,
        created_at: "2026-10-05T12:34:56Z".into(),
        updated_at: "2026-10-05T12:34:56Z".into(),
        emulator: None,
        slot: Some(RETROBAT_GB_SRM_PROFILE.into()),
    };
    let mut changed = original.clone();
    changed.file_name = "rommfs-revision-edited.srm".into();
    assert_eq!(original.id, changed.id);
    assert_ne!(
        super::super::incoming_transfer::remote_save_fingerprint(&original),
        super::super::incoming_transfer::remote_save_fingerprint(&changed)
    );
}

#[test]
fn reconciliation_failure_for_one_game_survives_another_games_success() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    fixture.on(
        "GET",
        "/api/saves?rom_id=7&slot=",
        ResponseSpec::Json {
            status: 400,
            body: r#"{"detail":"game-specific inventory failure"}"#.into(),
        },
    );
    let case = case(directory.path(), &fixture, false);
    let other_mapping = second_game_mapping(&case);
    let (sink, events) = channel();
    let mut agent = start_agent_with_mappings(
        &case,
        vec![case.mapping.clone(), other_mapping],
        &fixture,
        3600,
        Arc::new(AtomicBool::new(true)),
        sink,
    );

    let status = wait_for_queue_status(&events, Duration::from_secs(5), |status| {
        status.mapped_games == 2 && status.reconciled_games == 1 && status.failure.is_some()
    })
    .expect("game 7 failure should remain visible after game 8 succeeds");
    assert!(status.failure.as_deref().unwrap().contains("400"));
    assert!(status.games.iter().any(|game| {
        game.rom_id == ROM_ID
            && game
                .issue
                .as_deref()
                .is_some_and(|issue| issue.contains("400"))
    }));
    assert!(status
        .games
        .iter()
        .any(|game| game.rom_id == 8 && game.issue.is_none()));
    agent.stop();
}

#[test]
fn revision_matching_accepts_only_exact_app_uuid_and_verified_server_tag() {
    let revision = "550e8400-e29b-41d4-a716-446655440000";
    assert!(remote_filename_matches_revision(
        &format!("rommfs-{revision}.srm"),
        revision
    ));
    assert!(remote_filename_matches_revision(
        &format!("rommfs-{revision} [2026-10-05_12-34-56].srm"),
        revision
    ));
    for filename in [
        format!("prefix-rommfs-{revision} [2026-10-05_12-34-56].srm"),
        format!("rommfs-{revision} [2026-10-5_12-34-56].srm"),
        format!("rommfs-{revision} [2026-10-05 12-34-56].srm"),
        format!("rommfs-{revision} [2026-10-05_12-34-56].srm.backup"),
        "rommfs-550e8400-e29b-41d4-a716-446655440001 [2026-10-05_12-34-56].srm".into(),
    ] {
        assert!(
            !remote_filename_matches_revision(&filename, revision),
            "unexpected revision adoption: {filename}"
        );
    }
}

#[test]
fn runtime_revalidation_rejects_redirected_root_or_changed_mapping_and_identity() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let case = case(directory.path(), &fixture, false);
    let identity = SaveSyncIdentity {
        account_id: ACCOUNT_ID,
        scopes: vec!["assets.read".into(), "assets.write".into()],
    };
    validate_configuration(&case.scope, std::slice::from_ref(&case.mapping), &identity).unwrap();

    fs::create_dir_all(case.scope.installation_root.join("redirected-saves")).unwrap();
    fs::write(
        case.scope
            .installation_root
            .join("emulationstation/emulatorLauncher.cfg"),
        "home=.\\.emulationstation\nsaves=redirected-saves\n",
    )
    .unwrap();
    assert!(
        validate_configuration(&case.scope, std::slice::from_ref(&case.mapping), &identity)
            .unwrap_err()
            .to_string()
            .contains("root changed")
    );

    fs::write(
        case.scope
            .installation_root
            .join("emulationstation/emulatorLauncher.cfg"),
        "home=.\\.emulationstation\nsaves=.\\..\\saves\n",
    )
    .unwrap();
    let mut changed_mapping = case.mapping.clone();
    changed_mapping.target_path = case.scope.effective_saves_root.join("gb/Other.srm");
    assert!(validate_configuration(&case.scope, &[changed_mapping], &identity).is_err());

    let wrong_identity = SaveSyncIdentity {
        account_id: ACCOUNT_ID + 1,
        scopes: identity.scopes.clone(),
    };
    assert!(validate_configuration(
        &case.scope,
        std::slice::from_ref(&case.mapping),
        &wrong_identity
    )
    .is_err());
}

#[test]
fn identical_first_sync_establishes_a_baseline_without_posting_or_replacing_local_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        SAVE_BYTES,
        "2026-10-05T12:34:56Z",
    );
    let case = case(directory.path(), &fixture, false);
    let original = fs::read(&case.mapping.target_path).unwrap();
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);

    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("reconciled saves for Game.gb")
    })
    .is_some());
    agent.stop();

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), original);
    assert_eq!(
        fixture.route_hit_count("POST", "/api/saves?rom_id=7&slot="),
        0
    );
    assert_eq!(slot.remote_slot_id.as_deref(), Some("31"));
    assert_eq!(slot.local_baseline_hash, slot.remote_baseline_hash);
    assert!(journal.incoming_saves().unwrap().is_empty());
}

#[test]
fn repeated_reconciliation_reuses_verified_content_until_metadata_changes() {
    const FIRST_BYTES: &[u8] = b"cached remote bytes";
    const UPDATED_BYTES: &[u8] = b"updated remote metadata and bytes";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        FIRST_BYTES,
        "2026-10-05T12:34:56Z",
    );
    let first_case = case(directory.path(), &fixture, false);
    let (jobs, updates, active, worker, _client) = start_reconciliation_worker(&fixture);

    for _ in 0..2 {
        jobs.send(reconciliation_job(&first_case)).unwrap();
        let inventory = receive_inventory(&updates);
        assert_eq!(inventory.saves[0].bytes, FIRST_BYTES);
    }
    assert_eq!(fixture.request_count("GET", "/api/saves/31/content"), 1);

    fixture.seed_save(FixtureSaveRecord {
        id: 31,
        rom_id: ROM_ID,
        user_id: ACCOUNT_ID,
        file_name: "rommfs-550e8400-e29b-41d4-a716-446655440000.srm".into(),
        file_size_bytes: UPDATED_BYTES.len(),
        slot: RETROBAT_GB_SRM_PROFILE.into(),
        bytes: UPDATED_BYTES.to_vec(),
        created_at: "2026-10-05T12:34:56Z".into(),
        updated_at: "2026-10-05T12:34:57Z".into(),
    });
    jobs.send(reconciliation_job(&first_case)).unwrap();
    let inventory = receive_inventory(&updates);
    assert_eq!(inventory.saves[0].bytes, UPDATED_BYTES);
    assert_eq!(fixture.request_count("GET", "/api/saves/31/content"), 2);

    let other_installation = tempfile::tempdir().unwrap();
    let other_case = case(other_installation.path(), &fixture, false);
    jobs.send(reconciliation_job(&other_case)).unwrap();
    assert_eq!(receive_inventory(&updates).saves[0].bytes, UPDATED_BYTES);
    assert_eq!(fixture.request_count("GET", "/api/saves/31/content"), 3);

    active.store(false, std::sync::atomic::Ordering::SeqCst);
    drop(jobs);
    worker.join().unwrap();
}

#[test]
fn unordered_remote_history_uses_unique_latest_timestamp_and_remembers_all_ids() {
    const OLDER_BYTES: &[u8] = b"older remote save";
    const LATEST_BYTES: &[u8] = b"latest remote save";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        OLDER_BYTES,
        "2026-10-04T12:34:56Z",
    );
    seed_remote(
        &fixture,
        32,
        "550e8400-e29b-41d4-a716-446655440001",
        LATEST_BYTES,
        "2026-10-05T12:34:56Z",
    );
    let case = case(directory.path(), &fixture, false);
    fs::remove_file(&case.mapping.target_path).unwrap();
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);

    let reconciliation = wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("Game.gb")
    });
    assert!(
        reconciliation
            .as_deref()
            .is_some_and(|message| message.contains("installed first remote save")),
        "unexpected reconciliation result: {reconciliation:?}"
    );
    agent.stop();

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), LATEST_BYTES);
    assert_eq!(slot.remote_slot_id.as_deref(), Some("32"));
    let mut history_ids = slot.remote_history_ids;
    history_ids.sort();
    assert_eq!(history_ids, ["31", "32"]);
}

#[test]
fn fractional_rfc3339_precision_selects_the_newest_same_second_record() {
    const EARLIER_BYTES: &[u8] = b"earlier fractional revision";
    const LATER_BYTES: &[u8] = b"later fractional revision";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        EARLIER_BYTES,
        "2026-10-05T12:34:56.123456788Z",
    );
    seed_remote(
        &fixture,
        32,
        "550e8400-e29b-41d4-a716-446655440001",
        LATER_BYTES,
        "2026-10-05T12:34:56.123456789Z",
    );
    let case = case(directory.path(), &fixture, false);
    fs::remove_file(&case.mapping.target_path).unwrap();
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);

    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("installed first remote save for Game.gb")
    })
    .is_some());
    agent.stop();

    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), LATER_BYTES);
    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    assert_eq!(
        journal
            .slot(&case.mapping.rom_key)
            .unwrap()
            .unwrap()
            .remote_slot_id
            .as_deref(),
        Some("32")
    );
}

#[test]
fn invalid_history_dates_and_offsets_fail_without_publishing_a_remote_save() {
    for timestamp in ["2026-02-30T12:34:56Z", "2026-10-05T12:34:56+25:00"] {
        let directory = tempfile::tempdir().unwrap();
        let fixture = FixtureServer::start();
        fixture.use_romm_save_store(ACCOUNT_ID, 201);
        seed_remote(
            &fixture,
            31,
            "550e8400-e29b-41d4-a716-446655440000",
            b"unorderable remote save",
            timestamp,
        );
        let case = case(directory.path(), &fixture, false);
        fs::remove_file(&case.mapping.target_path).unwrap();
        let (sink, events) = channel();
        let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);

        assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
            message.contains("inventory failed for Game.gb")
        })
        .is_some());
        agent.stop();

        let journal =
            SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
        assert!(!case.mapping.target_path.exists(), "{timestamp}");
        assert!(journal.incoming_saves().unwrap().is_empty(), "{timestamp}");
        assert_eq!(
            fixture.route_hit_count("POST", "/api/saves?rom_id=7&slot="),
            0
        );
    }
}

#[test]
fn tied_remote_history_requires_attention_without_publishing_either_save() {
    const FIRST_BYTES: &[u8] = b"first tied remote save";
    const SECOND_BYTES: &[u8] = b"second tied remote save";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        FIRST_BYTES,
        "2026-10-05T12:34:56Z",
    );
    seed_remote(
        &fixture,
        32,
        "550e8400-e29b-41d4-a716-446655440001",
        SECOND_BYTES,
        "2026-10-05T14:34:56+02:00",
    );
    let case = case(directory.path(), &fixture, false);
    fs::remove_file(&case.mapping.target_path).unwrap();
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);

    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("tie for the latest compatible revision")
    })
    .is_some());
    agent.stop();

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    assert!(slot.needs_attention);
    assert!(!case.mapping.target_path.exists());
    assert_eq!(journal.incoming_saves().unwrap().len(), 2);
    assert_eq!(
        fixture.route_hit_count("POST", "/api/saves?rom_id=7&slot="),
        0
    );
}
