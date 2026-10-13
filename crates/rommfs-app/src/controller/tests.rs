//! Controller behavior tests grouped by lifecycle and workflow.
use super::*;
use rommfs_core::cache::FakeClock;
use rommfs_core::romm::{PlatformDto, RomDto, RomFileDto, SaveSyncIdentity};
use rommfs_core::save_sync::RETROBAT_GB_SRM_PROFILE;
use rommfs_fixture::{FixtureBodyBarrier, FixtureSaveRecord, FixtureServer, ResponseSpec};
use std::io::Write;

struct UnusedSource;

impl ContentSource for UnusedSource {
    fn fetch(
        &self,
        _key: &RomKey,
        _writer: &mut dyn Write,
        _progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<u64> {
        unreachable!("the eviction test never reads ROM content")
    }
}

fn create_test_retrobat_install(root: &Path, create_saves: bool) {
    let launcher_home = root.join("emulationstation/.emulationstation");
    std::fs::create_dir_all(root.join("system")).unwrap();
    std::fs::create_dir_all(root.join("emulators/retroarch")).unwrap();
    std::fs::create_dir_all(&launcher_home).unwrap();
    if create_saves {
        std::fs::create_dir_all(root.join("saves")).unwrap();
    }
    std::fs::write(root.join("RetroBat.exe"), b"exe").unwrap();
    std::fs::write(root.join("system/version.info"), "8.2.1\n").unwrap();
    std::fs::write(
            root.join("emulators/retroarch/retroarch.cfg"),
            "savefile_directory = \":\\saves\"\nsavefiles_in_content_dir = \"false\"\nsort_savefiles_enable = \"false\"\n",
        )
        .unwrap();
    std::fs::write(
        root.join("emulationstation/emulatorLauncher.cfg"),
        "home=.\\.emulationstation\nsaves=.\\..\\saves\n",
    )
    .unwrap();
    std::fs::write(launcher_home.join("es_settings.cfg"), "<config/>\n").unwrap();
    std::fs::write(
            launcher_home.join("es_systems.cfg"),
            r#"<systemList><system><name>gb</name><command>"%HOME%\emulatorLauncher.exe" -gameinfo %GAMEINFOXML% %CONTROLLERSCONFIG% -system %SYSTEM% -emulator %EMULATOR% -core %CORE% -rom %ROM%</command><emulators><emulator name="libretro"><cores><core>gambatte</core></cores></emulator></emulators></system></systemList>"#,
        )
        .unwrap();
}

mod catalogue {
    use super::*;

    #[test]
    fn catalogue_warnings_collapse_to_one_counted_line_per_reason() {
        let mut warnings: Vec<String> = (1..=8)
            .map(|n| format!("rom {n} file {}: original file retained but its extension is not launchable by this frontend", n * 10))
            .collect();
        warnings.push("rom 3: no original file records".into());
        warnings.push("rom 4: no original file records".into());
        warnings.push("rom 1 file 10: visible path adjusted to \"a (1).zip\"".into());
        warnings.push("rom 2 file 20: visible path adjusted to \"b (1).zip\"".into());
        warnings.push("platform x: odd".into());
        let lines = super::summarize_catalogue_warnings(&warnings);
        assert_eq!(lines.len(), 4, "{lines:?}");
        assert!(lines.contains(&"8 files: original file retained but its extension is not launchable by this frontend (e.g. rom 1 file 10, rom 2 file 20, rom 3 file 30, rom 4 file 40, rom 5 file 50)".to_string()));
        assert!(lines.contains(&"2 roms: no original file records (e.g. rom 3, rom 4)".to_string()));
        assert!(lines.contains(
            &"2 files: visible path adjusted (e.g. rom 1 file 10, rom 2 file 20)".to_string()
        ));
        assert!(lines.contains(&"platform x: odd".to_string()));
    }

