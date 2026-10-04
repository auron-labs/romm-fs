//! Behavioral tests for the real `RommClient` against the in-process
//! `FixtureServer` (PRD R1, §6): verified token/catalogue/download flow,
//! bearer auth on every call, pagination counts, error mapping, truncation
//! and stall handling. All assertions are on observable wire facts.

use rommfs_core::romm::{Credentials, DownloadConfig, MetadataConfig, RommClient};
use rommfs_core::Error;
use rommfs_fixture::{contract, FixtureServer, ResponseSpec};
use serde_json::Value;
use std::time::Duration;

const BYTES: &[u8] = b"ROM-BYTES-\x00\x01\x02\x03-more-bytes-here-0123456789";

fn roms(items: &[Value], total: usize) -> String {
    contract::roms_page(items, total, 200, 0)
}

/// A fixture wired like the verified 5.3.1 contract: token + platforms +
/// one-page roms + a content endpoint.
fn contract_server(rom_items: Vec<Value>, total: usize) -> FixtureServer {
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
            body: contract::platforms(&[
                (1, "nes", "nes", "Nintendo Entertainment System"),
                (2, "snes", "snes", "Super Nintendo"),
            ]),
        },
    );
    fx.on(
        "GET",
        "/api/roms?",
        ResponseSpec::Json {
            status: 200,
            body: roms(&rom_items, total),
        },
    );
    fx
}

fn authed(fx: &FixtureServer) -> RommClient {
    let client = RommClient::new(fx.url()).unwrap();
    client
        .authenticate(Credentials {
            username: "user",
            password: "pass",
        })
        .unwrap();
    client
}

#[test]
fn token_platforms_roms_download_flow_is_byte_exact() {
    let items = vec![
        contract::rom(7, "nes", "Example Game.nes", BYTES.len() as u64, "s"),
        contract::rom(8, "snes", "Another Game.sfc", 99, "s"),
    ];
    let fx = contract_server(items, 2);
    fx.on(
        "GET",
        "/api/roms/",
        ResponseSpec::Bytes {
            status: 200,
            bytes: BYTES.to_vec(),
            truncate_at: None,
            stall_after_bytes: None,
        },
    );
    let client = authed(&fx);

    let platforms = client.platforms().unwrap();
    assert_eq!(platforms.len(), 2);
    assert_eq!(platforms[0].fs_slug, "nes");
    assert_eq!(platforms[0].rom_count, 0);

    let page = client.roms_page(200, 0).unwrap();
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.total, Some(2));
    assert_eq!(page.items[0].files.len(), 1);
    assert_eq!(page.items[0].files[0].file_name, "Example Game.nes");
    assert_eq!(page.items[0].files[0].file_size_bytes, BYTES.len() as u64);

    let mut out = Vec::new();
    let mut seen = Vec::new();
    let received = client
        .download_file(7, "Example Game.nes", &mut out, &mut |got, total| {
            seen.push((got, total))
        })
        .unwrap();
    assert_eq!(received, BYTES.len() as u64);
    assert_eq!(out, BYTES);
    // Progress reported real totals and ended at the full size.
    assert_eq!(
        seen.last().copied(),
        Some((BYTES.len() as u64, Some(BYTES.len() as u64)))
    );

    // Bearer auth travelled on catalogue AND content calls.
    assert_eq!(
        fx.auth_headers("GET", "/api/platforms"),
        vec![Some("Bearer fixture-access-token".to_string())]
    );
    assert_eq!(
        fx.auth_headers("GET", "/api/roms/"),
        vec![Some("Bearer fixture-access-token".to_string())]
    );
    assert_eq!(fx.count("GET", "/api/platforms"), 1);
    assert_eq!(fx.count("GET", "/api/roms?"), 1);
}

