use super::*;

#[test]
fn application_owner_keeps_worker_alive_after_view_handle_drops_and_quit_joins_once() {
    let (sink, _events) = rommfs_core::events::channel();
    let (cmd_tx, commands) = mpsc::channel::<CommandEnvelope>();
    let (facts, observed) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        for envelope in commands {
            let stopping = matches!(envelope.command, Command::Shutdown);
            facts.send(stopping).unwrap();
            if stopping {
                break;
            }
        }
    });
    let application = std::rc::Rc::new(Controller {
        sink,
        cmd_tx,
        worker: std::sync::Mutex::new(Some(worker)),
        save_sync_gate: SaveSyncCommandGate::new(),
        connection_gate: SaveSyncCommandGate::new(),
    });
    let view = application.clone();
    drop(view);
    application.send(Command::StopMount);
    assert!(!observed.recv_timeout(Duration::from_secs(2)).unwrap());
    application.shutdown();
    assert!(observed.recv_timeout(Duration::from_secs(2)).unwrap());
    assert!(application.worker.lock().unwrap().is_none());
    application.shutdown();
}
use std::time::Duration;

/// A minimal RetroBat installation with one system, set up for `root`.
#[cfg(unix)]
fn set_up_retrobat(installation: &std::path::Path, root: &std::path::Path) {
    let home = installation.join("emulationstation/.emulationstation");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(installation.join("roms/gb")).unwrap();
    std::fs::write(installation.join("retrobat.exe"), b"fixture").unwrap();
    std::fs::write(
        installation.join("emulationstation/emulatorLauncher.cfg"),
        "home=.\\.emulationstation\nsaves=..\\saves\n",
    )
    .unwrap();
    std::fs::write(
        home.join("es_systems.cfg"),
        "<systemList><system><name>gb</name><path>~/../roms/gb</path><extension>.gb</extension></system></systemList>",
    )
    .unwrap();
    rommfs_core::retrobat::setup_roms_link(installation, root, "s").unwrap();
}

#[cfg(unix)]
#[test]
fn first_gamelists_publish_while_artwork_is_still_pending() {
    use rommfs_core::retrobat::ArtworkBatch;
    use rommfs_fixture::contract;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    let installation = tmp.path().join("retrobat");
    std::fs::create_dir(&root).unwrap();
    rommfs_cfapi::claim_mount_root(&root, "s").unwrap();
    set_up_retrobat(&installation, &root);
    let gamelist = installation.join("roms/gb/gamelist.xml");
    // RetroBat's own entries stay; RomM games are merged in alongside them.
    std::fs::write(
        &gamelist,
        "<?xml version=\"1.0\"?>\n<gameList><game><path>./Local.gb</path><favorite>true</favorite></game></gameList>",
    )
    .unwrap();
    let platforms: Vec<PlatformDto> =
        serde_json::from_str(&contract::platforms(&[(1, "gb", "gb", "Game Boy")])).unwrap();
    let roms = vec![serde_json::from_value(contract::rom(1, "gb", "Game.gb", 3, "hash")).unwrap()];
    let catalogue =
        rommfs_core::catalogue::build_catalogue("s", &platforms, &roms, |_| {}).unwrap();
    let (sink, events) = rommfs_core::events::channel();
    let mut worker = Worker::new(sink);
    worker.server_id = Some("s".into());
    worker.preferences = Some(ConnectionPreferences {
        server: "s".into(),
        account_id: 7,
        device_identifier: "id".into(),
        mount_root: Some(root.clone()),
        integration: IntegrationMode::RetroBat,
        retrobat_root: Some(installation.clone()),
        start_at_login: false,
        save_root: None,
    });
    worker.mount = Some(ActiveMount {
        root: root.clone(),
        stop_fn: Box::new(|| {}),
    });
    worker.metadata_source = Some((
        catalogue,
        CatalogueSnapshot {
            platforms,
            roms,
            refreshed_at: 0,
        },
    ));
    let (tx, rx) = mpsc::channel();
    worker.metadata_job = Some(rx);
    worker.metadata_ready = Some(ArtworkBatch::default());
    worker.poll_metadata();
    let published = std::fs::read_to_string(&gamelist).unwrap();
    assert!(
        published.contains("<path>./RomM/Game.gb</path>"),
        "{published}"
    );
    assert!(published.contains("<path>./Local.gb</path><favorite>true</favorite>"));
    assert!(worker.metadata_job.is_some());
    assert!(worker.metadata_ready.is_none());
    assert!(events.try_iter().any(|event| matches!(event,
        AppEvent::Log(line) if line.message == "Gamelists published; caching frontend artwork")));

    tx.send(Ok(ArtworkBatch::default())).unwrap();
    worker.poll_metadata();
    assert!(worker.metadata_job.is_none());
    assert!(worker.metadata_source.is_none());
    assert!(events.try_iter().any(|event| matches!(
        event,
        AppEvent::FrontendMetadata {
            pending: false,
            error: None,
            ..
        }
    )));

    // A prior publication can leave its index behind without any gamelist.
    std::fs::remove_file(&gamelist).unwrap();
    assert!(root.join(".rommfs-metadata/index.json").is_file());
    let platforms =
        serde_json::from_str(&contract::platforms(&[(1, "gb", "gb", "Game Boy")])).unwrap();
    let roms = vec![serde_json::from_value(contract::rom(1, "gb", "Game.gb", 3, "hash")).unwrap()];
    let snapshot = CatalogueSnapshot {
        platforms,
        roms,
        refreshed_at: 0,
    };
    worker.catalogue = Some(
        rommfs_core::catalogue::build_catalogue("s", &snapshot.platforms, &snapshot.roms, |_| {})
            .unwrap(),
    );
    worker.source_snapshot = Some(snapshot);
    worker.rom_cache_directory = Some(tmp.path().join("rom-cache"));
    let client = Arc::new(RommClient::new("http://unused.invalid").unwrap());
    worker.fs = Some(
        worker
            .build_fs(&client, &root, Arc::new(NoopHydratedRemover))
            .unwrap(),
    );
    worker.client = Some(client);
    worker.queue_metadata();
    assert!(worker.metadata_ready.is_some());
    worker.poll_metadata();
    assert!(std::fs::read_to_string(&gamelist)
        .unwrap()
        .contains("./RomM/Game.gb"));
    worker.cancel_metadata();
}