    #[test]
    fn catalogue_pages_are_fetched_once_and_warm_remounts_make_no_http_requests() {
        use rommfs_fixture::contract;
        let tmp = tempfile::tempdir().unwrap();
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
                serde_json::json!({"id":7,"oauth_scopes":["me.read","assets.read","assets.write"]})
                    .to_string(),
        },
    );
        let slugs: Vec<_> = (0..25).map(|n| format!("platform{n}")).collect();
        let platforms: Vec<_> = slugs
            .iter()
            .enumerate()
            .map(|(n, slug)| (n as i64, slug.as_str(), slug.as_str(), slug.as_str()))
            .collect();
        fx.on(
            "GET",
            "/api/platforms",
            ResponseSpec::Json {
                status: 200,
                body: contract::platforms(&platforms),
            },
        );
        let items: Vec<_> = (0..2448)
            .map(|n| {
                contract::rom(
                    n + 1,
                    &slugs[n as usize % 25],
                    &format!("game{n}.bin"),
                    10,
                    "hash",
                )
            })
            .collect();
        for offset in (0..2448).step_by(200) {
            fx.on(
                "GET",
                &format!("/api/roms?limit=200&offset={offset}&"),
                ResponseSpec::Json {
                    status: 200,
                    body: contract::roms_page(
                        &items[offset..(offset + 200).min(items.len())],
                        items.len(),
                        200,
                        offset,
                    ),
                },
            );
        }
        let (sink, events) = rommfs_core::events::channel();
        let mut worker = Worker::new(sink);
        worker.rom_cache_directory = Some(tmp.path().join("rom-cache"));
        worker.catalogue_store =
            Some(CatalogueStore::open(&tmp.path().join("catalogue.db")).unwrap());
        worker.connect(fx.url(), "user", "password");
        assert_eq!(worker.catalogue.as_ref().unwrap().entries.len(), 2448);
        assert_eq!(fx.count("GET", "/api/platforms"), 1);
        for offset in (0..2448).step_by(200) {
            assert_eq!(
                fx.count("GET", &format!("/api/roms?limit=200&offset={offset}&")),
                1
            );
        }
        let root = tmp.path().join("roms");
        for _ in 0..2 {
            worker.start_mount(root.to_str().unwrap());
            worker.stop_mount();
        }
        assert_eq!(fx.count("GET", "/api/platforms"), 1);
        assert_eq!(fx.count("GET", "/api/roms/"), 0);
        assert!(
            worker.catalogue.is_some(),
            "mount construction retains the reusable snapshot"
        );
        let snapshot = worker.source_snapshot.clone().unwrap();
        worker.catalogue = None;
        assert!(worker.restore_catalogue());
        assert_eq!(
            worker.source_snapshot.as_ref().unwrap().refreshed_at,
            snapshot.refreshed_at
        );
        // A failed later page retains the whole preceding complete snapshot on
        // disk and in memory, rather than publishing the first successful page.
        fx.on(
            "GET",
            "/api/roms?limit=200&offset=200&",
            ResponseSpec::Json {
                status: 500,
                body: "{}".into(),
            },
        );
        worker.refresh_catalogue();
        let result = worker
            .refresh_result
            .take()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        worker.finish_catalogue_refresh(result);
        assert_eq!(worker.catalogue.as_ref().unwrap().entries.len(), 2448);
        assert_eq!(
            worker
                .catalogue_store
                .as_ref()
                .unwrap()
                .load(worker.server_id.as_deref().unwrap(), "7")
                .unwrap()
                .unwrap()
                .roms
                .len(),
            2448
        );
        assert!(events
            .try_iter()
            .any(|e| matches!(e,AppEvent::CatalogueFailed{reason} if reason.contains("500"))));
    }

    #[test]
    fn invalid_layout_selection_keeps_the_previous_projection_and_preferences() {
        let directory = tempfile::tempdir().unwrap();
        let (sink, events) = rommfs_core::events::channel();
        let mut worker = Worker::new(sink);
        worker.preferences = Some(ConnectionPreferences {
            server: "s".into(),
            account_id: 42,
            device_identifier: "id".into(),
            mount_root: None,
            integration: IntegrationMode::Generic,
            retrobat_root: None,
            start_at_login: false,
            save_root: None,
        });
        worker.set_integration(IntegrationMode::RetroBat, Some(directory.path().into()));
        assert_eq!(
            worker.preferences.as_ref().unwrap().integration,
            IntegrationMode::Generic
        );
        assert!(events.try_iter().any(|e| matches!(
            e,
            AppEvent::IntegrationUpdated {
                mode: IntegrationMode::Generic,
                error: Some(_),
                ..
            }
        )));
    }

    #[test]
    fn cancelled_refresh_cannot_emit_late_events_or_replace_the_new_session() {
        let (sink, events) = rommfs_core::events::channel();
        let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let scoped = sink.clone().scoped(active.clone());
        let mut worker = Worker::new(sink);
        worker.refresh_active = Some(active);
        worker.cancel_catalogue_refresh();
        scoped.emit(AppEvent::CatalogueProgress {
            operation_id: 99,
            stage: "old account".into(),
            completed: 200,
            total: Some(2448),
            elapsed_ms: 100,
        });
        assert!(events.try_recv().is_err());
    }
}

