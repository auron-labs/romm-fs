use super::*;
use rommfs_core::save_sync::{
    file_slot, save_file_key, ConsentSettings, SaveProfile, SaveSyncSettingsStore,
};
fn identity() -> SaveSyncIdentity {
    SaveSyncIdentity {
        account_id: ACCOUNT_ID,
        scopes: vec!["assets.read".into(), "assets.write".into()],
    }
}

#[test]
fn clean_agent_discovers_all_file_slots_once_and_keeps_per_file_status() {
    let fx = FixtureServer::start();
    fx.use_romm_save_store(ACCOUNT_ID, 200);
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("saves");
    fs::create_dir(&root).unwrap();
    let server = rommfs_core::catalog::server_id_of(fx.url());
    let scope = SaveSyncScope {
        server_id: server.clone(),
        account_id: ACCOUNT_ID.to_string(),
        installation_root: root.clone(),
        effective_saves_root: root.clone(),
    };
    let platform:PlatformDto=serde_json::from_value(json!({"id":1,"slug":"switch","fs_slug":"switch","name":"Switch","custom_name":null,"rom_count":1})).unwrap();
    let rom:RomDto=serde_json::from_value(json!({"id":ROM_ID,"platform_fs_slug":"switch","platform_slug":"switch","fs_name":"Game.nsp","fs_size_bytes":3,"has_simple_single_file":true,"has_nested_single_file":false,"has_multiple_files":false,"missing_from_fs":false,"is_physical":false,"updated_at":"now","files":[{"id":70,"file_name":"Game.nsp","file_size_bytes":3,"is_top_level":true}]})).unwrap();
    let catalogue = build_catalogue(
        &server,
        std::slice::from_ref(&platform),
        std::slice::from_ref(&rom),
        |_| {},
    )
    .unwrap();
    let rules = rommfs_core::save_sync::SaveFileRules::from_catalogue(
        &catalogue,
        std::slice::from_ref(&rom),
        Default::default(),
    )
    .unwrap();
    let paths = [
        "switch/Game/user/0",
        "switch/Game/clock.rtc",
        "switch/Game/empty",
    ];
    let bodies = [b"extensionless".to_vec(), b"clock".to_vec(), Vec::new()];
    for (i, (path, body)) in paths.iter().zip(&bodies).enumerate() {
        fx.seed_save(FixtureSaveRecord {
            id: i as i64 + 1,
            rom_id: ROM_ID,
            user_id: ACCOUNT_ID,
            file_name: format!("rommfs-{}.bin", uuid::Uuid::new_v4()),
            file_size_bytes: body.len(),
            slot: file_slot(Path::new(path)).unwrap(),
            bytes: body.clone(),
            created_at: "2026-10-08T00:00:00Z".into(),
            updated_at: "2026-10-08T00:00:00Z".into(),
        });
    }
    let (sink, events) = channel();
    let mut agent = SaveSyncAgent::start_with_rules(
        scope,
        Vec::new(),
        identity(),
        Arc::new(RommClient::new(fx.url()).unwrap()),
        directory.path().join("journal.db"),
        directory.path().join("spool"),
        1,
        7,
        Arc::new(AtomicBool::new(true)),
        sink,
        Some(rules),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = std::collections::HashSet::new();
    while seen.len() < paths.len() && Instant::now() < deadline {
        if let Ok(AppEvent::SaveSyncReconciliation { game, .. }) =
            events.recv_timeout(Duration::from_millis(100))
        {
            if game.local_hash.is_some() && game.issue.is_none() {
                seen.insert(game.file_id);
            }
        }
    }
    assert_eq!(seen.len(), paths.len());
    for (path, body) in paths.iter().zip(&bodies) {
        assert_eq!(fs::read(root.join(path)).unwrap(), *body);
    }
    assert_eq!(
        fx.requests()
            .iter()
            .filter(|r| r.method == "GET" && r.target == "/api/saves?")
            .count(),
        1
    );
    assert_eq!(
        fx.count_requests("POST", "/api/saves?"),
        0,
        "incoming watcher feedback must not upload duplicates"
    );
    let new_path = Path::new("switch/Game/user/.hidden-save");
    let new_bytes = b"new file found by watcher";
    fs::write(root.join(new_path), new_bytes).unwrap();
    let key = save_file_key(&server, ROM_ID, new_path).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut uploaded = false;
    let mut trace = Vec::new();
    while Instant::now() < deadline {
        if let Ok(event) = events.recv_timeout(Duration::from_millis(100)) {
            if let AppEvent::SaveSyncQueueUpdated(queue) = &event {
                if queue.games.iter().any(|g| {
                    g.file_id == key.file_id
                        && g.remote_hash.as_deref() == Some(sha256_content_hash(new_bytes).as_str())
                }) {
                    uploaded = true;
                    break;
                }
            }
            trace.push(event);
        }
    }
    agent.stop();
    assert!(
        uploaded,
        "new ordinary file must be discovered, mapped and uploaded without waiting for the 60-second fallback: {trace:?}; requests={:?}",
        fx.requests()
    );
    let slot = file_slot(new_path).unwrap();
    let saves = fx.saved_saves();
    let found: Vec<_> = saves.iter().filter(|s| s.slot == slot).collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].bytes, new_bytes);
}

