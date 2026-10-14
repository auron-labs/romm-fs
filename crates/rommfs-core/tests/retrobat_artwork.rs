use rommfs_core::catalogue::build_catalogue;
use rommfs_core::retrobat::{cache_artwork, cache_artwork_cancellable};
use rommfs_core::romm::{PairedSession, PlatformDto, RomDto, RommClient};
use rommfs_fixture::{contract, FixtureServer, ResponseSpec};
use serde_json::json;
use std::sync::atomic::AtomicBool;

fn client(fixture: &FixtureServer) -> RommClient {
    let client = RommClient::new(fixture.url()).unwrap();
    client
        .restore_paired_session(&PairedSession {
            access_token: "secret".into(),
            device_id: "test".into(),
            scopes: Vec::new(),
            expires_at: None,
        })
        .unwrap();
    client
}

fn sources(resource: &str) -> (Vec<PlatformDto>, Vec<RomDto>) {
    let platforms =
        serde_json::from_str(&contract::platforms(&[(1, "gb", "gb", "Game Boy")])).unwrap();
    let mut rom = contract::rom(1, "gb", "Game.gb", 3, "hash");
    rom["path_cover_large"] = json!(resource);
    (platforms, vec![serde_json::from_value(rom).unwrap()])
}

#[test]
fn eight_artwork_downloads_can_run_concurrently() {
    use rommfs_fixture::FixtureBodyBarrier;
    use std::time::Duration;
    let fixture = FixtureServer::start();
    let (platforms, _) = sources("unused");
    let mut roms = Vec::new();
    let mut barriers = Vec::new();
    for id in 1..=8 {
        let resource = format!("/assets/romm/resources/cover{id}.png");
        let barrier = FixtureBodyBarrier::new();
        fixture.on(
            "GET",
            &resource,
            ResponseSpec::HeldBytes {
                status: 200,
                bytes: b"\x89PNG\r\n\x1a\nfixture".to_vec(),
                barrier: barrier.clone(),
            },
        );
        barriers.push(barrier);
        let mut rom = contract::rom(id, "gb", &format!("Game{id}.gb"), 3, "hash");
        rom["path_cover_large"] = json!(resource);
        roms.push(serde_json::from_value(rom).unwrap());
    }
    let client = client(&fixture);
    let catalogue = build_catalogue(fixture.url(), &platforms, &roms, |_| {}).unwrap();
    let cache = tempfile::tempdir().unwrap();
    std::thread::scope(|scope| {
        let download = scope.spawn(|| cache_artwork(&client, &catalogue, &roms, cache.path()));
        let all_started = barriers
            .iter()
            .all(|barrier| barrier.wait_until_blocked(Duration::from_secs(5)));
        for barrier in &barriers {
            barrier.release();
        }
        let batch = download.join().unwrap().unwrap();
        assert!(
            all_started,
            "all eight requests should start before any completes"
        );
        assert_eq!(batch.files().len(), 8);
        assert!(batch.warnings().is_empty());
    });
}

#[test]
fn artwork_is_deduplicated_cached_and_never_requests_rom_bodies() {
    let fixture = FixtureServer::start();
    let resource = "/assets/romm/resources/roms/1/cover/big.png?ts=3";
    fixture.on(
        "GET",
        resource,
        ResponseSpec::Bytes {
            status: 200,
            bytes: b"\x89PNG\r\n\x1a\nfixture".to_vec(),
            truncate_at: None,
            stall_after_bytes: None,
        },
    );
    let client = client(&fixture);
    let (platforms, roms) = sources(resource);
    let catalogue = build_catalogue(fixture.url(), &platforms, &roms, |_| {}).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let first = cache_artwork(&client, &catalogue, &roms, cache.path()).unwrap();
    assert!(first.warnings().is_empty(), "{:?}", first.warnings());
    assert_eq!(first.references().len(), 2);
    assert_eq!(first.files().len(), 1);
    assert_eq!(fixture.request_count("GET", resource), 1);
    let second = cache_artwork(&client, &catalogue, &roms, cache.path()).unwrap();
    assert_eq!(first.references(), second.references());
    assert_eq!(fixture.request_count("GET", resource), 1);
    assert_eq!(fixture.request_count("GET", "/api/roms"), 0);
    let mut changed = roms;
    changed[0].updated_at = "new-version".into();
    cache_artwork(&client, &catalogue, &changed, cache.path()).unwrap();
    assert_eq!(fixture.request_count("GET", resource), 2);
}

#[test]
fn external_traversal_and_rom_urls_are_rejected_before_network_access() {
    let fixture = FixtureServer::start();
    let client = client(&fixture);
    for resource in [
        "https://example.com/cover.png",
        "//example.com/cover.png",
        "/api/roms/1/content/game.gb",
        "/assets/romm/resources/../roms/game",
        "/assets/romm/resources/%2e%2e/game",
        "/assets/romm/resources/a%2fb",
        "/assets/romm/resources/a\\b",
        "/assets/romm/resources/a#fragment",
    ] {
        assert!(
            client.download_artwork(resource, &mut Vec::new()).is_err(),
            "{resource}"
        );
    }
    assert!(fixture.requests().is_empty());
}

#[test]
fn missing_invalid_images_are_nonfatal_and_cancellation_does_not_publish_cache() {
    let fixture = FixtureServer::start();
    let resource = "/assets/romm/resources/cover.png";
    fixture.on(
        "GET",
        resource,
        ResponseSpec::Bytes {
            status: 200,
            bytes: b"<html>login</html>".to_vec(),
            truncate_at: None,
            stall_after_bytes: None,
        },
    );
    let client = client(&fixture);
    let (platforms, roms) = sources(resource);
    let catalogue = build_catalogue(fixture.url(), &platforms, &roms, |_| {}).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let batch = cache_artwork(&client, &catalogue, &roms, cache.path()).unwrap();
    assert!(!batch.warnings().is_empty());
    assert!(batch.references().is_empty() && batch.files().is_empty());
    assert_eq!(std::fs::read_dir(cache.path()).unwrap().count(), 0);
    assert!(cache_artwork_cancellable(
        &client,
        &catalogue,
        &roms,
        cache.path(),
        &AtomicBool::new(false)
    )
    .is_err());
    assert_eq!(fixture.request_count("GET", resource), 1);
}

#[test]
fn rejected_authentication_is_not_disguised_as_missing_artwork() {
    let fixture = FixtureServer::start();
    let resource = "/assets/romm/resources/cover.png";
    fixture.on(
        "GET",
        resource,
        ResponseSpec::Json {
            status: 401,
            body: "{}".into(),
        },
    );
    let client = client(&fixture);
    let (platforms, roms) = sources(resource);
    let cat = build_catalogue(fixture.url(), &platforms, &roms, |_| {}).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let error = match cache_artwork(&client, &cat, &roms, cache.path()) {
        Err(error) => error,
        Ok(_) => panic!("authentication rejection must require sign-in"),
    };
    assert!(error.needs_sign_in());
    assert!(!client.has_token());
    assert_eq!(std::fs::read_dir(cache.path()).unwrap().count(), 0);
}