mod connection {
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
            state.apply(&event);
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
        assert_eq!(fixture.count_requests("GET", "/api/platforms"), 0);
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
        assert_eq!(fixture.count_requests("POST", "/api/auth/device/token"), 1);
        assert!(worker.client.is_none());
        assert!(worker.pairing.is_none());
        assert!(events.try_iter().any(
            |e| matches!(e, AppEvent::PairingEnded { reason:Some(r) } if r.contains("denied"))
        ));
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
            state.apply(event);
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
            state.apply(&event);
        }
        assert_eq!(
            state.configured_mount_root.as_deref(),
            Some(mount_root.display().to_string().as_str()),
            "reauthentication must restore the remembered folder to the UI"
        );
        assert_eq!(state.folder_summary(), FolderSummary::Stopped);
    }
}

mod mount {
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
        state.apply(&AppEvent::ForeignRootEntries {
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
        state.apply(&AppEvent::ForeignRootEntries {
            root: "C:\\RomM".into(),
            entries: vec![ForeignEntry {
                path: "gb/gamelist.xml".into(),
                kind: ForeignKind::File,
            }],
            removal: Some(Ok(1)),
        });
        assert_eq!(state.foreign_removal, Some(Ok(1)));
        assert_eq!(state.foreign_entries.len(), 1);
        state.apply(&AppEvent::MountStarting {
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
        state.apply(&AppEvent::MountFailed {
            reason: "connect to a RomM server before mounting".into(),
        });
        assert_eq!(state.mount, MountState::Failed);
        assert!(state.mount_error.is_some());

        state.apply(&AppEvent::Connected);
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
                metadata_active: None,
                metadata_source: None,
                metadata_ready: None,
                metadata_wait_reported: false,
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
            state.apply(&event);
        }
        assert_eq!(state.folder_summary(), FolderSummary::Stopped);

        state.apply(&AppEvent::SignedOut);

        assert!(state.mount_path.is_none());
        assert_eq!(
            state.folder_summary(),
            FolderSummary::SetupNeeded,
            "server A's mount path must not become the next session's folder"
        );

        // A path stashed by a mount attempt dies with that session too.
        state.mount_path = Some("/srv/a".into());
        state.apply(&AppEvent::Connecting);
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
            state.apply(&event);
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
        state.apply(&AppEvent::MountStarted {
            path: "/live".into(),
        });
        state.apply(&AppEvent::MountRootConfigured {
            server: "s".into(),
            mount_root: "/newer".into(),
        });
        assert_eq!(state.mount_path.as_deref(), Some("/live"));
        assert_eq!(state.configured_mount_root.as_deref(), Some("/newer"));
    }
}

mod save_sync {
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
        worker.save_sync_settings_problem =
            Some("Selection was not saved: transient failure".into());
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
        state.apply(&AppEvent::SaveSyncSessionChanged { session_id: 8 });
        state.apply(&AppEvent::SaveSyncAuthenticationRequired { session_id: 7 });
        assert_ne!(state.conn, ConnState::SignInRequired);
        state.apply(&AppEvent::SaveSyncQueueUpdated(SaveSyncQueueStatus {
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
        state.apply(&AppEvent::SaveSyncTransferProgress {
            session_id: 7,
            rom_id: 3,
            revision: "stale-revision".into(),
            phase: "verified".into(),
            detail: None,
        });
        assert!(state.save_sync_queue.is_none());
        assert!(state.save_sync_transfers.is_empty());

        state.apply(&AppEvent::SaveSyncAuthenticationRequired { session_id: 8 });
        assert!(state.save_sync_authentication_required);
        assert_eq!(state.conn, ConnState::SignInRequired);

        state.apply(&AppEvent::SaveSyncQueueUpdated(SaveSyncQueueStatus {
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
                bytes: br#"{"id":42,"oauth_scopes":["me.read","assets.read","assets.write"]}"#
                    .to_vec(),
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
            file_name: "rommfs-550e8400-e29b-41d4-a716-446655440000 [2026-10-05_12-34-56].srm"
                .into(),
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
        if inventory_started {
            inventory_barrier.release();
        }
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
                assert!(available, "test account/profile was not save-sync ready");
                assert_eq!(mapped_targets, 1);
                assert_eq!(account_id, Some(42));
                saw_disabled_status = true;
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
        assert_eq!(fixture.count_requests("POST", "/api/saves?"), 0);
        assert_eq!(fixture.count_requests("GET", "/api/saves/31/content"), 0);
        assert!(!install_root.join("saves/gb/Game.srm").exists());
    }
}