fn mapping(root: &Path, relative: &str, server: &str) -> SaveMapping {
    SaveMapping {
        rom_key: save_file_key(server, ROM_ID, Path::new(relative)).unwrap(),
        profile: SaveProfile::ordinary_files(),
        relative_path: relative.into(),
        target_path: root.join(relative),
        visible_rom_name: relative.into(),
        legacy_compatible: relative == "gb/Game.srm",
    }
}
fn journal(directory: &Path, scope: &SaveSyncScope, mappings: &[SaveMapping]) -> SaveSyncJournal {
    let mut journal = SaveSyncJournal::open(
        directory.join("journal.db"),
        directory.join("spool"),
        scope.clone(),
    )
    .unwrap();
    journal
        .reconcile_mappings(&MappingReport {
            mappings: mappings.to_vec(),
            ..Default::default()
        })
        .unwrap();
    for mapping in mappings {
        journal
            .confirm_mapping(&mapping.rom_key, &mapping.relative_path)
            .unwrap();
    }
    journal
}
fn reconcile(
    client: &RommClient,
    journal: &mut SaveSyncJournal,
    mapping: &SaveMapping,
    scope: &SaveSyncScope,
) -> super::super::incoming::ReconciliationOutcome {
    let active = AtomicBool::new(true);
    let enabled = Arc::new(AtomicBool::new(true)).into();
    let job = IncomingJob {
        mapping: mapping.clone(),
        identity: identity(),
        scope: scope.clone(),
    };
    let inventory = super::super::incoming_transfer::reconcile_attempt(
        client,
        &job,
        &mut Default::default(),
        &enabled,
        &active,
    )
    .unwrap();
    super::super::incoming::reconcile_inventory(
        journal,
        mapping,
        &identity(),
        scope,
        inventory,
        &enabled,
        &active,
    )
    .unwrap()
}
fn upload(
    client: Arc<RommClient>,
    journal: &mut SaveSyncJournal,
    mapping: &SaveMapping,
    scope: &SaveSyncScope,
) {
    let observed = journal.observe_local_save(mapping).unwrap();
    let snapshot = journal
        .capture_snapshot(
            mapping,
            observed.generation,
            observed.content_hash.as_deref().unwrap(),
        )
        .unwrap();
    let (tx, rx) = mpsc::channel();
    let (updates, received) = mpsc::channel();
    let active = Arc::new(AtomicBool::new(true));
    let worker_active = active.clone();
    let handle = thread::spawn(move || {
        run_transfer_worker(
            rx,
            updates,
            client,
            Arc::new(AtomicBool::new(true)).into(),
            worker_active,
        )
    });
    let slot = journal.slot(&mapping.rom_key).unwrap().unwrap();
    tx.send(WorkerJob::Upload(Box::new(
        super::super::transfer::TransferJob {
            snapshot: snapshot.clone(),
            mapping: mapping.clone(),
            identity: identity(),
            scope: scope.clone(),
            history: journal.snapshots().unwrap(),
            baseline_id: slot.remote_slot_id,
            baseline_hash: slot.remote_baseline_hash,
            accepted_remote_ids: slot.remote_history_ids,
        },
    )))
    .unwrap();
    {
        match received.recv_timeout(Duration::from_secs(10)).unwrap() {
            TransferUpdate::Verified {
                revision,
                remote_id,
                content_hash,
            } => {
                assert_eq!(content_hash, snapshot.content_hash);
                journal
                    .record_remote_complete(&revision, &remote_id, &content_hash)
                    .unwrap();
            }
            other => panic!(
                "unexpected transfer result {}",
                match other {
                    TransferUpdate::Paused { .. } => "paused",
                    TransferUpdate::Retry { .. } => "retry",
                    _ => "other",
                }
            ),
        }
    }
    active.store(false, std::sync::atomic::Ordering::SeqCst);
    drop(tx);
    handle.join().unwrap();
}
#[test]
fn ordinary_file_pipeline_reconstructs_replaces_and_preserves_conflicts_and_removal() {
    let fx = FixtureServer::start();
    fx.use_romm_save_store(ACCOUNT_ID, 200);
    let client = Arc::new(RommClient::new(fx.url()).unwrap());
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let root_a = first.path().join("saves");
    let root_b = second.path().join("saves");
    fs::create_dir(&root_a).unwrap();
    fs::create_dir(&root_b).unwrap();
    let server = rommfs_core::catalog::server_id_of(fx.url());
    let scope_a = SaveSyncScope {
        server_id: server.clone(),
        account_id: ACCOUNT_ID.to_string(),
        installation_root: root_a.clone(),
        effective_saves_root: root_a.clone(),
    };
    let scope_b = SaveSyncScope {
        installation_root: root_b.clone(),
        effective_saves_root: root_b.clone(),
        ..scope_a.clone()
    };
    let paths = [
        "game/bytes.dat",
        "game/clock.rtc",
        "game/nested/user/0",
        "game/empty",
    ];
    let bytes = [
        b"arbitrary".to_vec(),
        b"companion".to_vec(),
        vec![4; 2 * 1024 * 1024 + 7],
        Vec::new(),
    ];
    let maps_a: Vec<_> = paths.iter().map(|p| mapping(&root_a, p, &server)).collect();
    let maps_b: Vec<_> = paths.iter().map(|p| mapping(&root_b, p, &server)).collect();
    let mut a = journal(first.path(), &scope_a, &maps_a);
    for (map, bytes) in maps_a.iter().zip(&bytes) {
        fs::create_dir_all(map.target_path.parent().unwrap()).unwrap();
        fs::write(&map.target_path, bytes).unwrap();
        upload(client.clone(), &mut a, map, &scope_a);
    }
    // Clean client knows only server slots; recreate exact per-file mappings.
    let remote = client.save_inventory(ROM_ID, "").unwrap();
    assert_eq!(remote.len(), paths.len());
    for save in remote {
        assert!(paths.contains(
            &rommfs_core::save_sync::path_from_file_slot(save.slot.as_deref().unwrap())
                .unwrap()
                .to_str()
                .unwrap()
        ));
    }
    let mut b = journal(second.path(), &scope_b, &maps_b);
    for (map, bytes) in maps_b.iter().zip(&bytes) {
        assert!(reconcile(&client, &mut b, map, &scope_b).installed);
        assert_eq!(fs::read(&map.target_path).unwrap(), *bytes);
    }
    assert_eq!(b.slots().unwrap().len(), 4);
    // Server history timestamps are made strictly ordered by the fixture setup.
    let slot = file_slot(&maps_a[0].relative_path).unwrap();
    let revision = uuid::Uuid::new_v4();
    fx.seed_save(FixtureSaveRecord {
        id: 99,
        rom_id: ROM_ID,
        user_id: ACCOUNT_ID,
        file_name: format!("rommfs-{revision}.bin"),
        file_size_bytes: 9,
        slot: slot.clone(),
        bytes: b"new bytes".to_vec(),
        created_at: "2026-10-09T00:00:00Z".into(),
        updated_at: "2026-10-09T00:00:00Z".into(),
    });
    assert!(reconcile(&client, &mut b, &maps_b[0], &scope_b).installed);
    assert_eq!(fs::read(&maps_b[0].target_path).unwrap(), b"new bytes");
    let backups: Vec<_> = fs::read_dir(maps_b[0].target_path.parent().unwrap())
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .contains(".rommfs-backup-")
                .then_some(p)
        })
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read(&backups[0]).unwrap(), bytes[0]);
    drop(b);
    let mut b = journal(second.path(), &scope_b, &maps_b);
    fs::write(&maps_b[0].target_path, b"local edit").unwrap();
    fx.seed_save(FixtureSaveRecord {
        id: 100,
        rom_id: ROM_ID,
        user_id: ACCOUNT_ID,
        file_name: format!("rommfs-{}.bin", uuid::Uuid::new_v4()),
        file_size_bytes: 11,
        slot,
        bytes: b"remote edit".to_vec(),
        created_at: "2026-10-10T00:00:00Z".into(),
        updated_at: "2026-10-10T00:00:00Z".into(),
    });
    let outcome = reconcile(&client, &mut b, &maps_b[0], &scope_b);
    assert!(outcome.conflict);
    assert_eq!(fs::read(&maps_b[0].target_path).unwrap(), b"local edit");
    assert_eq!(fs::read(&outcome.incoming[0].path).unwrap(), b"remote edit");
    fs::remove_file(&maps_b[1].target_path).unwrap();
    assert!(reconcile(&client, &mut b, &maps_b[1], &scope_b).conflict);
    assert!(!maps_b[1].target_path.exists());
}

