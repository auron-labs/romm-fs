// Behavior-focused tests for the save-sync actor.
use super::*;

#[test]
fn production_agent_uploads_only_after_inventory_and_byte_readback() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    let case = case(directory.path(), &fixture, true);
    let (sink, events) = channel();
    let enabled = Arc::new(AtomicBool::new(true));
    let mut agent = start_agent(&case, &fixture, 0, enabled, sink);

    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("verified upload revision")
    })
    .is_some());
    agent.stop();

    let (snapshot, slot) = snapshot_after_stop(&case);
    assert_eq!(snapshot.state, SnapshotState::RemoteComplete);
    assert_eq!(snapshot.remote_slot_id.as_deref(), Some("31"));
    assert_eq!(slot.remote_slot_id.as_deref(), Some("31"));
    assert_eq!(
        slot.remote_baseline_hash.as_deref(),
        Some(snapshot.content_hash.as_str())
    );
    assert_eq!(fixture.count_requests("GET", INVENTORY_PATH), 3);
    assert_eq!(
        fixture.count_requests("POST", "/api/saves?rom_id=7&slot="),
        1
    );
    assert_eq!(fixture.count_requests("GET", CONTENT_PATH), 1);
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);
    assert!(fixture
        .requests()
        .iter()
        .filter(|request| request.target != "/api/token")
        .all(|request| request
            .headers
            .get("authorization")
            .is_some_and(|value| value == "Bearer fixture-access-token")));
}

#[test]
fn queue_status_counts_an_uncaptured_local_generation_as_pending_outbound() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let case = case(directory.path(), &fixture, false);
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 3600, Arc::new(AtomicBool::new(true)), sink);

    let status = wait_for_queue_status(&events, Duration::from_secs(5), |status| {
        status.mapped_games == 1 && status.pending_outbound > 0
    })
    .expect("dirty local generation should be visible in queue status before capture");
    assert_eq!(status.reconciled_games, 0);
    assert_eq!(status.pending_outbound, 1);
    assert!(
        SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone())
            .unwrap()
            .snapshots()
            .unwrap()
            .is_empty()
    );
    agent.stop();
}

#[test]
fn one_game_upload_retry_remains_visible_after_another_game_verifies() {
    const SECOND_GAME_SAVE: &[u8] = b"healthy game save";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    fixture.on_sequence(
        "POST",
        "/api/saves?rom_id=7&slot=",
        vec![
            ResponseSpec::JsonWithHeaders {
                status: 503,
                body: r#"{"detail":"temporarily unavailable"}"#.into(),
                headers: vec![("Retry-After".into(), "1".into())],
            },
            ResponseSpec::RomMSaveUpload {
                status: 201,
                user_id: ACCOUNT_ID,
            },
        ],
    );
    let case = case(directory.path(), &fixture, true);
    let other_mapping = second_game_mapping(&case);
    fs::create_dir_all(other_mapping.target_path.parent().unwrap()).unwrap();
    fs::write(&other_mapping.target_path, SECOND_GAME_SAVE).unwrap();
    let (sink, events) = channel();
    let mut agent = start_agent_with_mappings(
        &case,
        vec![case.mapping.clone(), other_mapping],
        &fixture,
        0,
        Arc::new(AtomicBool::new(true)),
        sink,
    );

    let status = wait_for_queue_status(&events, Duration::from_secs(8), |status| {
        status
            .failure
            .as_deref()
            .is_some_and(|failure| failure.contains("503"))
            && status.pending_outbound > 0
            && fixture.saved_saves().iter().any(|save| save.rom_id == 8)
    })
    .expect("game 7's retry should remain visible while game 8 succeeds");
    let failed_game = status
        .games
        .iter()
        .find(|game| game.rom_id == ROM_ID)
        .unwrap();
    assert!(failed_game
        .issue
        .as_deref()
        .is_some_and(|issue| issue.contains("503")));
    assert!(status
        .games
        .iter()
        .find(|game| game.rom_id == 8)
        .unwrap()
        .issue
        .is_none());
    assert_eq!(
        fixture.count_requests("POST", "/api/saves?rom_id=7&slot="),
        1
    );
    assert_eq!(
        fixture.count_requests("POST", "/api/saves?rom_id=8&slot="),
        1
    );
    let recovered_status = wait_for_queue_status(&events, Duration::from_secs(8), |status| {
        status.failure.is_none() && status.pending_outbound == 0 && fixture.saved_saves().len() == 2
    })
    .expect("the retry status should clear after its revision verifies");
    assert!(recovered_status
        .games
        .iter()
        .find(|game| game.rom_id == ROM_ID)
        .unwrap()
        .issue
        .is_none());
    assert_eq!(
        fixture.count_requests("POST", "/api/saves?rom_id=7&slot="),
        2
    );
    agent.stop();
}

