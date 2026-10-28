// Behavior-focused tests for the save-sync actor.
use super::*;

#[test]
fn terminal_actor_failure_emits_session_queue_with_durable_counts() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let case = case(directory.path(), &fixture, true);
    let (sink, events) = channel();
    let agent = SaveSyncAgent::start(
        case.scope.clone(),
        vec![case.mapping.clone()],
        SaveSyncIdentity {
            account_id: ACCOUNT_ID,
            scopes: vec!["assets.read".into()],
        },
        authenticated_client(&fixture),
        case.database.clone(),
        case.spool.clone(),
        0,
        1,
        Arc::new(AtomicBool::new(true)),
        sink,
        None,
    )
    .unwrap();

    let status = wait_for_queue_status(&events, Duration::from_secs(5), |status| {
        status.actor_failed
    })
    .expect("a stopped actor must emit its terminal failure status");
    assert_eq!(status.session_id, 1);
    assert_eq!(status.mapped_games, 1);
    assert_eq!(status.reconciled_games, 0);
    assert_eq!(status.pending_outbound, 1);
    assert!(status.failure.as_deref().unwrap().contains("assets.write"));
    drop(agent);
}

#[test]
fn real_filesystem_notification_drives_the_agent_without_waiting_for_fallback_scan() {
    const NEXT_SAVE: &[u8] = b"native watcher save generation";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    let case = case(directory.path(), &fixture, false);
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("verified upload revision")
    })
    .is_some());
    let initial_posts = fixture.request_count("POST", "/api/saves?rom_id=7&slot=");
    assert_eq!(initial_posts, 1);

    fs::write(&case.mapping.target_path, NEXT_SAVE).unwrap();
    assert!(wait_for_log(&events, Duration::from_secs(10), |message| {
        message.contains("verified upload revision")
    })
    .is_some());
    agent.stop();

    assert_eq!(
        fixture.request_count("POST", "/api/saves?rom_id=7&slot="),
        2
    );
    assert_eq!(fixture.saved_saves()[1].bytes, NEXT_SAVE);
}

#[test]
fn disabled_consent_does_not_start_watch_or_offer_a_durable_revision() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let case = case(directory.path(), &fixture, true);
    let (sink, _events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(false)), sink);
    thread::sleep(Duration::from_millis(200));
    agent.stop();

    let (snapshot, _) = snapshot_after_stop(&case);
    assert_eq!(snapshot.state, SnapshotState::Ready);
    assert_eq!(fixture.route_hit_count("GET", INVENTORY_PATH), 0);
    assert_eq!(
        fixture.route_hit_count("POST", "/api/saves?rom_id=7&slot="),
        0
    );
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);
}

#[test]
fn save_sync_401_emits_scoped_auth_status_and_pauses_further_requests() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.on_sequence(
        "GET",
        INVENTORY_PATH,
        vec![
            ResponseSpec::Json {
                status: 200,
                body: "[]".into(),
            },
            ResponseSpec::Json {
                status: 401,
                body: "expired token".into(),
            },
        ],
    );
    let case = case(directory.path(), &fixture, true);
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut authentication_required = false;
    while Instant::now() < deadline {
        match events.recv_timeout(Duration::from_millis(50)) {
            Ok(AppEvent::SaveSyncAuthenticationRequired { session_id: 1 }) => {
                authentication_required = true;
                break;
            }
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    assert!(
        authentication_required,
        "401 did not mark the current save-sync session as requiring authentication"
    );
    let status = wait_for_queue_status(&events, Duration::from_secs(3), |status| {
        status.authentication_required && status.network_paused
    })
    .expect("401 should be present in the current queue status");
    assert_eq!(status.session_id, 1);
    agent.stop();
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);
    assert_eq!(
        fixture.route_hit_count("POST", "/api/saves?rom_id=7&slot="),
        0
    );
    assert_eq!(fixture.request_count("GET", INVENTORY_PATH), 2);
    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    assert!(journal.incoming_saves().unwrap().is_empty());
}

