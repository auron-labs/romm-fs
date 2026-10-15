use super::*;

#[test]
fn mount_root_errors_preserve_io_kind_context_and_permission_guidance() {
    for kind in [
        std::io::ErrorKind::PermissionDenied,
        std::io::ErrorKind::NotFound,
    ] {
        let error = mount_root_error(
            anyhow::Error::new(std::io::Error::new(kind, "filesystem failure"))
                .context("create ownership manifest C:\\RomM.rommfs-root"),
        );
        let Error::Io(io) = error else {
            panic!("filesystem failures must remain I/O errors");
        };
        assert_eq!(io.kind(), kind);
        let message = io.to_string();
        assert!(message.contains("C:\\RomM.rommfs-root: filesystem failure"));
        assert_eq!(
            message.contains("%USERPROFILE%\\RomM"),
            kind == std::io::ErrorKind::PermissionDenied
        );
    }
    assert!(matches!(
        mount_root_error(anyhow::anyhow!("root is not empty")),
        Error::Unsupported(_)
    ));
}

#[test]
fn mount_root_is_created_and_existing_user_files_are_preserved() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    check_mount_root(&root, "server").unwrap();
    assert!(root.is_dir());
    check_mount_root(&root, "server").unwrap();
    std::fs::write(root.join("save.srm"), b"user data").unwrap();
    assert!(check_mount_root(&root, "server").is_err());
    assert_eq!(std::fs::read(root.join("save.srm")).unwrap(), b"user data");
}

#[test]
fn foreign_entries_populate_remediation_state_and_clear_on_retry() {
    let mut state = UiState::new(16);
    state.apply_event(&AppEvent::ForeignRootEntries {
        root: "C:\\RomM".into(),
        entries: vec![
            ForeignEntry {
                path: "gb/gamelist.xml".into(),
                kind: ForeignKind::File,
            },
            ForeignEntry {
                path: "gb".into(),
                kind: ForeignKind::Link,
            },
        ],
        removal: None,
    });
    assert_eq!(state.foreign_root.as_deref(), Some("C:\\RomM"));
    assert_eq!(state.foreign_entries.len(), 2);
    assert!(state.foreign_removal.is_none());
    state.apply_event(&AppEvent::ForeignRootEntries {
        root: "C:\\RomM".into(),
        entries: vec![ForeignEntry {
            path: "gb/gamelist.xml".into(),
            kind: ForeignKind::File,
        }],
        removal: Some(Ok(1)),
    });
    assert_eq!(state.foreign_removal, Some(Ok(1)));
    assert_eq!(state.foreign_entries.len(), 1);
    state.apply_event(&AppEvent::MountStarting {
        path: "C:\\RomM".into(),
    });
    assert!(state.foreign_entries.is_empty());
    assert!(state.foreign_root.is_none());
    assert!(state.foreign_removal.is_none());
}

#[cfg(unix)]
#[test]
fn remove_foreign_links_reports_and_preserves_real_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("mount");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("user.srm"), b"keep").unwrap();
    std::os::unix::fs::symlink(&root, root.join("leftover")).unwrap();
    let (sink, events) = rommfs_core::events::channel();
    let mut worker = Worker::new(sink);
    worker.remove_foreign_links(&root);
    assert_eq!(std::fs::read(root.join("user.srm")).unwrap(), b"keep");
    assert!(std::fs::symlink_metadata(root.join("leftover")).is_err());
    let events: Vec<_> = events.try_iter().collect();
    let report = events
        .iter()
        .find_map(|e| match e {
            AppEvent::ForeignRootEntries {
                entries, removal, ..
            } => Some((entries, removal)),
            _ => None,
        })
        .expect("cleanup must republish the foreign entry list");
    assert_eq!(report.1, &Some(Ok(1)));
    assert_eq!(report.0.len(), 1);
    assert_eq!(report.0[0].kind, ForeignKind::File);
}

#[test]
fn failed_mount_logs_its_stage_and_path_before_and_after_validation() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("save.srm"), b"user data").unwrap();
    let (sink, events) = rommfs_core::events::channel();
    let mut worker = Worker::new(sink);
    worker.client = Some(Arc::new(RommClient::new("http://unused.invalid").unwrap()));
    worker.server_id = Some("server".into());
    worker.start_mount(root.to_str().unwrap());
    let events: Vec<_> = events.try_iter().collect();
    let checking = events
        .iter()
        .position(|event| {
            matches!(event,
        AppEvent::Log(line) if line.message.contains("checking and preparing")
            && line.message.contains("roms"))
        })
        .unwrap();
    let failed = events
        .iter()
        .position(|event| {
            matches!(event,
        AppEvent::MountFailed { reason } if reason.contains("prepare and validate directory")
            && reason.contains("roms") && reason.contains("not empty"))
        })
        .unwrap();
    assert!(checking < failed);
    assert!(events.iter().any(|event| matches!(event,
        AppEvent::Log(line) if line.level == Level::Error
            && line.message.contains("prepare and validate directory")
            && line.message.contains("not empty"))));
    assert_eq!(std::fs::read(root.join("save.srm")).unwrap(), b"user data");
}

