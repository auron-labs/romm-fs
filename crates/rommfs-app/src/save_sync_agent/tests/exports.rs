// Behavior-focused tests for the save-sync actor.
use super::*;

#[test]
fn different_first_sync_keeps_both_and_explicit_export_is_separate_and_no_clobber() {
    const REMOTE_BYTES: &[u8] = b"remote first-sync battery";
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
    let original_local = fs::read(&case.mapping.target_path).unwrap();
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("first sync found different local and remote saves")
    })
    .is_some());

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let incoming = journal.incoming_saves().unwrap();
    assert_eq!(incoming.len(), 1);
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), original_local);
    assert_eq!(fs::read(&incoming[0].path).unwrap(), REMOTE_BYTES);
    drop(journal);
    let export = directory.path().join("reviewed-remote.srm");
    agent
        .export_incoming(&incoming[0].id, export.clone())
        .unwrap();
    assert_eq!(fs::read(&export).unwrap(), REMOTE_BYTES);
    assert!(agent
        .export_incoming(&incoming[0].id, export.clone())
        .is_err());
    assert_eq!(fs::read(&export).unwrap(), REMOTE_BYTES);
    agent.stop();

    let outbound_before = fixture.count_requests("POST", "/api/saves?rom_id=7&slot=");
    let inventory_before = fixture.count_requests("GET", INVENTORY_PATH);
    let offline_export = directory
        .path()
        .join("rommfs-incoming-reviewed.rommfs-incoming");
    export_pending_incoming(
        &case.scope,
        std::slice::from_ref(&case.mapping),
        &SaveSyncIdentity {
            account_id: ACCOUNT_ID,
            scopes: Vec::new(),
        },
        &case.database,
        &case.spool,
        &incoming[0].id,
        &offline_export,
    )
    .unwrap();
    assert_eq!(fs::read(&offline_export).unwrap(), REMOTE_BYTES);
    assert_eq!(
        fixture.count_requests("POST", "/api/saves?rom_id=7&slot="),
        outbound_before
    );
    assert_eq!(
        fixture.count_requests("GET", INVENTORY_PATH),
        inventory_before
    );

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    assert_eq!(journal.incoming_saves().unwrap().len(), 1);
    assert!(
        journal
            .slot(&case.mapping.rom_key)
            .unwrap()
            .unwrap()
            .needs_attention
    );
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
}

#[test]
fn invalid_export_components_and_redirected_parent_fail_before_filesystem_mutation() {
    const REMOTE_BYTES: &[u8] = b"private staged save for export validation";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let case = case(directory.path(), &fixture, false);
    let export_root = directory.path().join("exports");
    fs::create_dir_all(&export_root).unwrap();
    let staged = case.spool.join("export-validation.srm");
    fs::write(&staged, REMOTE_BYTES).unwrap();
    let expected_hash = sha256_content_hash(REMOTE_BYTES);

    for filename in [
        "unsafe.srm:stream",
        "CON.srm",
        "CONIN$.srm",
        "trailing-dot..",
        "trailing-space.srm ",
    ] {
        let result = super::files::export_incoming(
            &staged,
            &export_root.join(filename),
            &expected_hash,
            &case.scope,
            std::slice::from_ref(&case.mapping.target_path),
        );
        assert!(result.is_err(), "accepted invalid name: {filename:?}");
    }
    let traversal = export_root.join("..").join("escaped-export.srm");
    assert!(super::files::export_incoming(
        &staged,
        &traversal,
        &expected_hash,
        &case.scope,
        std::slice::from_ref(&case.mapping.target_path),
    )
    .is_err());
    assert_eq!(fs::read_dir(&export_root).unwrap().count(), 0);
    assert!(!directory.path().join("escaped-export.srm").exists());
    let missing_parent = directory.path().join("missing-export-parent");
    assert!(super::files::export_incoming(
        &staged,
        &missing_parent.join("remote.srm"),
        &expected_hash,
        &case.scope,
        std::slice::from_ref(&case.mapping.target_path),
    )
    .is_err());
    assert!(!missing_parent.exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let redirected = directory.path().join("redirected-parent");
        let external = directory.path().join("outside-export");
        fs::create_dir(&external).unwrap();
        symlink(&external, &redirected).unwrap();
        assert!(super::files::export_incoming(
            &staged,
            &redirected.join("remote.srm"),
            &expected_hash,
            &case.scope,
            std::slice::from_ref(&case.mapping.target_path),
        )
        .is_err());
        assert_eq!(fs::read_dir(&external).unwrap().count(), 0);
    }
}

#[cfg(windows)]
#[test]
fn canonicalized_export_parent_accepts_save_export() {
    const REMOTE_BYTES: &[u8] = b"canonicalized incoming export bytes";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let case = case(directory.path(), &fixture, false);
    let export_root = directory.path().join("exports");
    fs::create_dir(&export_root).unwrap();
    let canonical_export_root = fs::canonicalize(&export_root).unwrap();
    let staged = case.spool.join("canonicalized-export.srm");
    fs::write(&staged, REMOTE_BYTES).unwrap();
    let destination = canonical_export_root.join("remote.srm");

    super::files::export_incoming(
        &staged,
        &destination,
        &sha256_content_hash(REMOTE_BYTES),
        &case.scope,
        std::slice::from_ref(&case.mapping.target_path),
    )
    .unwrap();

    assert_eq!(fs::read(destination).unwrap(), REMOTE_BYTES);
}