#[test]
fn conflicted_game_does_not_block_a_healthy_local_only_game() {
    const REMOTE_CONFLICT: &[u8] = b"game one's remote conflict";
    const HEALTHY_LOCAL: &[u8] = b"game two local SRAM";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        91,
        "550e8400-e29b-41d4-a716-446655440000",
        REMOTE_CONFLICT,
        "2026-10-05T12:34:56Z",
    );
    let case = case(directory.path(), &fixture, false);
    let conflict_mapping = case.mapping.clone();
    let healthy_mapping = second_game_mapping(&case);
    fs::create_dir_all(healthy_mapping.target_path.parent().unwrap()).unwrap();
    fs::write(&healthy_mapping.target_path, HEALTHY_LOCAL).unwrap();
    let mappings = vec![conflict_mapping.clone(), healthy_mapping.clone()];
    let mut journal =
        SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    journal
        .reconcile_mappings(&MappingReport {
            mappings: mappings.clone(),
            ..MappingReport::default()
        })
        .unwrap();
    journal
        .confirm_mapping(&healthy_mapping.rom_key, &healthy_mapping.relative_path)
        .unwrap();
    drop(journal);

    let (sink, events) = channel();
    let mut agent = start_agent_with_mappings(
        &case,
        mappings,
        &fixture,
        0,
        Arc::new(AtomicBool::new(true)),
        sink,
    );
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("verified upload revision")
    })
    .is_some());
    agent.stop();

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    assert!(
        journal
            .slot(&conflict_mapping.rom_key)
            .unwrap()
            .unwrap()
            .needs_attention
    );
    assert_eq!(journal.incoming_saves().unwrap().len(), 1);
    assert_eq!(fs::read(&conflict_mapping.target_path).unwrap(), SAVE_BYTES);
    assert_eq!(
        fs::read(&healthy_mapping.target_path).unwrap(),
        HEALTHY_LOCAL
    );
    let uploaded = fixture
        .saved_saves()
        .into_iter()
        .find(|save| save.rom_id == 8)
        .unwrap();
    assert_eq!(uploaded.bytes, HEALTHY_LOCAL);
    assert_eq!(
        fixture.route_hit_count("POST", "/api/saves?rom_id=7&slot="),
        0
    );
}

#[test]
fn explicit_remote_choice_replaces_only_the_reviewed_local_version_and_keeps_backup() {
    const REMOTE_BYTES: &[u8] = b"verified remote SRAM";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        92,
        "550e8400-e29b-41d4-a716-446655440000",
        REMOTE_BYTES,
        "2026-10-05T12:34:56Z",
    );
    seed_remote(
        &fixture,
        96,
        "550e8400-e29b-41d4-a716-446655440000",
        b"competing verified remote SRAM",
        "2026-10-05T12:34:56Z",
    );
    fixture.seed_save(FixtureSaveRecord {
        id: 97,
        rom_id: ROM_ID,
        user_id: ACCOUNT_ID,
        file_name: "foreign-save.srm".into(),
        file_size_bytes: 18,
        slot: RETROBAT_GB_SRM_PROFILE.into(),
        bytes: b"unknown remote save".to_vec(),
        created_at: "2026-10-05T12:34:56Z".into(),
        updated_at: "2026-10-05T12:34:56Z".into(),
    });
    let case = case(directory.path(), &fixture, false);
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(
        wait_for_queue_status(&events, Duration::from_secs(5), |status| {
            status.attention_games > 0
        })
        .is_some()
    );
    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let incoming = journal
        .incoming_saves()
        .unwrap()
        .into_iter()
        .find(|record| record.state == "staged" && record.remote_id == "92")
        .unwrap();
    let old_hash = sha256_content_hash(SAVE_BYTES);
    agent
        .resolve_conflict(
            1,
            case.mapping.rom_key.clone(),
            Some(old_hash),
            Some(incoming.id),
        )
        .unwrap();
    agent.stop();

    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), REMOTE_BYTES);
    let backup = case
        .mapping
        .target_path
        .parent()
        .unwrap()
        .read_dir()
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .contains("rommfs-backup")
        })
        .unwrap();
    assert_eq!(fs::read(backup).unwrap(), SAVE_BYTES);
    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    assert!(!slot.needs_attention);
    assert!(journal.incoming_saves().unwrap().is_empty());
    assert_eq!(
        slot.remote_baseline_hash.as_deref(),
        Some(sha256_content_hash(REMOTE_BYTES).as_str())
    );

    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(
        wait_for_queue_status(&events, Duration::from_secs(5), |status| {
            status.attention_games == 0 && status.reconciled_games > 0
        })
        .is_some(),
        "the chosen RomM revision stays applied after restart despite unchanged competing and rejected history"
    );
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), REMOTE_BYTES);
    agent.stop();
}

