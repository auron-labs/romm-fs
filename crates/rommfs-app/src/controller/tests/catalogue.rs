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
    for offset in (0..2448).step_by(500) {
        fx.on(
            "GET",
            &format!("/api/roms?limit=500&offset={offset}&"),
            ResponseSpec::Json {
                status: 200,
                body: contract::roms_page(
                    &items[offset..(offset + 500).min(items.len())],
                    items.len(),
                    500,
                    offset,
                ),
            },
        );
    }
    let (sink, events) = rommfs_core::events::channel();
    let mut worker = Worker::new(sink);
    worker.rom_cache_directory = Some(tmp.path().join("rom-cache"));
    worker.catalogue_store = Some(CatalogueStore::open(&tmp.path().join("catalogue.db")).unwrap());
    worker.connect(fx.url(), "user", "password");
    let deadline = Instant::now() + Duration::from_secs(5);
    while worker.refresh_result.is_some() && Instant::now() < deadline {
        worker.poll_catalogue_refresh();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(worker.catalogue.as_ref().unwrap().entries.len(), 2448);
    assert_eq!(fx.route_hit_count("GET", "/api/platforms"), 1);
    for offset in (0..2448).step_by(500) {
        assert_eq!(
            fx.route_hit_count("GET", &format!("/api/roms?limit=500&offset={offset}&")),
            1
        );
    }
    let root = tmp.path().join("roms");
    for _ in 0..2 {
        worker.start_mount(root.to_str().unwrap());
        worker.stop_mount();
        worker.drain_cancelled_mount_preparations();
    }
    assert_eq!(fx.route_hit_count("GET", "/api/platforms"), 1);
    assert_eq!(fx.route_hit_count("GET", "/api/roms/"), 0);
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
        "/api/roms?limit=500&offset=500&",
        ResponseSpec::Json {
            status: 500,
            body: "{}".into(),
        },
    );
    worker.refresh_catalogue();
    let (_, result) = worker
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
        hide_owned_copies: true,
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
fn remount_notice_tracks_file_content_and_size_but_not_artwork() {
    use rommfs_fixture::contract;

    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let snapshot = CatalogueSnapshot {
        platforms: serde_json::from_str(&contract::platforms(&[(1, "gb", "gb", "Game Boy")]))
            .unwrap(),
        roms: vec![
            serde_json::from_value(contract::rom(7, "gb", "Game.gb", 3, "original")).unwrap(),
        ],
        refreshed_at: 0,
        full_refreshed_at: 0,
    };
    let (sink, events) = rommfs_core::events::channel();
    let mut worker = Worker::new(sink);
    worker.server_id = Some("s".into());
    worker.rom_cache_directory = Some(directory.path().join("cache"));
    worker.store_catalogue(worker.project_snapshot(&snapshot).unwrap());
    let client = Arc::new(RommClient::new(fixture.url()).unwrap());
    worker.fs = Some(
        worker
            .build_fs(&client, directory.path(), Arc::new(NoopHydratedRemover))
            .unwrap(),
    );
    let mut state = UiState::new(32);

    let mut artwork = snapshot.clone();
    artwork.roms[0].metadata.path_cover_large = Some("/assets/new-cover.png".into());
    artwork.roms[0].updated_at = "artwork-only-update".into();
    let mut version = snapshot.clone();
    version.roms[0].files[0].sha1_hash = Some("changed".into());
    let mut size = snapshot.clone();
    size.roms[0].files[0].file_size_bytes += 1;
    for (next, pending) in [
        (snapshot, false),
        (artwork, false),
        (version, true),
        (size, true),
    ] {
        worker.publish_snapshot(next, false).unwrap();
        for event in events.try_iter() {
            state.apply_event(&event);
        }
        assert_eq!(state.catalogue_pending_remount, pending);
    }
    assert!(fixture.requests().is_empty());
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

#[test]
fn layout_changes_wait_for_cold_loading_and_mount_preparation() {
    for preparing in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let (sink, events) = rommfs_core::events::channel();
        let mut worker = Worker::new(sink);
        worker.preferences = Some(ConnectionPreferences {
            server: "server".into(),
            account_id: 42,
            device_identifier: "device".into(),
            mount_root: None,
            integration: IntegrationMode::RetroBat,
            retrobat_root: Some(directory.path().into()),
            start_at_login: false,
            save_root: None,
            hide_owned_copies: true,
        });
        if preparing {
            let (send, result) = mpsc::channel();
            worker.mount_preparation = Some(MountPreparation {
                root: directory.path().join("games"),
                path: "games".into(),
                server_id: "server".into(),
                started: Instant::now(),
                active: Arc::new(std::sync::atomic::AtomicBool::new(true)),
                result,
                handle: std::thread::spawn(move || {
                    let _ = send.send(Err(Error::Cancelled));
                }),
            });
        } else {
            worker.pending_mount_start = Some("games".into());
        }

        worker.set_integration(IntegrationMode::Generic, None);
        assert_eq!(
            worker.preferences.as_ref().unwrap().integration,
            IntegrationMode::RetroBat
        );
        assert!(events.try_iter().any(|event| matches!(
            event,
            AppEvent::IntegrationUpdated { error: Some(reason), .. }
                if reason.contains("stop the games folder")
        )));
        worker.stop_mount();
        worker.drain_cancelled_mount_preparations();
    }
}

