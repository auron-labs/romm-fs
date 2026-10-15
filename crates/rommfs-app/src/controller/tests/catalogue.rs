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
    worker.catalogue_store = Some(CatalogueStore::open(&tmp.path().join("catalogue.db")).unwrap());
    worker.connect(fx.url(), "user", "password");
    assert_eq!(worker.catalogue.as_ref().unwrap().entries.len(), 2448);
    assert_eq!(fx.route_hit_count("GET", "/api/platforms"), 1);
    for offset in (0..2448).step_by(200) {
        assert_eq!(
            fx.route_hit_count("GET", &format!("/api/roms?limit=200&offset={offset}&")),
            1
        );
    }
    let root = tmp.path().join("roms");
    for _ in 0..2 {
        worker.start_mount(root.to_str().unwrap());
        worker.stop_mount();
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
        "/api/roms?limit=200&offset=200&",
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