#[test]
fn all_roms_pages_exactly_until_total() {
    // 450 ROMs at page size 200 -> exactly 3 page requests.
    let items: Vec<Value> = (0..450)
        .map(|i| contract::rom(i, "nes", &format!("r{i}.nes"), 10, "h"))
        .collect();
    let fx = FixtureServer::start();
    fx.on(
        "POST",
        "/api/token",
        ResponseSpec::Json {
            status: 200,
            body: contract::token_ok(),
        },
    );
    for (offset, chunk) in [(0usize, 0..200), (200, 200..400), (400, 400..450)] {
        let body = contract::roms_page(&items[chunk.clone()], 450, 200, offset);
        fx.on(
            "GET",
            &format!("/api/roms?limit=200&offset={offset}&"),
            ResponseSpec::Json { status: 200, body },
        );
    }
    // Catch-all LAST so the specific offsets match first.
    fx.on(
        "GET",
        "/api/roms?",
        ResponseSpec::Json {
            status: 500,
            body: "{}".into(),
        },
    );
    let client = authed(&fx);

    let all = client.all_roms().unwrap();
    assert_eq!(all.len(), 450);
    assert_eq!(fx.count("GET", "/api/roms?limit=200&offset=0&"), 1);
    assert_eq!(fx.count("GET", "/api/roms?limit=200&offset=200&"), 1);
    assert_eq!(fx.count("GET", "/api/roms?limit=200&offset=400&"), 1);
    // No request beyond the expected pages.
    assert_eq!(fx.count("GET", "/api/roms?"), 0);
}

#[test]
fn listing_never_requests_rom_bodies() {
    let items = vec![contract::rom(7, "nes", "Example Game.nes", 10, "s")];
    let fx = contract_server(items, 1);
    fx.on(
        "GET",
        "/api/roms/",
        ResponseSpec::Json {
            status: 500,
            body: "{}".into(),
        },
    );
    let client = authed(&fx);

    client.platforms().unwrap();
    client.all_roms().unwrap();
    client.roms_page(200, 0).unwrap();

    // Not a single content request was made while listing.
    assert_eq!(fx.count("GET", "/api/roms/"), 0);
}

#[test]
fn rejected_credentials_and_permissions_map_to_auth_errors() {
    // 401 on the token endpoint -> Auth.
    let fx = FixtureServer::start();
    fx.on(
        "POST",
        "/api/token",
        ResponseSpec::Json {
            status: 401,
            body: r#"{"detail":"Incorrect username or password"}"#.into(),
        },
    );
    let client = RommClient::new(fx.url()).unwrap();
    let err = client
        .authenticate(Credentials {
            username: "user",
            password: "wrong",
        })
        .unwrap_err();
    assert!(matches!(err, Error::Auth(_)), "got {err:?}");
    assert!(!client.has_token());

    // 403 on the token endpoint -> Forbidden (insufficient scope).
    let fx2 = FixtureServer::start();
    fx2.on(
        "POST",
        "/api/token",
        ResponseSpec::Json {
            status: 403,
            body: r#"{"detail":"Insufficient scope"}"#.into(),
        },
    );
    let client2 = RommClient::new(fx2.url()).unwrap();
    let err2 = client2
        .authenticate(Credentials {
            username: "user",
            password: "pass",
        })
        .unwrap_err();
    assert!(matches!(err2, Error::Forbidden(_)), "got {err2:?}");
}

#[test]
fn mid_session_401_clears_the_token() {
    let fx = contract_server(vec![], 0);
    let client = authed(&fx);
    assert!(client.has_token());

    // Token expires server-side: the next catalogue call gets a 401.
    fx.on(
        "GET",
        "/api/platforms",
        ResponseSpec::Json {
            status: 401,
            body: r#"{"detail":"Not authenticated"}"#.into(),
        },
    );
    let err = client.platforms().unwrap_err();
    assert!(matches!(err, Error::Auth(_)), "got {err:?}");
    assert!(err.needs_sign_in());
    assert!(!client.has_token(), "token must be cleared after a 401");
}