fn authed_client(fx: &FixtureServer) -> RommClient {
    let client = RommClient::new(fx.url()).unwrap();
    client
        .authenticate(Credentials {
            username: "user",
            password: "pass",
        })
        .unwrap();
    client
}

fn fixture_rom(id: i64, file_name: &str) -> RomDto {
    serde_json::from_value(rommfs_fixture::contract::rom(
        id, "nes", file_name, 10, "hash",
    ))
    .unwrap()
}

/// 2448 ROMs at 500/page: five pages, so four are fetched concurrently.
fn concurrent_catalogue_fixture() -> (FixtureServer, FixtureBodyBarrier, usize) {
    use rommfs_fixture::contract;
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
        "/api/platforms",
        ResponseSpec::Json {
            status: 200,
            body: contract::platforms(&[(1, "nes", "nes", "NES")]),
        },
    );
    let total: usize = 2448;
    let items: Vec<_> = (0..total)
        .map(|n| contract::rom(n as i64 + 1, "nes", &format!("game{n}.bin"), 10, "hash"))
        .collect();
    fx.on(
        "GET",
        "/api/roms?limit=500&offset=0&",
        ResponseSpec::Json {
            status: 200,
            body: contract::roms_page(&items[0..500], total, 500, 0),
        },
    );
    let barrier = FixtureBodyBarrier::new();
    for offset in [500usize, 1000, 1500, 2000] {
        let end = (offset + 500).min(total);
        fx.on(
            "GET",
            &format!("/api/roms?limit=500&offset={offset}&"),
            ResponseSpec::HeldJson {
                status: 200,
                body: contract::roms_page(&items[offset..end], total, 500, offset),
                barrier: barrier.clone(),
            },
        );
    }
    (fx, barrier, total)
}

#[test]
fn concurrent_catalogue_pages_are_reassembled_in_offset_order() {
    let (fx, barrier, total) = concurrent_catalogue_fixture();
    let client = authed_client(&fx);
    let (sink, _events) = rommfs_core::events::channel();
    let snapshot = std::thread::scope(|scope| {
        let handle = scope.spawn(|| fetch_catalogue(&client, &sink, 1, None));
        // All four remaining pages must be in flight at once.
        for _ in 0..4 {
            assert!(
                barrier.wait_until_blocked(Duration::from_secs(5)),
                "fewer than four catalogue pages were fetched concurrently"
            );
        }
        for _ in 0..4 {
            barrier.release();
        }
        handle.join().unwrap().unwrap()
    });
    let ids: Vec<i64> = snapshot.roms.iter().map(|rom| rom.id).collect();
    assert_eq!(ids, (1..=total as i64).collect::<Vec<_>>());
}