#[test]
fn remote_choice_is_rejected_when_the_local_save_changed_after_review() {
    const REMOTE_BYTES: &[u8] = b"verified remote SRAM";
    const CHANGED_LOCAL: &[u8] = b"new emulator write after review";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        98,
        "550e8400-e29b-41d4-a716-446655440000",
        REMOTE_BYTES,
        "2026-10-05T12:34:56Z",
    );
    let case = case(directory.path(), &fixture, false);
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(
        wait_for_queue_status(&events, Duration::from_secs(5), |status| {
            status.attention_games > 0
        })
        .is_some()
    );
    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let incoming = journal
        .incoming_saves()
        .unwrap()
        .into_iter()
        .find(|record| record.state == "staged")
        .unwrap();
    drop(journal);

    fs::write(&case.mapping.target_path, CHANGED_LOCAL).unwrap();
    let error = agent
        .resolve_conflict(
            1,
            case.mapping.rom_key.clone(),
            Some(sha256_content_hash(SAVE_BYTES)),
            Some(incoming.id),
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("local save changed since review"));
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), CHANGED_LOCAL);
    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    assert!(journal
        .incoming_saves()
        .unwrap()
        .iter()
        .any(|record| record.remote_id == "98" && record.state == "staged"));
    agent.stop();
}

#[test]
fn remote_choice_restores_a_staged_save_after_reviewed_local_removal() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        99,
        "550e8400-e29b-41d4-a716-446655440000",
        SAVE_BYTES,
        "2026-10-05T12:34:56Z",
    );
    let case = case(directory.path(), &fixture, false);
    accept_remote_baseline(&case, "99", SAVE_BYTES);
    fs::remove_file(&case.mapping.target_path).unwrap();

    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(
        wait_for_queue_status(&events, Duration::from_secs(5), |status| {
            status
                .incoming
                .iter()
                .any(|incoming| incoming.remote_id == "99" && incoming.state == "staged")
        })
        .is_some()
    );
    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    assert_eq!(slot.current_local_hash, None);
    let incoming = journal
        .incoming_saves()
        .unwrap()
        .into_iter()
        .find(|record| record.state == "staged" && record.remote_id == "99")
        .unwrap();
    drop(journal);

    agent
        .resolve_conflict(1, case.mapping.rom_key.clone(), None, Some(incoming.id))
        .unwrap();
    agent.stop();

    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);
    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    assert!(!slot.needs_attention);
    assert_eq!(slot.conflict_resolution.as_deref(), Some("use_remote_ack"));
    assert_eq!(slot.conflict_local_hash, None);
    assert!(journal.incoming_saves().unwrap().is_empty());

    let (sink, events) = channel();
    let mut restarted = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(
        wait_for_queue_status(&events, Duration::from_secs(5), |status| {
            status.attention_games == 0 && status.reconciled_games > 0
        })
        .is_some()
    );
    restarted.stop();
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);
}

#[test]
fn remote_choice_rejects_a_local_save_created_after_absence_was_reviewed() {
    const NEW_LOCAL: &[u8] = b"new local save after removal review";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        100,
        "550e8400-e29b-41d4-a716-446655440000",
        SAVE_BYTES,
        "2026-10-05T12:34:56Z",
    );
    let case = case(directory.path(), &fixture, false);
    accept_remote_baseline(&case, "100", SAVE_BYTES);
    fs::remove_file(&case.mapping.target_path).unwrap();

    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(
        wait_for_queue_status(&events, Duration::from_secs(5), |status| {
            status
                .incoming
                .iter()
                .any(|incoming| incoming.remote_id == "100" && incoming.state == "staged")
        })
        .is_some()
    );
    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let incoming = journal
        .incoming_saves()
        .unwrap()
        .into_iter()
        .find(|record| record.state == "staged" && record.remote_id == "100")
        .unwrap();
    drop(journal);

    fs::write(&case.mapping.target_path, NEW_LOCAL).unwrap();
    let error = agent
        .resolve_conflict(1, case.mapping.rom_key.clone(), None, Some(incoming.id))
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("local save changed since review"));
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), NEW_LOCAL);
    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    assert!(journal
        .incoming_saves()
        .unwrap()
        .iter()
        .any(|record| record.remote_id == "100" && record.state == "staged"));
    agent.stop();
}