#[test]
fn successive_revisions_wait_for_the_prior_baseline_to_be_verified() {
    const NEXT_SAVE_BYTES: &[u8] = b"other battery save";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    let case = case(directory.path(), &fixture, true);
    let first = case.snapshot.as_ref().unwrap();
    let mut journal =
        SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    fs::write(&case.mapping.target_path, NEXT_SAVE_BYTES).unwrap();
    let observation = journal.observe_local_save(&case.mapping).unwrap();
    let second = journal
        .capture_snapshot(
            &case.mapping,
            observation.generation,
            observation.content_hash.as_deref().unwrap(),
        )
        .unwrap();
    drop(journal);

    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);

    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains(&format!("verified upload revision {}", second.revision))
    })
    .is_some());
    agent.stop();

    let (latest, slot) = snapshot_after_stop(&case);
    assert_eq!(latest.state, SnapshotState::RemoteComplete);
    assert_eq!(latest.revision, second.revision);
    assert_eq!(slot.remote_slot_id.as_deref(), Some("32"));
    assert_eq!(
        fixture.count_requests("POST", "/api/saves?rom_id=7&slot="),
        2
    );
    assert_eq!(fixture.count_requests("GET", INVENTORY_PATH), 5);
    assert_eq!(fixture.count_requests("GET", CONTENT_PATH), 2);
    assert_eq!(
        fixture.count_requests("GET", "/api/saves/32/content?optimistic=false"),
        1
    );
    let remote_saves = fixture.saved_saves();
    assert_eq!(remote_saves.len(), 2);
    assert_eq!(
        remote_saves[0].file_name,
        format!("rommfs-{} [2026-10-05_12-34-56].srm", first.revision)
    );
    assert_eq!(
        remote_saves[1].file_name,
        format!("rommfs-{} [2026-10-05_12-34-56].srm", second.revision)
    );
    assert_ne!(first.revision, second.revision);
    assert_eq!(remote_saves[0].bytes, SAVE_BYTES);
    assert_eq!(remote_saves[1].bytes, NEXT_SAVE_BYTES);
    assert_eq!(
        fs::read(&case.mapping.target_path).unwrap(),
        NEXT_SAVE_BYTES
    );
}

#[test]
fn local_write_while_first_upload_response_is_blocked_is_captured_and_uploaded_next() {
    const NEXT_SAVE_BYTES: &[u8] = b"write made during the first HTTP upload";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    let barrier = FixtureBodyBarrier::new();
    fixture.on_sequence(
        "POST",
        "/api/saves?rom_id=7&slot=",
        vec![
            ResponseSpec::HeldRomMSaveUpload {
                user_id: ACCOUNT_ID,
                barrier: barrier.clone(),
            },
            ResponseSpec::RomMSaveUpload {
                status: 201,
                user_id: ACCOUNT_ID,
            },
        ],
    );
    let case = case(directory.path(), &fixture, true);
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(barrier.wait_until_blocked(Duration::from_secs(5)));

    // The fixture has consumed and parsed the real first POST body, but is
    // holding its response. The actor remains free to watch and journal this
    // later emulator generation while the single HTTP worker is blocked.
    fs::write(&case.mapping.target_path, NEXT_SAVE_BYTES).unwrap();
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("captured local revision")
    })
    .is_some());
    barrier.release();
    for _ in 0..2 {
        assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
            message.contains("verified upload revision")
        })
        .is_some());
    }
    agent.stop();

    let remote = fixture.saved_saves();
    assert_eq!(remote.len(), 2);
    assert_eq!(remote[0].bytes, SAVE_BYTES);
    assert_eq!(remote[1].bytes, NEXT_SAVE_BYTES);
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 2);
    assert_eq!(
        fs::read(&case.mapping.target_path).unwrap(),
        NEXT_SAVE_BYTES
    );
}