#[test]
fn cancelling_a_concurrent_catalogue_fetch_reports_cancelled() {
    let (fx, barrier, _total) = concurrent_catalogue_fixture();
    let client = authed_client(&fx);
    let (sink, _events) = rommfs_core::events::channel();
    let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let scoped = sink.clone().scoped(active.clone());
    let result = std::thread::scope(|scope| {
        let handle = scope.spawn(|| fetch_catalogue(&client, &scoped, 1, None));
        for _ in 0..4 {
            assert!(barrier.wait_until_blocked(Duration::from_secs(5)));
        }
        active.store(false, std::sync::atomic::Ordering::Release);
        for _ in 0..4 {
            barrier.release();
        }
        handle.join().unwrap()
    });
    assert!(matches!(result, Err(Error::Cancelled)), "got {result:?}");
}

#[test]
fn incremental_refresh_applies_changes_additions_and_deletions() {
    use rommfs_fixture::contract;
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
        "/api/platforms",
        ResponseSpec::Json {
            status: 200,
            body: contract::platforms(&[(1, "nes", "nes", "NES")]),
        },
    );
    fx.on(
        "GET",
        "/api/roms/identifiers",
        ResponseSpec::Json {
            status: 200,
            body: serde_json::json!([1, 2, 3]).to_string(),
        },
    );
    // The delta reports the changed ROM 2 and the new ROM 3; ROM 4 was deleted.
    let delta = vec![
        contract::rom(2, "nes", "changed.bin", 10, "hash"),
        contract::rom(3, "nes", "new.bin", 10, "hash"),
    ];
    fx.on(
        "GET",
        "/api/roms?limit=500&offset=0&with_files=true&with_char_index=false&with_filter_values=false&with_rom_id_index=false&with_total=true&updated_after=",
        ResponseSpec::Json {
            status: 200,
            body: contract::roms_page(&delta, 2, 500, 0),
        },
    );
    let client = authed_client(&fx);
    let now = rommfs_core::cache::now_unix_secs();
    let prior = CatalogueSnapshot {
        platforms: vec![],
        roms: vec![
            fixture_rom(1, "unchanged.bin"),
            fixture_rom(2, "old.bin"),
            fixture_rom(4, "deleted.bin"),
        ],
        refreshed_at: now,
        full_refreshed_at: now,
    };
    let (sink, _events) = rommfs_core::events::channel();
    let snapshot = fetch_catalogue(&client, &sink, 1, Some(prior)).unwrap();
    assert_eq!(
        snapshot.roms.iter().map(|rom| rom.id).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    let file_name = |id: i64| {
        snapshot.roms.iter().find(|rom| rom.id == id).unwrap().files[0]
            .file_name
            .clone()
    };
    assert_eq!(file_name(1), "unchanged.bin");
    assert_eq!(file_name(2), "changed.bin");
    assert_eq!(file_name(3), "new.bin");
    // The delta covers the whole window since the previous refresh start, with
    // the five-minute skew.
    let expected = rommfs_core::romm::iso8601_utc(now.saturating_sub(300)).replace(':', "%3A");
    let delta_target = fx
        .requests()
        .into_iter()
        .find(|request| request.target.contains("updated_after="))
        .expect("a delta request was made");
    assert!(
        delta_target.target.contains(&expected),
        "{} !~ {expected}",
        delta_target.target
    );
}

