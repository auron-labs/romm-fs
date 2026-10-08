use rommfs_core::{
    romm::{Credentials, PairingPoll, RommClient},
    Error,
};
use rommfs_fixture::{FixtureServer, ResponseSpec};
use serde_json::json;

#[test]
fn device_pairing_uses_verified_shapes_and_approved_token_authenticates_account() {
    let fx = FixtureServer::start();
    fx.on("POST", "/api/auth/device/init", ResponseSpec::Json { status: 201, body: json!({
        "device_code":"private-device-code", "user_code":"ABCD-EFGH",
        "verification_path":"/pair/device", "verification_path_complete":"/pair/device?user_code=ABCD-EFGH",
        "expires_in":600, "interval":5
    }).to_string() });
    let client = RommClient::new(fx.url()).unwrap();
    let request = client.begin_pairing("stable-installation-id").unwrap();
    assert_eq!(
        client.pairing_url(&request).unwrap(),
        format!("{}/pair/device?user_code=ABCD-EFGH", fx.url())
    );
    for (detail, expected) in [
        ("authorization_pending", 0),
        ("slow_down", 1),
        ("access_denied", 2),
        ("expired_token", 3),
    ] {
        fx.on(
            "POST",
            "/api/auth/device/token",
            ResponseSpec::Json {
                status: 400,
                body: json!({"detail": detail}).to_string(),
            },
        );
        let result = client.poll_pairing(&request.device_code).unwrap();
        let actual = match result {
            PairingPoll::Pending => 0,
            PairingPoll::SlowDown => 1,
            PairingPoll::Denied => 2,
            PairingPoll::Expired => 3,
            PairingPoll::Approved(_) => 4,
        };
        assert_eq!(actual, expected);
    }
    fx.on("POST", "/api/auth/device/token", ResponseSpec::Json { status: 200, body: json!({
        "access_token":"approved-secret", "device_id":"device-id", "scopes":["me.read"], "expires_at":null
    }).to_string() });
    let PairingPoll::Approved(session) = client.poll_pairing(&request.device_code).unwrap() else {
        panic!("approval expected");
    };
    let requests = fx.requests();
    let init = requests
        .iter()
        .find(|r| r.target == "/api/auth/device/init")
        .unwrap();
    let init_body: serde_json::Value = serde_json::from_slice(&init.body).unwrap();
    assert_eq!(
        init_body["client_device_identifier"],
        "stable-installation-id"
    );
    for request in requests
        .iter()
        .filter(|r| r.target == "/api/auth/device/token")
    {
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body, json!({"device_code":"private-device-code"}));
    }
    client.restore_paired_session(&session).unwrap();
    fx.on(
        "GET",
        "/api/users/me",
        ResponseSpec::Json {
            status: 200,
            body: json!({"id":42,"oauth_scopes":["me.read"]}).to_string(),
        },
    );
    assert_eq!(client.save_sync_identity().unwrap().unwrap().account_id, 42);
    assert_eq!(
        fx.auth_headers("GET", "/api/users/me"),
        vec![Some("Bearer approved-secret".into())]
    );
    fx.on(
        "GET",
        "/api/users/me",
        ResponseSpec::Json {
            status: 401,
            body: "{}".into(),
        },
    );
    assert!(matches!(client.save_sync_identity(), Err(Error::Auth(_))));
    assert!(!client.has_token());
}

#[test]
fn pairing_rejects_other_origins_and_authentication_errors_never_echo_secrets() {
    for server in [
        "http://user:password@example.com",
        "file:///etc/passwd",
        "https://example.com?token=secret",
        "https://example.com/#secret",
    ] {
        assert!(RommClient::new(server).is_err());
    }
    let fx = FixtureServer::start();
    for path in [
        "https://attacker.invalid/",
        "//attacker.invalid/",
        "/pair/device?user_code=x\n",
        "/other?user_code=x",
    ] {
        fx.on("POST", "/api/auth/device/init", ResponseSpec::Json { status:201, body:json!({
            "device_code":"secret", "user_code":"code", "verification_path":"/pair/device", "verification_path_complete":path, "expires_in":600, "interval":5
        }).to_string() });
        let client = RommClient::new(fx.url()).unwrap();
        assert!(client
            .pairing_url(&client.begin_pairing("id").unwrap())
            .is_err());
    }
    fx.on(
        "POST",
        "/api/token",
        ResponseSpec::Json {
            status: 500,
            body: r#"{"detail":"password=private-secret"}"#.into(),
        },
    );
    let client = RommClient::new(fx.url()).unwrap();
    let error = client
        .authenticate(Credentials {
            username: "user",
            password: "private-secret",
        })
        .unwrap_err();
    assert!(matches!(error, Error::Http { status: 500, .. }));
    assert!(!error.to_string().contains("private-secret"));
}