#[test]
fn stopping_cancels_artwork_and_discards_late_metadata_results() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let (sink, events) = rommfs_core::events::channel();
    let mut worker = Worker::new(sink);
    let active = Arc::new(AtomicBool::new(true));
    let (tx, rx) = mpsc::channel();
    worker.metadata_active = Some(active.clone());
    worker.metadata_job = Some(rx);
    worker.stop_mount();
    assert!(!active.load(Ordering::Acquire));
    assert!(tx
        .send(Err(Error::Transport("old session failure".into())))
        .is_err());
    worker.poll_metadata();
    let mut state = UiState::new(32);
    while let Ok(event) = events.try_recv() {
        state.apply_event(&event);
    }
    assert!(!state.frontend_metadata_pending);
    assert!(state.frontend_metadata_error.is_none());
}

#[test]
fn changing_accounts_drains_catalogue_and_artwork_threads_before_returning() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let (sink, _events) = rommfs_core::events::channel();
    let mut worker = Worker::new(sink);
    let completed = Arc::new(AtomicUsize::new(0));
    for artwork in [true, false] {
        let active = Arc::new(AtomicBool::new(true));
        let flag = active.clone();
        let done = completed.clone();
        let handle = std::thread::spawn(move || {
            while flag.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            done.fetch_add(1, Ordering::SeqCst);
        });
        if artwork {
            worker.metadata_active = Some(active);
            worker.metadata_handle = Some(handle);
        } else {
            worker.refresh_active = Some(active);
            worker.refresh_handle = Some(handle);
        }
    }
    worker.clear_active_connection();
    assert_eq!(completed.load(Ordering::SeqCst), 2);
    assert!(worker.metadata_handle.is_none());
    assert!(worker.refresh_handle.is_none());
}

#[test]
fn failed_reconnect_discards_previous_worker_session() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = [0; 2048];
        let _ = std::io::Read::read(&mut stream, &mut request).unwrap();
        stream
            .write_all(
                b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
    });
    let (sink, events) = rommfs_core::events::channel();
    let mut worker = Worker::new(sink);
    worker.client = Some(Arc::new(
        RommClient::new("http://previous.invalid").unwrap(),
    ));
    worker.server_id = Some("http://previous.invalid".into());
    worker.names.insert(
        RomKey {
            server_id: "http://previous.invalid".into(),
            rom_id: 1,
            file_id: 2,
        },
        "old.nes".into(),
    );

    worker.connect(&format!("http://{address}"), "user", "password");
    server.join().unwrap();

    assert!(worker.client.is_none());
    assert!(worker.server_id.is_none());
    assert!(worker.catalogue.is_none());
    assert!(worker.names.is_empty());
    assert!(matches!(
        events.try_recv(),
        Ok(AppEvent::SaveSyncSessionChanged { session_id: 1 })
    ));
    assert!(matches!(events.try_recv(), Ok(AppEvent::Connecting)));
    assert!(matches!(
        events.try_recv(),
        Ok(AppEvent::SignInRequired { .. })
    ));
}

