use super::*;

#[test]
fn missing_previous_save_sync_selection_stays_selected_and_paused() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("previous-retrobat");
    let newly_found = dir.path().join("new-install");
    std::fs::create_dir_all(newly_found.join("emulationstation/config")).unwrap();
    std::fs::create_dir_all(newly_found.join("emulators/retroarch")).unwrap();
    std::fs::create_dir(newly_found.join("saves")).unwrap();
    std::fs::write(newly_found.join("RetroBat.exe"), b"exe").unwrap();
    std::fs::write(
        newly_found.join("emulationstation/config/es_systems.cfg"),
        "<systemList/>",
    )
    .unwrap();
    std::fs::write(
        newly_found.join("emulators/retroarch/retroarch.cfg"),
        "# config",
    )
    .unwrap();
    let (sink, events) = rommfs_core::events::channel();
    let mut worker = Worker::new(sink);
    worker.save_sync_selected_root = Some(missing.clone());

    worker.refresh_save_sync_from(DiscoveryInput {
        drive_roots: vec![(
            newly_found.clone(),
            rommfs_core::save_sync::InstallationSource::FixedDrive,
        )],
        process_images: Vec::new(),
    });

    assert!(matches!(
        events.try_recv(),
        Ok(AppEvent::SaveSyncSessionChanged { session_id: 1 })
    ));
    let event = events.try_recv().unwrap();
    match event {
        AppEvent::SaveSyncUpdated {
            candidates,
            selected_root,
            selected_problem,
            ..
        } => {
            assert_eq!(candidates.len(), 1);
            assert_eq!(candidates[0].info.install_root, newly_found);
            assert_eq!(selected_root, Some(missing.display().to_string()));
            assert!(selected_problem
                .unwrap()
                .contains("previously selected RetroBat installation is missing"));
        }
        other => panic!("expected save-sync state event, got {other:?}"),
    }
}

#[test]
fn manual_save_sync_selection_is_validated_and_remembered() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("chosen-retrobat");
    std::fs::create_dir_all(root.join("emulationstation/config")).unwrap();
    std::fs::create_dir_all(root.join("emulators/retroarch")).unwrap();
    std::fs::create_dir(root.join("saves")).unwrap();
    std::fs::write(root.join("RetroBat.exe"), b"exe").unwrap();
    std::fs::write(
        root.join("emulationstation/config/es_systems.cfg"),
        "<systemList/>",
    )
    .unwrap();
    std::fs::write(root.join("emulators/retroarch/retroarch.cfg"), "# config").unwrap();

    let (sink, events) = rommfs_core::events::channel();
    let mut worker = Worker::new(sink);
    worker.save_sync_settings =
        Some(SaveSyncSettingsStore::open(dir.path().join("settings/save-sync.db")).unwrap());
    worker.save_sync_settings_problem = Some("Selection was not saved: transient failure".into());
    worker.select_save_sync_installation(&root);
    assert!(worker.save_sync_settings_problem.is_none());

    assert!(matches!(
        events.try_recv(),
        Ok(AppEvent::SaveSyncSessionChanged { session_id: 1 })
    ));
    let event = events.try_recv().unwrap();
    match event {
        AppEvent::SaveSyncUpdated {
            selected_root,
            documented_saves_root,
            effective_saves_root,
            enabled,
            debounce_secs,
            selected_problem,
            ..
        } => {
            assert_eq!(selected_root, Some(root.display().to_string()));
            assert_eq!(
                documented_saves_root,
                Some(root.join("saves").display().to_string())
            );
            assert!(!enabled);
            assert_eq!(debounce_secs, rommfs_core::save_sync::DEFAULT_DEBOUNCE_SECS);
            assert!(selected_problem.is_some());
            assert!(effective_saves_root.is_none());
        }
        other => panic!("expected save-sync state event, got {other:?}"),
    }
    assert_eq!(
        worker
            .save_sync_settings
            .as_ref()
            .unwrap()
            .selected_installation()
            .unwrap(),
        Some(root)
    );
}

#[test]
fn save_sync_enable_request_is_refused_without_verified_account_and_save_root() {
    let (sink, events) = rommfs_core::events::channel();
    let mut worker = Worker::new(sink);

    worker.set_save_sync_enabled(true);

    let mut enabled = true;
    let mut debounce_secs = 0;
    let mut problem = None;
    for event in events.try_iter() {
        if let AppEvent::SaveSyncUpdated {
            enabled: event_enabled,
            debounce_secs: event_debounce,
            selected_problem,
            ..
        } = event
        {
            enabled = event_enabled;
            debounce_secs = event_debounce;
            problem = selected_problem;
        }
    }
    assert!(!enabled);
    assert_eq!(debounce_secs, rommfs_core::save_sync::DEFAULT_DEBOUNCE_SECS);
    assert!(problem
        .unwrap()
        .contains("verify the account, save folder, and game mappings first"));
}