#[test]
fn incremental_refresh_falls_back_to_a_full_walk_when_the_server_rejects_the_delta() {
    use rommfs_fixture::contract;
    // `/identifiers` missing (404, older server) and the delta page rejecting
    // `updated_after` (422) both fall back to the full walk.
    for reject in ["identifiers", "updated_after"] {
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
            "/api/platforms",
            ResponseSpec::Json {
                status: 200,
                body: contract::platforms(&[(1, "nes", "nes", "NES")]),
            },
        );
        let (status, body) = match reject {
            "identifiers" => (404, r#"{"detail":"not found"}"#.to_string()),
            _ => (422, r#"{"detail":"invalid updated_after"}"#.to_string()),
        };
        fx.on(
            "GET",
            "/api/roms/identifiers",
            if reject == "identifiers" {
                ResponseSpec::Json {
                    status,
                    body: body.clone(),
                }
            } else {
                ResponseSpec::Json {
                    status: 200,
                    body: "[1,2]".into(),
                }
            },
        );
        if reject == "updated_after" {
            fx.on(
                "GET",
                "/api/roms?limit=500&offset=0&with_files=true&with_char_index=false&with_filter_values=false&with_rom_id_index=false&with_total=true&updated_after=",
                ResponseSpec::Json { status, body },
            );
        }
        let full = vec![
            contract::rom(1, "nes", "a.bin", 10, "hash"),
            contract::rom(2, "nes", "b.bin", 10, "hash"),
        ];
        fx.on(
            "GET",
            "/api/roms?limit=500&offset=0&",
            ResponseSpec::Json {
                status: 200,
                body: contract::roms_page(&full, 2, 500, 0),
            },
        );
        let client = authed_client(&fx);
        let now = rommfs_core::cache::now_unix_secs();
        let prior = CatalogueSnapshot {
            platforms: vec![],
            roms: vec![fixture_rom(9, "stale.bin")],
            refreshed_at: now,
            full_refreshed_at: now,
        };
        let (sink, events) = rommfs_core::events::channel();
        let snapshot = fetch_catalogue(&client, &sink, 1, Some(prior)).unwrap();
        assert_eq!(
            snapshot.roms.iter().map(|rom| rom.id).collect::<Vec<_>>(),
            vec![1, 2],
            "reject={reject}"
        );
        assert!(events.try_iter().any(|event| matches!(
            event,
            AppEvent::Log(line) if line.message.contains("incremental refresh unavailable")
        )));
    }
}

#[test]
fn incremental_refresh_falls_back_when_visible_ids_cannot_be_reconciled() {
    use rommfs_fixture::contract;
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
        "/api/platforms",
        ResponseSpec::Json {
            status: 200,
            body: contract::platforms(&[(1, "nes", "nes", "NES")]),
        },
    );
    // The server lists ROM 2 as visible, but neither the snapshot nor the delta
    // carries it, so the delta cannot be trusted.
    fx.on(
        "GET",
        "/api/roms/identifiers",
        ResponseSpec::Json {
            status: 200,
            body: serde_json::json!([1, 2, 3]).to_string(),
        },
    );
    let delta = vec![contract::rom(3, "nes", "c.bin", 10, "hash")];
    fx.on(
        "GET",
        "/api/roms?limit=500&offset=0&with_files=true&with_char_index=false&with_filter_values=false&with_rom_id_index=false&with_total=true&updated_after=",
        ResponseSpec::Json {
            status: 200,
            body: contract::roms_page(&delta, 1, 500, 0),
        },
    );
    let full = vec![
        contract::rom(1, "nes", "a.bin", 10, "hash"),
        contract::rom(2, "nes", "b.bin", 10, "hash"),
        contract::rom(3, "nes", "c.bin", 10, "hash"),
    ];
    fx.on(
        "GET",
        "/api/roms?limit=500&offset=0&",
        ResponseSpec::Json {
            status: 200,
            body: contract::roms_page(&full, 3, 500, 0),
        },
    );
    let client = authed_client(&fx);
    let now = rommfs_core::cache::now_unix_secs();
    let prior = CatalogueSnapshot {
        platforms: vec![],
        roms: vec![fixture_rom(1, "a.bin"), fixture_rom(4, "gone.bin")],
        refreshed_at: now,
        full_refreshed_at: now,
    };
    let (sink, _events) = rommfs_core::events::channel();
    let snapshot = fetch_catalogue(&client, &sink, 1, Some(prior)).unwrap();
    assert_eq!(
        snapshot.roms.iter().map(|rom| rom.id).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
}

#[test]
fn catalogue_first_page_integrity_is_enforced() {
    use rommfs_fixture::contract;
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
        "/api/platforms",
        ResponseSpec::Json {
            status: 200,
            body: contract::platforms(&[(1, "nes", "nes", "NES")]),
        },
    );
    let client = authed_client(&fx);
    let (sink, _events) = rommfs_core::events::channel();
    // (label, declared total, returned limit, first-page item count). The first
    // page must be exactly `min(limit, total)`, and a zero limit is invalid.
    let cases = [
        ("short before total", 1000usize, 500usize, 10usize),
        ("more than the total", 10, 500, 20),
        ("zero returned limit", 1000, 0, 0),
    ];
    for (label, total, limit, count) in cases {
        let items: Vec<_> = (0..count)
            .map(|n| contract::rom(n as i64 + 1, "nes", &format!("g{n}.bin"), 10, "hash"))
            .collect();
        fx.on(
            "GET",
            "/api/roms?limit=500&offset=0&",
            ResponseSpec::Json {
                status: 200,
                body: contract::roms_page(&items, total, limit, 0),
            },
        );
        let result = fetch_catalogue(&client, &sink, 1, None);
        assert!(
            matches!(result, Err(Error::InvalidCatalogue(_))),
            "{label}: {result:?}"
        );
    }
}