#[test]
fn connect_clears_a_stale_mount_failure() {
    let mut state = UiState::new(32);
    state.apply_event(&AppEvent::MountFailed {
        reason: "connect to a RomM server before mounting".into(),
    });
    assert_eq!(state.mount, MountState::Failed);
    assert!(state.mount_error.is_some());

    state.apply_event(&AppEvent::Connected);
    assert_eq!(state.mount, MountState::NotMounted);
    assert!(state.mount_error.is_none());
}

#[cfg(unix)]
#[test]
fn mount_root_creation_rejects_linked_ancestors() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    let link = tmp.path().join("link");
    std::fs::create_dir(&target).unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(check_mount_root(&link.join("roms"), "server").is_err());
    assert!(!target.join("roms").exists());
}

#[cfg(windows)]
#[test]
fn mount_root_creation_rejects_junction_ancestors() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    let link = tmp.path().join("link");
    std::fs::create_dir(&target).unwrap();
    let output = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .output()
        .expect("mklink junction");
    assert!(
        output.status.success(),
        "junction creation failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(check_mount_root(&link.join("roms"), "server").is_err());
    assert!(!target.join("roms").exists());
}

#[test]
fn duplicate_mount_start_preserves_the_active_mount() {
    let (sink, events) = rommfs_core::events::channel();
    let mut worker = Worker::new(sink);
    worker.mount = Some(ActiveMount {
        root: PathBuf::from("active-root"),
        stop_fn: Box::new(|| {}),
    });

    worker.start_mount("another-root");

    assert_eq!(
        worker.mount.as_ref().unwrap().root,
        PathBuf::from("active-root")
    );
    assert!(
        events.try_recv().is_err(),
        "duplicate start must not emit failure"
    );
}

#[test]
fn worker_loop_sweeps_again_after_the_mount_start_sweep() {
    let dir = tempfile::tempdir().unwrap();
    let key = RomKey {
        server_id: "fixture".into(),
        rom_id: 12,
        file_id: 34,
    };
    let cache_dir = dir.path().to_path_buf();
    let bin = cache_dir.join(format!("{}.bin", key.cache_stem()));
    let worker_bin = bin.clone();
    let (sink, events) = rommfs_core::events::channel();
    let (cmd_tx, cmd_rx) = mpsc::channel::<CommandEnvelope>();
    let worker_thread = std::thread::spawn(move || {
        // Construct Worker inside its owning thread: ActiveMount may
        // contain a platform mount handle that is intentionally !Send.
        let mut index = CacheIndex::open(&cache_dir).unwrap();
        std::fs::write(&worker_bin, b"cached bytes").unwrap();
        index.mark_ready(&key, 12, None, "nes/game.nes").unwrap();
        index.touch(&key, 0).unwrap();

        let live = Arc::new(LiveState::default());
        let downloads = Arc::new(DownloadManager::new(
            index,
            Arc::clone(&live),
            Arc::new(UnusedSource),
            rommfs_core::events::channel().0,
            HashMap::new(),
            HashMap::new(),
        ));
        let catalogue = build_catalogue(
            "fixture",
            &[PlatformDto {
                id: 1,
                slug: "nes".into(),
                fs_slug: "nes".into(),
                name: "Nintendo".into(),
                custom_name: None,
                rom_count: 0,
            }],
            &[],
            |_| {},
        )
        .unwrap();
        let fs = Arc::new(RommFs::new(
            RommTree::new(catalogue),
            downloads,
            Evictor::new(1, live, Arc::new(NoopHydratedRemover)),
            Arc::new(FakeClock::new(100)),
        ));
        let worker = Worker {
            auxiliary_job: None,
            startup_restoring: false,
            startup_requires_mount: false,
            save_file_rules: None,
            installer: installer::InstallerJob::default(),
            metadata_job: None,
            metadata_handle: None,
            metadata_job_id: 0,
            metadata_active: None,
            metadata_source: None,
            metadata_ready: None,
            metadata_wait_reported: false,
            metadata_publish: None,
            metadata_publish_handle: None,
            draining: Vec::new(),
            rom_cache_directory: None,
            sink,
            client: None,
            server_id: None,
            catalogue: None,
            connection_settings: None,
            preferences: None,
            pairing: None,
            catalogue_store: None,
            catalogue_account: None,
            source_snapshot: None,
            refresh_result: None,
            refresh_handle: None,
            refresh_active: None,
            catalogue_operation_id: 0,
            names: HashMap::from([(key, "game.nes".into())]),
            fs: Some(fs),
            mount: Some(ActiveMount {
                root: cache_dir,
                stop_fn: Box::new(|| {}),
            }),
            save_sync_settings: None,
            save_sync_storage_dir: None,
            save_sync_selected_root: None,
            save_sync_settings_problem: None,
            save_sync_enabled: false,
            save_sync_debounce_secs: rommfs_core::save_sync::DEFAULT_DEBOUNCE_SECS,
            save_sync_identity: None,
            save_sync_scope: None,
            save_sync_profile: None,
            save_sync_mapping_report: None,
            save_sync_catalogue: None,
            save_sync_agent: None,
            save_sync_gate: SaveSyncCommandGate::new(),
            connection_gate: SaveSyncCommandGate::new(),
            current_connection_epoch: 0,
            current_save_sync_epoch: 0,
            save_sync_runtime_problem: None,
            save_sync_session_id: 0,
            play_data_enabled: false,
            play_data_available: false,
            play_data_problem: None,
            hide_copies_count: 0,
            play_watch: None,
            play_watch_checked: Instant::now(),
            next_catalogue_poll: None,
        };
        worker_loop_with_interval_from(cmd_rx, worker, Duration::from_millis(10));
    });

    let event = events.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(event, AppEvent::Evicted { rom_id: 12, .. }));
    assert!(
        !bin.exists(),
        "the periodic sweep removes stale cache bytes"
    );

    cmd_tx
        .send(CommandEnvelope {
            command: Command::Shutdown,
            save_sync_epoch: 0,
            connection_epoch: 0,
        })
        .unwrap();
    worker_thread.join().unwrap();
}