#[test]
fn ambiguous_post_is_reconciled_after_restart_without_duplicate_upload() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 503);
    let case = case(directory.path(), &fixture, true);
    let (sink, events) = channel();
    let enabled = Arc::new(AtomicBool::new(true));
    let mut first_agent = start_agent(&case, &fixture, 0, Arc::clone(&enabled), sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("503")
    })
    .is_some());
    first_agent.stop();
    let (interrupted, _) = snapshot_after_stop(&case);
    assert_eq!(interrupted.state, SnapshotState::RemoteAmbiguous);

    let (sink, events) = channel();
    let mut restarted_agent = start_agent(&case, &fixture, 0, enabled, sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("reconciled saves for")
    })
    .is_some());
    restarted_agent.stop();

    let (recovered, _) = snapshot_after_stop(&case);
    assert_eq!(recovered.state, SnapshotState::RemoteComplete);
    assert_eq!(
        fixture.count_requests("POST", "/api/saves?rom_id=7&slot="),
        1
    );
    assert_eq!(fixture.count_requests("GET", CONTENT_PATH), 1);
    let stored = fixture.saved_saves();
    assert_eq!(stored.len(), 1);
    assert_eq!(
        stored[0].file_name,
        format!("rommfs-{} [2026-10-05_12-34-56].srm", recovered.revision)
    );
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);
}

#[test]
fn ambiguous_older_snapshot_is_verified_without_uploading_over_a_newer_conflict() {
    const NEWER_SAVE: &[u8] = b"newer local generation";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 503);
    let case = case(directory.path(), &fixture, true);
    let older = case.snapshot.as_ref().unwrap().clone();
    let enabled = Arc::new(AtomicBool::new(true));

    let (sink, events) = channel();
    let mut first_agent = start_agent(&case, &fixture, 0, Arc::clone(&enabled), sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("503")
    })
    .is_some());
    first_agent.stop();

    fs::write(&case.mapping.target_path, NEWER_SAVE).unwrap();
    let mut journal =
        SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let observation = journal.observe_local_save(&case.mapping).unwrap();
    assert_ne!(observation.generation, older.generation);
    let newer = journal
        .capture_snapshot(
            &case.mapping,
            observation.generation,
            observation.content_hash.as_deref().unwrap(),
        )
        .unwrap();
    drop(journal);

    let (sink, events) = channel();
    let mut restarted_agent = start_agent(&case, &fixture, 0, enabled, sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("first sync found different local and remote saves")
    })
    .is_some());
    restarted_agent.stop();

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let snapshots = journal.snapshots().unwrap();
    let recovered_older = snapshots
        .iter()
        .find(|snapshot| snapshot.revision == older.revision)
        .unwrap();
    let still_conflicted_newer = snapshots
        .iter()
        .find(|snapshot| snapshot.revision == newer.revision)
        .unwrap();
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    assert_eq!(recovered_older.state, SnapshotState::RemoteComplete);
    assert_eq!(recovered_older.generation, older.generation);
    assert_eq!(recovered_older.content_hash, older.content_hash);
    assert_eq!(recovered_older.remote_slot_id.as_deref(), Some("31"));
    assert_eq!(still_conflicted_newer.state, SnapshotState::Ready);
    assert!(slot.needs_attention);
    let expected_local_hash = sha256_content_hash(NEWER_SAVE);
    assert_eq!(
        slot.current_local_hash.as_deref(),
        Some(expected_local_hash.as_str())
    );
    let expected_baseline_hash = sha256_content_hash(SAVE_BYTES);
    assert_eq!(
        slot.local_baseline_hash.as_deref(),
        Some(expected_baseline_hash.as_str())
    );
    assert_eq!(slot.remote_slot_id.as_deref(), Some("31"));
    assert_eq!(
        slot.remote_baseline_hash.as_deref(),
        Some(older.content_hash.as_str())
    );
    assert_eq!(journal.incoming_saves().unwrap().len(), 1);
    assert_eq!(
        fixture.count_requests("POST", "/api/saves?rom_id=7&slot="),
        1
    );
    assert_eq!(fixture.saved_saves().len(), 1);
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), NEWER_SAVE);
}

#[test]
fn ambiguous_accepted_snapshot_does_not_resurrect_a_locally_removed_save() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 503);
    let case = case(directory.path(), &fixture, true);
    let accepted = case.snapshot.as_ref().unwrap().clone();
    let enabled = Arc::new(AtomicBool::new(true));

    let (sink, events) = channel();
    let mut first_agent = start_agent(&case, &fixture, 0, Arc::clone(&enabled), sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("503")
    })
    .is_some());
    first_agent.stop();
    fs::remove_file(&case.mapping.target_path).unwrap();

    let (sink, events) = channel();
    let mut restarted_agent = start_agent(&case, &fixture, 0, enabled, sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("tracked local save was removed")
    })
    .is_some());
    restarted_agent.stop();

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let recovered = journal
        .snapshots()
        .unwrap()
        .into_iter()
        .find(|snapshot| snapshot.revision == accepted.revision)
        .unwrap();
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    assert_eq!(recovered.state, SnapshotState::RemoteComplete);
    assert_eq!(recovered.remote_slot_id.as_deref(), Some("31"));
    assert!(slot.local_removed);
    assert!(slot.needs_attention);
    assert_eq!(journal.incoming_saves().unwrap().len(), 1);
    assert!(!case.mapping.target_path.exists());
    assert_eq!(
        fixture.count_requests("POST", "/api/saves?rom_id=7&slot="),
        1
    );
}