#[test]
fn stale_save_sync_status_and_transfer_events_are_rejected_after_scope_change() {
    let mut state = UiState::new(32);
    state.apply_event(&AppEvent::SaveSyncSessionChanged { session_id: 8 });
    state.apply_event(&AppEvent::SaveSyncAuthenticationRequired { session_id: 7 });
    assert_ne!(state.conn, ConnectionState::SignInRequired);
    state.apply_event(&AppEvent::SaveSyncQueueUpdated(SaveSyncQueueStatus {
        session_id: 7,
        mapped_games: 1,
        reconciled_games: 1,
        pending_outbound: 0,
        pending_incoming: 0,
        attention_games: 0,
        network_paused: false,
        authentication_required: false,
        actor_failed: false,
        failure: None,
        games: Vec::new(),
        incoming: Vec::new(),
    }));
    state.apply_event(&AppEvent::SaveSyncTransferProgress {
        session_id: 7,
        rom_id: 3,
        revision: "stale-revision".into(),
        phase: "verified".into(),
        detail: None,
    });
    assert!(state.save_sync_queue.is_none());
    assert!(state.save_sync_transfers.is_empty());

    state.apply_event(&AppEvent::SaveSyncAuthenticationRequired { session_id: 8 });
    assert!(state.save_sync_authentication_required);
    assert_eq!(state.conn, ConnectionState::SignInRequired);

    state.apply_event(&AppEvent::SaveSyncQueueUpdated(SaveSyncQueueStatus {
        session_id: 8,
        mapped_games: 1,
        reconciled_games: 0,
        pending_outbound: 2,
        pending_incoming: 1,
        attention_games: 1,
        network_paused: false,
        authentication_required: false,
        actor_failed: false,
        failure: Some("network retry".into()),
        games: Vec::new(),
        incoming: Vec::new(),
    }));
    let queue = state.save_sync_queue.as_ref().unwrap();
    assert_eq!(queue.pending_outbound, 2);
    assert_eq!(queue.reconciled_games, 0);
    assert_eq!(state.save_sync_pending_incoming, 1);
    assert_eq!(state.save_sync_attention_games, 1);
}

#[test]
fn invalid_debounce_values_do_not_change_persisted_consent() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("save-sync.db");
    let scope = SaveSyncScope {
        server_id: "fixture-server".into(),
        account_id: "42".into(),
        installation_root: dir.path().join("RetroBat"),
        effective_saves_root: dir.path().join("RetroBat/saves"),
    };
    let settings = SaveSyncSettingsStore::open(&database).unwrap();
    settings
        .save(
            &scope,
            ConsentSettings {
                enabled: false,
                debounce_secs: 17,
            },
        )
        .unwrap();
    drop(settings);

    let (sink, events) = rommfs_core::events::channel();
    let mut worker = Worker::new(sink);
    worker.save_sync_settings = Some(SaveSyncSettingsStore::open(&database).unwrap());
    worker.save_sync_scope = Some(scope.clone());
    worker.save_sync_debounce_secs = 17;
    worker.set_save_sync_debounce(0);
    worker.set_save_sync_debounce(3601);

    assert_eq!(
        worker
            .save_sync_settings
            .as_ref()
            .unwrap()
            .load(&scope)
            .unwrap()
            .debounce_secs,
        17
    );
    assert!(events.try_iter().any(|event| matches!(
        event,
        AppEvent::SaveSyncUpdated {
            selected_problem: Some(problem),
            ..
        } if problem.contains("1 to 3600 seconds")
    )));
}