#[test]
fn explicit_local_choice_uploads_against_unchanged_acknowledged_history() {
    const REMOTE_BYTES: &[u8] = b"remote SRAM kept in history";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        93,
        "550e8400-e29b-41d4-a716-446655440000",
        REMOTE_BYTES,
        "2026-10-05T12:34:56Z",
    );
    let case = case(directory.path(), &fixture, false);
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(
        wait_for_queue_status(&events, Duration::from_secs(5), |status| {
            status.attention_games > 0
        })
        .is_some()
    );
    agent
        .resolve_conflict(
            1,
            case.mapping.rom_key.clone(),
            Some(sha256_content_hash(SAVE_BYTES)),
            None,
        )
        .unwrap();
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("verified upload revision")
    })
    .is_some());
    agent.stop();

    let remote = fixture.saved_saves();
    assert_eq!(remote.len(), 2, "the kept RomM revision remains present");
    assert!(remote.iter().any(|save| save.bytes == REMOTE_BYTES));
    assert!(remote.iter().any(|save| save.bytes == SAVE_BYTES));
    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    assert!(!slot.needs_attention);
    assert_eq!(slot.conflict_resolution.as_deref(), Some("keep_local_ack"));
}

#[test]
fn keep_local_ack_with_rejected_history_survives_restart_and_later_uploads() {
    const REMOTE_BYTES: &[u8] = b"remote history kept after choice";
    const NEXT_LOCAL: &[u8] = b"later emulator save";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        94,
        "550e8400-e29b-41d4-a716-446655440000",
        REMOTE_BYTES,
        "2026-10-05T12:34:56Z",
    );
    fixture.seed_save(FixtureSaveRecord {
        id: 95,
        rom_id: ROM_ID,
        user_id: ACCOUNT_ID,
        file_name: "foreign-save.srm".into(),
        file_size_bytes: 18,
        slot: RETROBAT_GB_SRM_PROFILE.into(),
        bytes: b"unknown remote save".to_vec(),
        created_at: "2026-10-05T12:34:56Z".into(),
        updated_at: "2026-10-05T12:34:56Z".into(),
    });
    let case = case(directory.path(), &fixture, false);
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(
        wait_for_queue_status(&events, Duration::from_secs(5), |status| {
            status.attention_games > 0
        })
        .is_some()
    );
    agent
        .resolve_conflict(
            1,
            case.mapping.rom_key.clone(),
            Some(sha256_content_hash(SAVE_BYTES)),
            None,
        )
        .unwrap();
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("verified upload revision")
    })
    .is_some());
    agent.stop();

    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(
        wait_for_queue_status(&events, Duration::from_secs(5), |status| {
            status.attention_games == 0 && status.reconciled_games > 0
        })
        .is_some()
    );
    fs::write(&case.mapping.target_path, NEXT_LOCAL).unwrap();
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("verified upload revision")
    })
    .is_some());
    agent.stop();

    let remote = fixture.saved_saves();
    assert!(remote.iter().any(|save| save.bytes == REMOTE_BYTES));
    assert!(remote
        .iter()
        .any(|save| save.file_name == "foreign-save.srm" && save.bytes == b"unknown remote save"));
    assert!(remote.iter().any(|save| save.bytes == SAVE_BYTES));
    assert!(remote.iter().any(|save| save.bytes == NEXT_LOCAL));
    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    assert!(!slot.needs_attention);
    assert_eq!(slot.conflict_resolution.as_deref(), Some("keep_local_ack"));
    assert!(journal.incoming_saves().unwrap().is_empty());
}