#[test]
fn browser_pairing_cancelled_during_account_verification_never_connects_or_remembers() {
    let fixture = FixtureServer::start();
    fixture.on("POST", "/api/auth/device/init", ResponseSpec::Json { status: 201, body: serde_json::json!({
        "device_code":"private-code", "user_code":"ABCD", "verification_path":"/pair/device",
        "verification_path_complete":"/pair/device?user_code=ABCD", "expires_in":600, "interval":1
    }).to_string() });
    fixture.on("POST", "/api/auth/device/token", ResponseSpec::Json { status: 200, body: serde_json::json!({
        "access_token":"secret-token", "device_id":"device", "scopes":["me.read"], "expires_at":null
    }).to_string() });
    let barrier = FixtureBodyBarrier::new();
    fixture.on(
        "GET",
        "/api/users/me",
        ResponseSpec::HeldBytes {
            status: 200,
            bytes: br#"{"id":42,"oauth_scopes":["me.read"]}"#.to_vec(),
            barrier: barrier.clone(),
        },
    );
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("connection.db");
    let gate = SaveSyncCommandGate::new();
    let worker_gate = gate.clone();
    let worker_path = settings_path.clone();
    let url = fixture.url().to_owned();
    let (sink, events) = rommfs_core::events::channel();
    let worker = std::thread::spawn(move || {
        let mut worker = Worker::new(sink);
        worker.connection_gate = worker_gate;
        worker.connection_settings = Some(ConnectionSettings::open(&worker_path).unwrap());
        worker.begin_pairing(&url);
        worker.pairing.as_mut().unwrap().next_poll = Instant::now();
        worker.poll_pairing();
        assert!(worker.client.is_none());
        assert!(worker.preferences.is_none());
        assert!(worker
            .connection_settings
            .as_ref()
            .unwrap()
            .load()
            .unwrap()
            .is_none());
        worker.cancel_pairing();
    });
    assert!(barrier.wait_until_blocked(Duration::from_secs(5)));
    gate.invalidate();
    barrier.release();
    worker.join().unwrap();
    let events: Vec<_> = events.try_iter().collect();
    assert!(events.iter().any(
        |e| matches!(e, AppEvent::PairingEnded { reason: Some(r) } if r.contains("cancelled"))
    ));
    assert!(!events.iter().any(|e| matches!(
        e,
        AppEvent::Connected | AppEvent::ConnectFailed { .. } | AppEvent::CatalogueLoaded { .. }
    )));
    assert_eq!(fixture.request_count("GET", "/api/platforms"), 0);
    assert!(!format!("{events:?}").contains("secret-token"));
}

#[test]
fn save_sync_changes_do_not_cancel_browser_pairing_and_denial_is_visible() {
    let fixture = FixtureServer::start();
    fixture.on("POST", "/api/auth/device/init", ResponseSpec::Json { status:201, body:serde_json::json!({
        "device_code":"private-code", "user_code":"ABCD", "verification_path":"/pair/device",
        "verification_path_complete":"/pair/device?user_code=ABCD", "expires_in":600, "interval":1
    }).to_string() });
    fixture.on(
        "POST",
        "/api/auth/device/token",
        ResponseSpec::Json {
            status: 400,
            body: r#"{"detail":"access_denied"}"#.into(),
        },
    );
    let (sink, events) = rommfs_core::events::channel();
    let mut worker = Worker::new(sink);
    worker.begin_pairing(fixture.url());
    worker.save_sync_gate.invalidate();
    worker.pairing.as_mut().unwrap().next_poll = Instant::now();
    worker.poll_pairing();
    assert_eq!(fixture.request_count("POST", "/api/auth/device/token"), 1);
    assert!(worker.client.is_none());
    assert!(worker.pairing.is_none());
    assert!(events
        .try_iter()
        .any(|e| matches!(e, AppEvent::PairingEnded { reason:Some(r) } if r.contains("denied"))));
}