#[test]
fn preview_reports_mapped_targets_separately_from_empty_existing_saves_before_consent() {
    let directory = tempfile::tempdir().unwrap();
    let install_root = directory.path().join("RetroBat");
    create_test_retrobat_install(&install_root, false);
    let settings_dir = directory.path().join("settings");
    let settings = SaveSyncSettingsStore::open(settings_dir.join("save-sync.db")).unwrap();
    let fixture = FixtureServer::start();
    let server_id = server_id_of(fixture.url());
    let client = Arc::new(RommClient::new(fixture.url()).unwrap());
    let rom = RomDto {
        id: 7,
        platform_fs_slug: "gb".into(),
        platform_slug: "gb".into(),
        fs_name: "Game.gb".into(),
        fs_size_bytes: 1,
        has_simple_single_file: true,
        has_nested_single_file: false,
        has_multiple_files: false,
        missing_from_fs: false,
        is_physical: false,
        updated_at: String::new(),
        metadata: Default::default(),
        rom_user: None,
        fs_path: String::new(),
        files: vec![RomFileDto {
            id: 70,
            file_name: "Game.gb".into(),
            file_size_bytes: 1,
            last_modified: None,
            crc_hash: None,
            md5_hash: None,
            sha1_hash: None,
            is_top_level: true,
            file_path: String::new(),
            category: None,
        }],
    };
    let catalogue = build_catalogue(
        &server_id,
        &[PlatformDto {
            id: 1,
            slug: "gb".into(),
            fs_slug: "gb".into(),
            name: "Game Boy".into(),
            custom_name: None,
            rom_count: 1,
        }],
        &[rom],
        |_| {},
    )
    .unwrap();
    let (sink, events) = rommfs_core::events::channel();
    let mut worker = Worker::new(sink);
    worker.save_sync_settings = Some(settings);
    worker.save_sync_storage_dir = Some(settings_dir);
    worker.save_sync_selected_root = Some(install_root.clone());
    worker.server_id = Some(server_id);
    worker.client = Some(client);
    worker.save_sync_identity = Some(SaveSyncIdentity {
        account_id: 42,
        scopes: vec!["assets.read".into(), "assets.write".into()],
    });
    worker.save_sync_catalogue = Some(catalogue);

    worker.refresh_save_sync_from(DiscoveryInput::default());

    let update = events
        .try_iter()
        .find_map(|event| match event {
            AppEvent::SaveSyncUpdated {
                mapped_targets,
                catalogue_unmapped,
                existing_saves,
                available,
                enabled,
                ..
            } => Some((
                mapped_targets,
                catalogue_unmapped,
                existing_saves.unwrap(),
                available,
                enabled,
            )),
            _ => None,
        })
        .expect("worker preview update");
    assert_eq!(update.0, 1);
    assert_eq!(update.1, 0);
    assert!(
        update.3,
        "an absent local save must not block incoming sync"
    );
    assert!(!update.4, "consent remains off by default");
    assert_eq!(
        update.2.status,
        rommfs_core::save_sync::ExistingSaveScanStatus::Complete
    );
    assert_eq!(update.2.supported_files, 0);
    assert_eq!(update.2.skipped_files, 0);
    assert!(!install_root.join("saves").exists());
    assert!(
        fixture.requests().is_empty(),
        "preview is local-only before consent"
    );
}