#[test]
fn keep_pc_after_verified_baseline_appends_a_new_revision_for_competing_history() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    let case = case(directory.path(), &fixture, false);
    let reviewed_hash = sha256_content_hash(SAVE_BYTES);

    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("verified upload revision")
    })
    .is_some());
    agent.stop();
    assert_eq!(
        fixture.request_count("POST", "/api/saves?rom_id=7&slot="),
        1
    );

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    assert_eq!(
        slot.local_baseline_hash.as_deref(),
        Some(reviewed_hash.as_str())
    );
    assert_eq!(
        slot.current_local_hash.as_deref(),
        Some(reviewed_hash.as_str())
    );
    drop(journal);

    fixture.seed_save(FixtureSaveRecord {
        id: 500,
        rom_id: ROM_ID,
        user_id: ACCOUNT_ID,
        file_name: "foreign-save.srm".into(),
        file_size_bytes: 18,
        slot: RETROBAT_GB_SRM_PROFILE.into(),
        bytes: b"unknown remote save".to_vec(),
        created_at: "2026-10-06T12:34:56Z".into(),
        updated_at: "2026-10-06T12:34:56Z".into(),
    });
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(
        wait_for_queue_status(&events, Duration::from_secs(5), |status| {
            status.attention_games > 0
        })
        .is_some()
    );
    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    assert!(slot.needs_attention);
    assert_eq!(
        slot.current_local_hash.as_deref(),
        Some(reviewed_hash.as_str())
    );
    assert_eq!(
        slot.local_baseline_hash.as_deref(),
        Some(reviewed_hash.as_str())
    );
    drop(journal);

    agent
        .resolve_conflict(
            1,
            case.mapping.rom_key.clone(),
            Some(reviewed_hash.clone()),
            None,
        )
        .unwrap();
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("verified upload revision")
    })
    .is_some());
    agent.stop();

    assert_eq!(
        fixture.request_count("POST", "/api/saves?rom_id=7&slot="),
        2
    );
    let remote = fixture.saved_saves();
    assert!(remote.iter().any(|save| save.id == 500));
    assert!(remote.iter().any(|save| save.bytes == SAVE_BYTES));
    let local_revisions = remote
        .iter()
        .filter(|save| save.file_name.starts_with("rommfs-") && save.bytes == SAVE_BYTES)
        .count();
    assert_eq!(
        local_revisions, 2,
        "Keep PC appends an upload and preserves the accepted revision"
    );

    let (sink, events) = channel();
    let mut restarted = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(
        wait_for_queue_status(&events, Duration::from_secs(5), |status| {
            status.attention_games == 0 && status.reconciled_games > 0
        })
        .is_some()
    );
    restarted.stop();
}

#[test]
fn tracked_local_removal_and_missing_remote_never_delete_or_resurrect_local_saves() {
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
    let removed_case = case(directory.path(), &fixture, false);
    accept_remote_baseline(&removed_case, "31", SAVE_BYTES);
    fs::remove_file(&removed_case.mapping.target_path).unwrap();
    let (sink, events) = channel();
    let mut agent = start_agent(
        &removed_case,
        &fixture,
        0,
        Arc::new(AtomicBool::new(true)),
        sink,
    );
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("tracked local save was removed")
    })
    .is_some());
    agent.stop();
    let status = events
        .try_iter()
        .filter_map(|event| match event {
            AppEvent::SaveSyncQueueUpdated(status) => Some(status),
            _ => None,
        })
        .last()
        .expect("removal has a current queue result");
    assert_eq!(status.attention_games, 1);
    assert_eq!(
        status.games[0].work,
        rommfs_core::events::SaveSyncWork::NeedsReview
    );
    assert!(!removed_case.mapping.target_path.exists());
    let journal = SaveSyncJournal::open(
        &removed_case.database,
        &removed_case.spool,
        removed_case.scope.clone(),
    )
    .unwrap();
    assert!(
        journal
            .slot(&removed_case.mapping.rom_key)
            .unwrap()
            .unwrap()
            .local_removed
    );
    assert_eq!(journal.incoming_saves().unwrap().len(), 1);

    let missing_fixture = FixtureServer::start();
    missing_fixture.use_romm_save_store(ACCOUNT_ID, 201);
    let missing_directory = tempfile::tempdir().unwrap();
    let missing_case = case(missing_directory.path(), &missing_fixture, false);
    accept_remote_baseline(&missing_case, "44", SAVE_BYTES);
    let original = fs::read(&missing_case.mapping.target_path).unwrap();
    let (sink, events) = channel();
    let mut missing_agent = start_agent(
        &missing_case,
        &missing_fixture,
        0,
        Arc::new(AtomicBool::new(true)),
        sink,
    );
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("previously accepted remote save is missing")
    })
    .is_some());
    missing_agent.stop();
    assert_eq!(
        fs::read(&missing_case.mapping.target_path).unwrap(),
        original
    );
    assert_eq!(
        missing_fixture.route_hit_count("POST", "/api/saves?rom_id=7&slot="),
        0
    );
}