#[test]
fn forbidden_catalogue_call_maps_to_forbidden() {
    let fx = contract_server(vec![], 0);
    fx.on(
        "GET",
        "/api/platforms",
        ResponseSpec::Json {
            status: 403,
            body: r#"{"detail":"Insufficient scope"}"#.into(),
        },
    );
    let client = authed(&fx);
    let err = client.platforms().unwrap_err();
    assert!(matches!(err, Error::Forbidden(_)), "got {err:?}");
    assert!(client.has_token(), "403 must not clear the token");
}

#[test]
fn malformed_catalogue_payload_is_an_error_not_empty_library() {
    let fx = contract_server(vec![], 0);
    fx.on(
        "GET",
        "/api/platforms",
        ResponseSpec::Json {
            status: 200,
            body: "this is not json".into(),
        },
    );
    let client = authed(&fx);
    let err = client.platforms().unwrap_err();
    assert!(matches!(err, Error::InvalidCatalogue(_)), "got {err:?}");
}

#[test]
fn stalled_authentication_and_catalogue_responses_are_bounded() {
    let metadata = MetadataConfig {
        connect_timeout: Duration::from_secs(1),
        response_timeout: Duration::from_millis(100),
    };

    let auth_fx = FixtureServer::start();
    auth_fx.on("POST", "/api/token", ResponseSpec::Stall);
    let auth_client = RommClient::new(auth_fx.url())
        .unwrap()
        .with_metadata_config(metadata);
    let auth_error = auth_client
        .authenticate(Credentials {
            username: "user",
            password: "pass",
        })
        .unwrap_err();
    assert!(
        matches!(auth_error, Error::Transport(_)),
        "got {auth_error:?}"
    );
    assert_eq!(auth_fx.count("POST", "/api/token"), 1);

    let catalogue_fx = contract_server(vec![], 0);
    let catalogue_client = authed(&catalogue_fx).with_metadata_config(metadata);
    catalogue_fx.on("GET", "/api/platforms", ResponseSpec::Stall);
    let catalogue_error = catalogue_client.platforms().unwrap_err();
    assert!(
        matches!(catalogue_error, Error::Transport(_)),
        "got {catalogue_error:?}"
    );
    assert_eq!(catalogue_fx.count("GET", "/api/platforms"), 1);
}

#[test]
fn truncated_body_fails_with_expected_and_received() {
    let fx = contract_server(vec![], 0);
    fx.on(
        "GET",
        "/api/roms/",
        ResponseSpec::Bytes {
            status: 200,
            bytes: vec![b'x'; 5000],
            truncate_at: Some(1000),
            stall_after_bytes: None,
        },
    );
    let client = authed(&fx);

    let mut out = Vec::new();
    let err = client
        .download_file(9, "big.bin", &mut out, &mut |_, _| {})
        .unwrap_err();
    match err {
        Error::Truncated { expected, received } => {
            assert_eq!(expected, 5000);
            assert_eq!(received, 1000);
        }
        other => panic!("expected Truncated, got {other:?}"),
    }
}

#[test]
fn stalled_transfer_errors_out_via_low_speed_window() {
    let fx = contract_server(vec![], 0);
    fx.on(
        "GET",
        "/api/roms/",
        ResponseSpec::Bytes {
            status: 200,
            bytes: vec![b'y'; 64 * 1024],
            truncate_at: None,
            stall_after_bytes: Some(512),
        },
    );
    let client = authed(&fx).with_download_config(DownloadConfig {
        connect_timeout: Duration::from_secs(5),
        low_speed_bytes_per_sec: 8192,
        low_speed_window: Duration::from_secs(1),
        chunk_bytes: 4096,
    });

    let mut out = Vec::new();
    // Without the no-progress abort this would take ~30s and then succeed —
    // the fixture finishes the full body after its stall window.
    let err = client
        .download_file(9, "stall.bin", &mut out, &mut |_, _| {})
        .unwrap_err();
    assert!(matches!(err, Error::Transport(_)), "got {err:?}");
}