#[test]
fn legacy_migration_keeps_pending_snapshot_baselines_and_requires_new_consent() {
    let directory = tempfile::tempdir().unwrap();
    let fx = FixtureServer::start();
    let case = case(directory.path(), &fx, false);
    let mut old = journal(
        directory.path(),
        &case.scope,
        std::slice::from_ref(&case.mapping),
    );
    fs::write(&case.mapping.target_path, SAVE_BYTES).unwrap();
    let observation = old.observe_local_save(&case.mapping).unwrap();
    let snapshot = old
        .capture_snapshot(
            &case.mapping,
            observation.generation,
            observation.content_hash.as_deref().unwrap(),
        )
        .unwrap();
    old.record_reconciled_baseline(
        &case.mapping.rom_key,
        Some(&snapshot.content_hash),
        Some("17"),
        Some(&snapshot.content_hash),
        &["17".into()],
    )
    .unwrap();
    let settings = SaveSyncSettingsStore::open(directory.path().join("settings.db")).unwrap();
    let incoming = old
        .stage_incoming(
            &case.mapping,
            "18",
            b"remote conflict",
            "two_sided_conflict",
            Some(&snapshot.content_hash),
            Some("17"),
            Some(&snapshot.content_hash),
        )
        .unwrap();
    fs::remove_file(&case.mapping.target_path).unwrap();
    old.observe_local_save(&case.mapping).unwrap();
    drop(old);
    let database = directory.path().join("journal.db");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute(
            "UPDATE save_sync_slots SET relative_path=?1",
            [
                rommfs_core::save_sync::portable_save_path(&case.mapping.relative_path)
                    .unwrap()
                    .replace('/', "\\"),
            ],
        )
        .unwrap();
    drop(connection);
    let mut old = SaveSyncJournal::open(
        &database,
        directory.path().join("spool"),
        case.scope.clone(),
    )
    .unwrap();
    assert_eq!(
        old.slot(&case.mapping.rom_key)
            .unwrap()
            .unwrap()
            .relative_path,
        case.mapping.relative_path
    );
    settings
        .save(
            &case.scope,
            ConsentSettings {
                enabled: true,
                debounce_secs: 5,
            },
        )
        .unwrap();
    assert!(!settings.load_files(&case.scope).unwrap().enabled);
    let upgraded = mapping(
        &case.scope.effective_saves_root,
        case.mapping.relative_path.to_str().unwrap(),
        &case.scope.server_id,
    );
    old.reconcile_mappings(&MappingReport {
        mappings: vec![upgraded.clone()],
        ..Default::default()
    })
    .unwrap();
    assert!(old.slot(&case.mapping.rom_key).unwrap().is_none());
    let slot = old.slot(&upgraded.rom_key).unwrap().unwrap();
    assert!(slot.mapping_confirmed);
    assert!(slot.local_removed);
    assert_eq!(slot.remote_slot_id.as_deref(), Some("17"));
    assert_eq!(
        slot.local_baseline_hash,
        Some(snapshot.content_hash.clone())
    );
    let migrated = old.snapshots().unwrap();
    assert_eq!(migrated[0].rom_key, upgraded.rom_key);
    assert_eq!(fs::read(&migrated[0].path).unwrap(), SAVE_BYTES);
    let incoming_records = old.incoming_saves().unwrap();
    assert_eq!(incoming_records[0].id, incoming.id);
    assert_eq!(incoming_records[0].rom_key, upgraded.rom_key);
    assert_eq!(
        fs::read(&incoming_records[0].path).unwrap(),
        b"remote conflict"
    );
    old.reconcile_mappings(&MappingReport {
        mappings: vec![upgraded],
        ..Default::default()
    })
    .unwrap();
    settings
        .save_files(
            &case.scope,
            ConsentSettings {
                enabled: true,
                debounce_secs: 5,
            },
        )
        .unwrap();
    assert!(settings.load_files(&case.scope).unwrap().enabled);
}