#[test]
fn mount_path_is_scoped_to_its_session() {
    let mut state = UiState::new(16);
    for event in [
        AppEvent::ConnectionRestored {
            server: "https://a.example".into(),
            mount_root: Some("/srv/a".into()),
        },
        AppEvent::MountStarted {
            path: "/srv/a".into(),
        },
        AppEvent::MountStopped,
    ] {
        state.apply_event(&event);
    }
    assert_eq!(state.folder_summary(), FolderSummary::Stopped);

    state.apply_event(&AppEvent::SignedOut);

    assert!(state.mount_path.is_none());
    assert_eq!(
        state.folder_summary(),
        FolderSummary::SetupNeeded,
        "server A's mount path must not become the next session's folder"
    );

    // A path stashed by a mount attempt dies with that session too.
    state.mount_path = Some("/srv/a".into());
    state.apply_event(&AppEvent::Connecting);
    assert!(state.mount_path.is_none());
    assert_eq!(state.folder_summary(), FolderSummary::SetupNeeded);
}

#[test]
fn games_folder_apply_updates_the_session_config() {
    let directory = tempfile::tempdir().unwrap();
    let (sink, events) = rommfs_core::events::channel();
    let mut worker = Worker::new(sink);
    worker.connection_settings =
        Some(ConnectionSettings::open(&directory.path().join("connection.db")).unwrap());
    worker.preferences = Some(ConnectionPreferences {
        server: "https://romm.example".into(),
        account_id: 42,
        device_identifier: "id".into(),
        mount_root: Some(directory.path().join("old")),
        integration: IntegrationMode::Generic,
        retrobat_root: None,
        start_at_login: false,
        save_root: None,
        hide_owned_copies: true,
    });

    worker.set_mount_root("  /new/roms  ");

    assert_eq!(
        worker.preferences.as_ref().unwrap().mount_root,
        Some(PathBuf::from("/new/roms")),
        "the edit is stored as the session's mount root"
    );
    assert_eq!(
        worker
            .connection_settings
            .as_ref()
            .unwrap()
            .load()
            .unwrap()
            .unwrap()
            .mount_root,
        Some(PathBuf::from("/new/roms")),
        "the applied folder is persisted for the next launch"
    );
    let mut state = UiState::new(16);
    for event in events.try_iter() {
        state.apply_event(&event);
    }
    assert_eq!(state.configured_mount_root.as_deref(), Some("/new/roms"));
    assert_eq!(
        state.mount_path.as_deref(),
        Some("/new/roms"),
        "a stopped folder's Start/Retry path must follow the applied edit"
    );
    assert_eq!(state.folder_summary(), FolderSummary::Stopped);

    // A live mount keeps its real path — the applied folder is the
    // next start's target.
    state.apply_event(&AppEvent::MountStarted {
        path: "/live".into(),
    });
    state.apply_event(&AppEvent::MountRootConfigured {
        server: "s".into(),
        mount_root: "/newer".into(),
    });
    assert_eq!(state.mount_path.as_deref(), Some("/live"));
    assert_eq!(state.configured_mount_root.as_deref(), Some("/newer"));
}