#[test]
fn connected_session_catalogue_poll_fires_only_when_due() {
    let (sink, _events) = rommfs_core::events::channel();
    let mut worker = Worker::new(sink);
    worker.client = Some(Arc::new(RommClient::new("http://127.0.0.1:9").unwrap()));
    worker.next_catalogue_poll = Some(Instant::now() + Duration::from_secs(60));
    worker.poll_catalogue_refresh_timer();
    assert!(worker.refresh_result.is_none(), "polled before it was due");

    worker.next_catalogue_poll = Some(Instant::now() - Duration::from_secs(1));
    worker.poll_catalogue_refresh_timer();
    assert!(worker.refresh_result.is_some(), "due poll did not refresh");
    assert!(worker.next_catalogue_poll.unwrap() > Instant::now());

    // A due poll while a refresh is already in flight is skipped: the running
    // refresh is neither replaced nor cancelled.
    let (_tx, rx) = mpsc::channel();
    worker.refresh_result = Some(rx);
    let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
    worker.refresh_active = Some(active.clone());
    worker.next_catalogue_poll = Some(Instant::now() - Duration::from_secs(1));
    worker.poll_catalogue_refresh_timer();
    assert!(
        worker.refresh_result.is_some(),
        "an in-flight refresh was replaced"
    );
    assert!(
        active.load(std::sync::atomic::Ordering::Acquire),
        "an in-flight refresh was cancelled"
    );
    assert!(worker.next_catalogue_poll.unwrap() > Instant::now());

    worker.cancel_catalogue_refresh();
    worker.next_catalogue_poll = None;
    worker.poll_catalogue_refresh_timer();
    assert!(worker.refresh_result.is_none(), "polled while disconnected");
}

#[test]
fn hydration_names_use_the_api_content_name_for_folder_roms() {
    use rommfs_fixture::contract;
    // A folder-layout ROM: the projected tree path carries the folder prefix,
    // while the content URL needs the API file_name alone.
    let rom = serde_json::json!({
        "id": 7,
        "platform_fs_slug": "gb",
        "platform_slug": "gb",
        "fs_name": "foldertest",
        "fs_path": "roms/gb",
        "fs_size_bytes": 39,
        "has_simple_single_file": false,
        "has_nested_single_file": false,
        "has_multiple_files": true,
        "missing_from_fs": false,
        "is_physical": true,
        "updated_at": "2026-10-01T00:00:00",
        "files": [{
            "id": 700,
            "file_name": "foldertest.gb",
            "file_path": "roms/gb/foldertest",
            "file_size_bytes": 39,
            "last_modified": "2026-10-01T00:00:00",
            "crc_hash": null,
            "md5_hash": null,
            "sha1_hash": null,
            "is_top_level": false,
        }],
    });
    let snapshot = CatalogueSnapshot {
        platforms: serde_json::from_str(&contract::platforms(&[(1, "gb", "gb", "Game Boy")]))
            .unwrap(),
        roms: vec![serde_json::from_value(rom).unwrap()],
        refreshed_at: 0,
        full_refreshed_at: 0,
    };
    let (sink, _events) = rommfs_core::events::channel();
    let mut worker = Worker::new(sink);
    worker.server_id = Some("s".into());
    worker.store_catalogue(worker.project_snapshot(&snapshot).unwrap());
    let entry = worker.catalogue.as_ref().unwrap().entries.first().unwrap();
    assert_eq!(entry.file_name, "foldertest/foldertest.gb");
    assert_eq!(entry.content_name, "foldertest.gb");
    assert_eq!(worker.names.get(&entry.key).unwrap(), "foldertest.gb");
}
