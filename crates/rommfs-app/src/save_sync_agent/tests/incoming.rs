// Behavior-focused tests for the save-sync actor.
use super::*;

#[test]
fn first_remote_save_is_published_only_when_target_never_existed() {
    const REMOTE_BYTES: &[u8] = b"remote-only first save";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        REMOTE_BYTES,
        "2026-10-05T12:34:56Z",
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

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), REMOTE_BYTES);
    assert_eq!(slot.remote_slot_id.as_deref(), Some("31"));
    assert_eq!(
        slot.local_baseline_hash,
        Some(sha256_content_hash(REMOTE_BYTES))
    );
    assert!(journal.snapshots().unwrap().is_empty());
    assert!(journal.incoming_saves().unwrap().is_empty());
    assert_eq!(
        fixture.route_hit_count("POST", "/api/saves?rom_id=7&slot="),
        0
    );
}

#[test]
fn truncated_incoming_body_is_rejected_without_staging_or_local_publication() {
    const REMOTE_BYTES: &[u8] = b"truncated remote SRAM body";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        REMOTE_BYTES,
        "2026-10-05T12:34:56Z",
    );
    fixture.on(
        "GET",
        CONTENT_PATH,
        ResponseSpec::Bytes {
            status: 200,
            bytes: REMOTE_BYTES.to_vec(),
            truncate_at: Some(REMOTE_BYTES.len() - 2),
            stall_after_bytes: None,
        },
    );
    let case = case(directory.path(), &fixture, false);
    fs::remove_file(&case.mapping.target_path).unwrap();
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);

    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("incoming reconciliation for Game.gb will retry")
    })
    .is_some());
    agent.stop();

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    assert!(!case.mapping.target_path.exists());
    assert!(journal.incoming_saves().unwrap().is_empty());
    assert_eq!(fixture.route_hit_count("GET", CONTENT_PATH), 1);
    assert_eq!(
        fixture.route_hit_count("POST", "/api/saves?rom_id=7&slot="),
        0
    );
}

#[test]
fn incoming_403_pauses_the_worker_without_invalidating_rom_session_or_repeating_requests() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    fixture.on(
        "GET",
        INVENTORY_PATH,
        ResponseSpec::Json {
            status: 403,
            body: "save permission denied".into(),
        },
    );
    let case = case(directory.path(), &fixture, false);
    let (jobs, updates, active, worker, client) = start_reconciliation_worker(&fixture);
    jobs.send(reconciliation_job(&case)).unwrap();
    assert!(matches!(
        updates.recv_timeout(Duration::from_secs(3)).unwrap(),
        TransferUpdate::Reconciled {
            result: Err(rommfs_core::romm::SaveApiFailure {
                error: rommfs_core::error::Error::Forbidden(_),
                ..
            }),
            ..
        }
    ));
    assert!(
        client.has_token(),
        "HTTP 403 must not invalidate ROM access"
    );
    jobs.send(reconciliation_job(&case)).unwrap();
    thread::sleep(Duration::from_millis(350));
    assert_eq!(fixture.request_count("GET", INVENTORY_PATH), 1);

    active.store(false, std::sync::atomic::Ordering::SeqCst);
    drop(jobs);
    worker.join().unwrap();

    fixture.on("GET", INVENTORY_PATH, ResponseSpec::RomMSaveInventory);
    let (jobs, updates, active, worker, _client) = start_reconciliation_worker(&fixture);
    jobs.send(reconciliation_job(&case)).unwrap();
    assert!(receive_inventory(&updates).saves.is_empty());
    assert_eq!(fixture.request_count("GET", INVENTORY_PATH), 2);
    active.store(false, std::sync::atomic::Ordering::SeqCst);
    drop(jobs);
    worker.join().unwrap();
}

#[test]
fn local_file_created_during_blocked_incoming_body_wins_without_remote_loss() {
    const REMOTE_BYTES: &[u8] = b"downloaded remote bytes";
    const EMULATOR_BYTES: &[u8] = b"new emulator SRAM";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        REMOTE_BYTES,
        "2026-10-05T12:34:56Z",
    );
    let barrier = FixtureBodyBarrier::new();
    fixture.on(
        "GET",
        "/api/saves/31/content?optimistic=false",
        ResponseSpec::HeldBytes {
            status: 200,
            bytes: REMOTE_BYTES.to_vec(),
            barrier: barrier.clone(),
        },
    );
    let case = case(directory.path(), &fixture, false);
    fs::remove_file(&case.mapping.target_path).unwrap();
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(barrier.wait_until_blocked(Duration::from_secs(5)));
    fs::write(&case.mapping.target_path, EMULATOR_BYTES).unwrap();
    barrier.release();
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("first sync found different local and remote saves")
    })
    .is_some());
    agent.stop();

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), EMULATOR_BYTES);
    assert_eq!(
        fs::read(&journal.incoming_saves().unwrap()[0].path).unwrap(),
        REMOTE_BYTES
    );
    assert_eq!(
        fixture.route_hit_count("POST", "/api/saves?rom_id=7&slot="),
        0
    );
}