#[test]
fn remembered_preferences_never_cross_accounts_or_servers() {
    let directory = tempfile::tempdir().unwrap();
    let (sink, _) = rommfs_core::events::channel();
    let mut worker = Worker::new(sink);
    worker.connection_settings =
        Some(ConnectionSettings::open(&directory.path().join("connection.db")).unwrap());
    let preferences = ConnectionPreferences {
        server: "https://romm.example".into(),
        account_id: 42,
        device_identifier: "id".into(),
        mount_root: Some(directory.path().join("roms")),
        integration: IntegrationMode::Generic,
        retrobat_root: None,
        start_at_login: false,
        save_root: None,
    };
    worker
        .connection_settings
        .as_ref()
        .unwrap()
        .save(&preferences)
        .unwrap();
    worker.preferences = Some(preferences.clone());
    worker.forget_other_account("https://romm.example/", Some(42));
    assert!(worker.preferences.is_some());
    worker.forget_other_account("https://romm.example", Some(43));
    assert!(worker.preferences.is_none());
    assert!(worker
        .connection_settings
        .as_ref()
        .unwrap()
        .load()
        .unwrap()
        .is_none());
    worker.preferences = Some(preferences);
    worker.forget_other_account("https://another.example", Some(42));
    assert!(worker.preferences.is_none());
}

#[test]
fn restore_keeps_the_configured_folder_visible() {
    let directory = tempfile::tempdir().unwrap();
    let (sink, events) = rommfs_core::events::channel();
    let mut worker = Worker::new(sink);
    worker.connection_settings =
        Some(ConnectionSettings::open(&directory.path().join("connection.db")).unwrap());
    worker
        .connection_settings
        .as_ref()
        .unwrap()
        .save(&ConnectionPreferences {
            server: "https://romm.example".into(),
            account_id: 42,
            device_identifier: "id".into(),
            mount_root: Some(directory.path().join("roms")),
            integration: IntegrationMode::Generic,
            retrobat_root: None,
            start_at_login: false,
            save_root: None,
        })
        .unwrap();

    worker.restore_connection();

    let collected: Vec<_> = events.try_iter().collect();
    let connecting_at = collected
        .iter()
        .position(|e| matches!(e, AppEvent::Connecting))
        .expect("restore emits Connecting");
    let restored_at = collected
        .iter()
        .position(|e| matches!(e, AppEvent::ConnectionRestored { .. }))
        .expect("restore emits ConnectionRestored");
    assert!(
        connecting_at < restored_at,
        "Connecting must precede ConnectionRestored — it clears the restored config"
    );
    let mut state = UiState::new(16);
    for event in &collected {
        state.apply_event(event);
    }
    assert_eq!(
        state.folder_summary(),
        FolderSummary::Stopped,
        "a configured-but-stopped folder must never read as unconfigured"
    );
}

#[test]
fn reauthentication_republishes_the_remembered_folder() {
    use rommfs_fixture::contract;
    let directory = tempfile::tempdir().unwrap();
    let fx = FixtureServer::start();
    fx.on(
        "POST",
        "/api/token",
        ResponseSpec::Json {
            status: 200,
            body: contract::token_ok(),
        },
    );
    fx.on(
        "GET",
        "/api/users/me",
        ResponseSpec::Json {
            status: 200,
            body:
                serde_json::json!({"id":42,"oauth_scopes":["me.read","assets.read","assets.write"]})
                    .to_string(),
        },
    );
    let mount_root = directory.path().join("roms");
    let (sink, events) = rommfs_core::events::channel();
    let mut worker = Worker::new(sink);
    worker.connection_settings =
        Some(ConnectionSettings::open(&directory.path().join("connection.db")).unwrap());
    worker
        .connection_settings
        .as_ref()
        .unwrap()
        .save(&ConnectionPreferences {
            server: fx.url().into(),
            account_id: 42,
            device_identifier: "id".into(),
            mount_root: Some(mount_root.clone()),
            integration: IntegrationMode::Generic,
            retrobat_root: None,
            start_at_login: false,
            save_root: None,
        })
        .unwrap();

    // Restore publishes the stored folder, then fails authentication
    // (no credential in the store for this fixture server).
    worker.restore_connection();
    // The user signs in again; the Connecting event clears the folder
    // facts, and accepting the session must restore them.
    worker.connect(fx.url(), "user", "password");

    let mut state = UiState::new(16);
    for event in events.try_iter() {
        state.apply_event(&event);
    }
    assert_eq!(
        state.configured_mount_root.as_deref(),
        Some(mount_root.display().to_string().as_str()),
        "reauthentication must restore the remembered folder to the UI"
    );
    assert_eq!(state.folder_summary(), FolderSummary::Stopped);
}

#[test]
fn a_metadata_failure_is_logged_once_with_its_error() {
    let (sink, events) = rommfs_core::events::channel();
    let worker = Worker::new(sink);
    worker.metadata_status(
        false,
        "Frontend metadata was not published",
        Some("boom".into()),
    );
    let logs: Vec<_> = events
        .try_iter()
        .filter_map(|event| match event {
            AppEvent::Log(line) => Some(line),
            _ => None,
        })
        .collect();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].level, Level::Error);
    assert!(logs[0].message.contains("boom"));
}