#[test]
fn disable_during_connect_prevents_the_older_scope_refresh_from_starting_sync() {
    let directory = tempfile::tempdir().unwrap();
    let install_root = directory.path().join("RetroBat");
    let save_root = install_root.join("saves");
    create_test_retrobat_install(&install_root, true);

    let fixture = FixtureServer::start();
    let account_barrier = FixtureBodyBarrier::new();
    let inventory_barrier = FixtureBodyBarrier::new();
    fixture.on(
        "POST",
        "/api/token",
        ResponseSpec::Json {
            status: 200,
            body: rommfs_fixture::contract::token_ok(),
        },
    );
    fixture.on(
        "GET",
        "/api/users/me",
        ResponseSpec::HeldBytes {
            status: 200,
            bytes: br#"{"id":42,"oauth_scopes":["me.read","assets.read","assets.write"]}"#.to_vec(),
            barrier: account_barrier.clone(),
        },
    );
    fixture.on(
        "GET",
        "/api/platforms",
        ResponseSpec::Json {
            status: 200,
            body: rommfs_fixture::contract::platforms(&[(1, "gb", "gb", "Game Boy")]),
        },
    );
    let rom = rommfs_fixture::contract::rom(7, "gb", "Game.gb", 1, "abc");
    fixture.on(
        "GET",
        "/api/roms?",
        ResponseSpec::Json {
            status: 200,
            body: rommfs_fixture::contract::roms_page(&[rom], 1, 100, 0),
        },
    );
    fixture.use_romm_save_store(42, 201);
    fixture.seed_save(FixtureSaveRecord {
        id: 31,
        rom_id: 7,
        user_id: 42,
        file_name: "rommfs-550e8400-e29b-41d4-a716-446655440000 [2026-10-05_12-34-56].srm".into(),
        file_size_bytes: 19,
        slot: RETROBAT_GB_SRM_PROFILE.into(),
        bytes: b"incoming save bytes".to_vec(),
        created_at: "2026-10-05T12:34:56Z".into(),
        updated_at: "2026-10-05T12:34:56Z".into(),
    });
    fixture.on(
        "GET",
        "/api/saves?",
        ResponseSpec::HeldBytes {
            status: 200,
            bytes: serde_json::json!([{
                "id": 31,
                "rom_id": 7,
                "user_id": 42,
                "file_name": "rommfs-550e8400-e29b-41d4-a716-446655440000 [2026-10-05_12-34-56].srm",
                "file_size_bytes": 19,
                "missing_from_fs": false,
                "created_at": "2026-10-05T12:34:56Z",
                "updated_at": "2026-10-05T12:34:56Z",
                "emulator": "retroarch-gambatte",
                "slot": RETROBAT_GB_SRM_PROFILE,
            }])
            .to_string()
            .into_bytes(),
            barrier: inventory_barrier.clone(),
        },
    );

    let settings_dir = directory.path().join("settings");
    let settings = SaveSyncSettingsStore::open(settings_dir.join("save-sync.db")).unwrap();
    let scope = SaveSyncScope {
        server_id: server_id_of(fixture.url()),
        account_id: "42".into(),
        installation_root: install_root.clone(),
        effective_saves_root: save_root,
    };
    settings
        .save_files(
            &scope,
            ConsentSettings {
                enabled: true,
                debounce_secs: 1,
            },
        )
        .unwrap();

    let path = PathBuf::from("gb/Game.srm");
    let mapping = rommfs_core::save_sync::SaveMapping {
        rom_key: rommfs_core::save_sync::save_file_key(&scope.server_id, 7, &path).unwrap(),
        profile: rommfs_core::save_sync::SaveProfile::ordinary_files(),
        target_path: scope.effective_saves_root.join(&path),
        relative_path: path,
        visible_rom_name: "Game.gb".into(),
        legacy_compatible: true,
    };
    let mut journal = SaveSyncJournal::open(
        settings_dir.join("save-sync-journal.db"),
        settings_dir.join("save-sync-snapshots"),
        scope.clone(),
    )
    .unwrap();
    journal
        .reconcile_mappings(&rommfs_core::save_sync::MappingReport {
            mappings: vec![mapping.clone()],
            ..Default::default()
        })
        .unwrap();
    journal
        .confirm_mapping(&mapping.rom_key, &mapping.relative_path)
        .unwrap();
    drop(journal);

    let (sink, events) = rommfs_core::events::channel();
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let gate = SaveSyncCommandGate::new();
    let worker_gate = gate.clone();
    let worker_sink = sink.clone();
    let worker_settings_dir = settings_dir.clone();
    let worker_install_root = install_root.clone();
    let worker = std::thread::spawn(move || {
        let mut worker = Worker::new_with_gate(worker_sink, worker_gate);
        worker.save_sync_settings = Some(settings);
        worker.save_sync_storage_dir = Some(worker_settings_dir);
        worker.save_sync_selected_root = Some(worker_install_root);
        worker_loop_with_interval_from(cmd_rx, worker, Duration::from_secs(3600));
    });
    let controller = Controller {
        sink,
        cmd_tx,
        worker: std::sync::Mutex::new(Some(worker)),
        save_sync_gate: gate,
        connection_gate: SaveSyncCommandGate::new(),
    };

    controller.send(Command::Connect {
        url: fixture.url().into(),
        username: "user".into(),
        password: "password".into(),
    });
    assert!(account_barrier.wait_until_blocked(Duration::from_secs(5)));
    controller.send(Command::SetSaveSyncEnabled { enabled: false });
    let disabled_epoch = controller.save_sync_gate.current_epoch();
    account_barrier.release();

    let inventory_started = inventory_barrier.wait_until_blocked(Duration::from_secs(3));
    inventory_barrier.release();
    let mut saw_disabled_status = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !saw_disabled_status && std::time::Instant::now() < deadline {
        if let Ok(AppEvent::SaveSyncUpdated {
            enabled,
            available,
            mapped_targets,
            account_id,
            ..
        }) = events.recv_timeout(Duration::from_millis(100))
        {
            assert!(!enabled, "an older connection refresh reopened save sync");
            if available {
                assert_eq!(mapped_targets, 1);
                assert_eq!(account_id, Some(42));
                saw_disabled_status = true;
            }
        }
    }

    assert!(
        saw_disabled_status,
        "worker did not report the disabled scope"
    );
    assert!(
        !inventory_started,
        "the stale refresh started an inventory request"
    );
    assert!(!controller.save_sync_gate.enabled_for(disabled_epoch));
    assert_eq!(fixture.request_count("POST", "/api/saves?"), 0);
    assert_eq!(fixture.request_count("GET", "/api/saves/31/content"), 0);
    assert!(!install_root.join("saves/gb/Game.srm").exists());
    controller.shutdown();
    let settings = SaveSyncSettingsStore::open(settings_dir.join("save-sync.db")).unwrap();
    assert!(!settings.load_files(&scope).unwrap().enabled);
}
