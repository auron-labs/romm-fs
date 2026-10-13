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
    let initial_posts = fixture.count_requests("POST", "/api/saves?rom_id=7&slot=");
    assert_eq!(initial_posts, 1);

    fs::write(&case.mapping.target_path, NEXT_SAVE).unwrap();
    assert!(wait_for_log(&events, Duration::from_secs(10), |message| {
        message.contains("verified upload revision")
    })
    .is_some());
    agent.stop();

    assert_eq!(
        fixture.count_requests("POST", "/api/saves?rom_id=7&slot="),
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
    assert_eq!(fixture.count("GET", INVENTORY_PATH), 0);
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
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
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
    assert_eq!(fixture.count_requests("GET", INVENTORY_PATH), 2);
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
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
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
        missing_fixture.count("POST", "/api/saves?rom_id=7&slot="),
        0
    );
}