#[test]
fn disabling_while_incoming_body_is_blocked_prevents_late_publication() {
    const REMOTE_BYTES: &[u8] = b"blocked incoming bytes";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        REMOTE_BYTES,
        "2026-10-05T12:34:56Z",
    );
    let barrier = FixtureBodyBarrier::new();
    fixture.on(
        "GET",
        "/api/saves/31/content?optimistic=false",
        ResponseSpec::HeldBytes {
            status: 200,
            bytes: REMOTE_BYTES.to_vec(),
            barrier: barrier.clone(),
        },
    );
    let case = case(directory.path(), &fixture, false);
    fs::remove_file(&case.mapping.target_path).unwrap();
    let enabled = Arc::new(AtomicBool::new(true));
    let (sink, _events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::clone(&enabled), sink);
    assert!(barrier.wait_until_blocked(Duration::from_secs(5)));
    enabled.store(false, std::sync::atomic::Ordering::SeqCst);
    barrier.release();
    agent.stop();

    assert!(!case.mapping.target_path.exists());
    assert_eq!(
        fixture.route_hit_count("POST", "/api/saves?rom_id=7&slot="),
        0
    );
}

#[test]
fn stopping_the_actor_drains_its_blocked_transfer_worker_before_returning() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        b"pending remote bytes",
        "2026-10-05T12:34:56Z",
    );
    let barrier = FixtureBodyBarrier::new();
    fixture.on(
        "GET",
        "/api/saves/31/content?optimistic=false",
        ResponseSpec::HeldBytes {
            status: 200,
            bytes: b"pending remote bytes".to_vec(),
            barrier: barrier.clone(),
        },
    );
    let case = case(directory.path(), &fixture, false);
    fs::remove_file(&case.mapping.target_path).unwrap();
    let (sink, _events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(barrier.wait_until_blocked(Duration::from_secs(5)));
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let shutdown = thread::spawn(move || {
        started_tx.send(()).unwrap();
        agent.stop();
        done_tx.send(()).unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let stopped_early = done_rx.recv_timeout(Duration::from_millis(200));
    // Release even on assertion failure so fixture teardown never hangs.
    barrier.release();
    assert!(
        matches!(stopped_early, Err(mpsc::RecvTimeoutError::Timeout)),
        "stop returned while its transfer was still blocked"
    );
    done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    shutdown.join().unwrap();
    assert!(!case.mapping.target_path.exists());
}

#[test]
fn cancel_returns_while_inventory_request_is_blocked_and_drain_finishes_after_release() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    let barrier = FixtureBodyBarrier::new();
    fixture.on(
        "GET",
        INVENTORY_PATH,
        ResponseSpec::HeldJson {
            status: 200,
            body: "[]".into(),
            barrier: barrier.clone(),
        },
    );
    let case = case(directory.path(), &fixture, false);
    fs::remove_file(&case.mapping.target_path).unwrap();
    let (sink, _events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(barrier.wait_until_blocked(Duration::from_secs(5)));

    let started = Instant::now();
    agent.cancel();
    assert!(
        started.elapsed() < Duration::from_millis(200),
        "cancel waited for the in-flight inventory response"
    );

    barrier.release();
    agent.stop();
    assert!(!case.mapping.target_path.exists());
}

#[test]
fn interrupted_incoming_publication_is_durable_but_never_retried_as_first_install() {
    const REMOTE_BYTES: &[u8] = b"survives staging interruption";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        REMOTE_BYTES,
        "2026-10-05T12:34:56Z",
    );
    let case = case(directory.path(), &fixture, false);
    fs::remove_file(&case.mapping.target_path).unwrap();
    let incoming_id = {
        let mut journal =
            SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
        let staged = journal
            .stage_incoming(
                &case.mapping,
                "31",
                REMOTE_BYTES,
                "first_remote_save",
                None,
                None,
                None,
            )
            .unwrap();
        let intent = journal
            .begin_incoming_publication(&staged.id, &case.mapping)
            .unwrap();
        intent.id
    };
    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    assert_eq!(journal.incoming_saves().unwrap()[0].id, incoming_id);
    assert_eq!(journal.incoming_saves().unwrap()[0].state, "interrupted");
    assert!(!case.mapping.target_path.exists());
    drop(journal);

    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("tracked local save was removed")
    })
    .is_some());
    agent.stop();
    assert!(!case.mapping.target_path.exists());
}

#[test]
fn external_change_matching_new_remote_bytes_resolves_staged_conflict_without_upload() {
    const REMOTE_BYTES: &[u8] = b"newer remote revision";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        SAVE_BYTES,
        "2026-10-04T12:34:56Z",
    );
    seed_remote(
        &fixture,
        32,
        "550e8400-e29b-41d4-a716-446655440001",
        REMOTE_BYTES,
        "2026-10-05T12:34:56Z",
    );
    let case = case(directory.path(), &fixture, false);
    accept_remote_baseline(&case, "31", SAVE_BYTES);
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("newer save than the local baseline")
    })
    .is_some());
    agent.stop();
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);

    fs::write(&case.mapping.target_path, REMOTE_BYTES).unwrap();
    let (sink, events) = channel();
    let mut restarted = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("reconciled saves for Game.gb")
    })
    .is_some());
    restarted.stop();

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    assert!(!slot.needs_attention);
    assert_eq!(slot.remote_slot_id.as_deref(), Some("32"));
    assert!(journal.incoming_saves().unwrap().is_empty());
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), REMOTE_BYTES);
    assert_eq!(
        fixture.route_hit_count("POST", "/api/saves?rom_id=7&slot="),
        0
    );
}