#[test]
fn malformed_success_response_is_treated_as_an_ambiguous_post() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let case = case(directory.path(), &fixture, true);
    let snapshot = case.snapshot.as_ref().unwrap();
    let saved_json = remote_save(&snapshot.revision, 31);
    fixture.on_sequence(
        "GET",
        INVENTORY_PATH,
        vec![
            ResponseSpec::Json {
                status: 200,
                body: "[]".into(),
            },
            ResponseSpec::Json {
                status: 200,
                body: "[]".into(),
            },
            ResponseSpec::Json {
                status: 200,
                body: format!("[{saved_json}]"),
            },
        ],
    );
    fixture.on(
        "POST",
        "/api/saves?rom_id=7&slot=",
        ResponseSpec::Json {
            status: 201,
            body: "not a save response".into(),
        },
    );
    install_content_route(&fixture);
    let (sink, events) = channel();
    let enabled = Arc::new(AtomicBool::new(true));
    let mut first_agent = start_agent(&case, &fixture, 0, Arc::clone(&enabled), sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("invalid catalogue payload")
    })
    .is_some());
    first_agent.stop();
    let (interrupted, _) = snapshot_after_stop(&case);
    assert_eq!(interrupted.state, SnapshotState::RemoteAmbiguous);

    let (sink, events) = channel();
    let mut restarted_agent = start_agent(&case, &fixture, 0, enabled, sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("reconciled saves for")
    })
    .is_some());
    restarted_agent.stop();

    let (recovered, _) = snapshot_after_stop(&case);
    assert_eq!(recovered.state, SnapshotState::RemoteComplete);
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 1);
    assert_eq!(fixture.count("GET", CONTENT_PATH), 1);
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);
}

#[test]
fn preexisting_remote_save_requires_attention_and_is_never_overwritten() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let case = case(directory.path(), &fixture, true);
    fixture.on(
        "GET",
        INVENTORY_PATH,
        ResponseSpec::Json {
            status: 200,
            body: format!(
                "[{}]",
                json!({
                    "id": 91,
                    "rom_id": ROM_ID,
                    "user_id": ACCOUNT_ID,
                    "file_name": "another-user-save.srm",
                    "file_size_bytes": SAVE_BYTES.len(),
                    "slot": RETROBAT_GB_SRM_PROFILE,
                })
            ),
        },
    );
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("outside the verified owned revision format")
    })
    .is_some());
    agent.stop();

    let (snapshot, slot) = snapshot_after_stop(&case);
    assert_eq!(snapshot.state, SnapshotState::Ready);
    assert!(slot.needs_attention);
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);
}

#[test]
fn upload_metadata_must_match_the_exact_revision_identifier() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let case = case(directory.path(), &fixture, true);
    let snapshot = case.snapshot.as_ref().unwrap();
    let expected_filename = format!("rommfs-{} [2026-10-05_12-34-56].srm", snapshot.revision);
    let invalid_response =
        remote_save(&snapshot.revision, 31).replace(&expected_filename, "different-save.srm");
    fixture.on(
        "GET",
        INVENTORY_PATH,
        ResponseSpec::Json {
            status: 200,
            body: "[]".into(),
        },
    );
    fixture.on(
        "POST",
        "/api/saves?rom_id=7&slot=",
        ResponseSpec::Json {
            status: 201,
            body: invalid_response,
        },
    );
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);

    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("filename")
    })
    .is_some());
    agent.stop();

    let (snapshot, slot) = snapshot_after_stop(&case);
    assert_eq!(snapshot.state, SnapshotState::Failed);
    assert!(slot.needs_attention);
    assert_eq!(fixture.count("GET", CONTENT_PATH), 0);
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);
}

#[test]
fn runtime_debounce_delays_inventory_and_keeps_conflicting_local_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let case = case(directory.path(), &fixture, false);
    fixture.on(
        "GET",
        INVENTORY_PATH,
        ResponseSpec::Json {
            status: 200,
            body: format!(
                "[{}]",
                json!({
                    "id": 92,
                    "rom_id": ROM_ID,
                    "user_id": ACCOUNT_ID,
                    "file_name": "already-on-server.srm",
                    "file_size_bytes": SAVE_BYTES.len(),
                    "slot": RETROBAT_GB_SRM_PROFILE,
                })
            ),
        },
    );
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 1, Arc::new(AtomicBool::new(true)), sink);

    assert!(wait_for_log(&events, Duration::from_secs(4), |message| {
        message.contains("outside the verified owned revision format")
    })
    .is_some());
    assert_eq!(fixture.count_requests("GET", INVENTORY_PATH), 1);
    agent.stop();

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    assert!(journal.snapshots().unwrap().is_empty());
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    assert!(slot.needs_attention);
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);
}

#[test]
fn startup_restores_a_durable_generation_that_was_not_yet_snapshotted() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let case = case(directory.path(), &fixture, false);
    let mut journal =
        SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    assert!(journal.observe_local_save(&case.mapping).unwrap().changed);
    drop(journal);
    fixture.on(
        "GET",
        INVENTORY_PATH,
        ResponseSpec::Json {
            status: 200,
            body: format!(
                "[{}]",
                json!({
                    "id": 93,
                    "rom_id": ROM_ID,
                    "user_id": ACCOUNT_ID,
                    "file_name": "preexisting-save.srm",
                    "file_size_bytes": SAVE_BYTES.len(),
                    "slot": RETROBAT_GB_SRM_PROFILE,
                })
            ),
        },
    );
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);

    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("outside the verified owned revision format")
    })
    .is_some());
    agent.stop();

    let (snapshot, slot) = snapshot_after_stop(&case);
    assert_eq!(snapshot.state, SnapshotState::Ready);
    assert!(slot.needs_attention);
    assert_eq!(fixture.count("GET", INVENTORY_PATH), 1);
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);
}

#[test]
fn inventory_retry_honors_retry_after_without_losing_the_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let case = case(directory.path(), &fixture, true);
    let snapshot = case.snapshot.as_ref().unwrap();
    let saved_json = remote_save(&snapshot.revision, 31);
    fixture.on_sequence(
        "GET",
        INVENTORY_PATH,
        vec![
            ResponseSpec::JsonWithHeaders {
                status: 503,
                body: r#"{"detail":"temporarily unavailable"}"#.into(),
                headers: vec![("Retry-After".into(), "1".into())],
            },
            ResponseSpec::Json {
                status: 200,
                body: "[]".into(),
            },
            ResponseSpec::Json {
                status: 200,
                body: format!("[{saved_json}]"),
            },
        ],
    );
    fixture.on(
        "POST",
        "/api/saves?rom_id=7&slot=",
        ResponseSpec::Json {
            status: 201,
            body: saved_json,
        },
    );
    install_content_route(&fixture);
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(wait_for_log(&events, Duration::from_secs(3), |message| {
        message.contains("temporarily unavailable")
    })
    .is_some());
    thread::sleep(Duration::from_millis(350));
    assert_eq!(fixture.count("GET", INVENTORY_PATH), 1);
    assert!(wait_for_log(&events, Duration::from_secs(4), |message| {
        message.contains("verified upload revision")
    })
    .is_some());
    agent.stop();

    let (snapshot, _) = snapshot_after_stop(&case);
    assert_eq!(snapshot.state, SnapshotState::RemoteComplete);
    assert_eq!(fixture.count("GET", INVENTORY_PATH), 4);
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);
}

#[test]
fn inventory_429_honors_retry_after_on_the_real_worker() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    fixture.on(
        "GET",
        INVENTORY_PATH,
        ResponseSpec::JsonWithHeaders {
            status: 429,
            body: "rate limited".into(),
            headers: vec![("Retry-After".into(), "3".into())],
        },
    );
    let case = case(directory.path(), &fixture, false);
    let (jobs, updates, active, worker, _client) = start_reconciliation_worker(&fixture);
    jobs.send(reconciliation_job(&case)).unwrap();
    assert!(matches!(
        updates.recv_timeout(Duration::from_secs(3)).unwrap(),
        TransferUpdate::ReconciliationRetry { .. }
    ));
    thread::sleep(Duration::from_millis(2300));
    assert_eq!(fixture.count_requests("GET", INVENTORY_PATH), 1);
    assert!(matches!(
        updates.recv_timeout(Duration::from_secs(2)).unwrap(),
        TransferUpdate::ReconciliationRetry { .. }
    ));
    assert_eq!(fixture.count_requests("GET", INVENTORY_PATH), 2);

    active.store(false, std::sync::atomic::Ordering::SeqCst);
    drop(jobs);
    worker.join().unwrap();
}
