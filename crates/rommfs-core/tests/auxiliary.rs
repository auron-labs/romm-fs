use rommfs_core::retrobat::materialize_auxiliaries;
use rommfs_core::romm::{PlatformDto, RomDto, RommClient};
use rommfs_fixture::{FixtureBodyBarrier, FixtureServer, ResponseSpec};
use std::fs;
use std::sync::atomic::AtomicBool;

#[test]
fn explicit_auxiliary_setup_materializes_verified_destinations_and_preserves_modified_files() {
    let fx = FixtureServer::start();
    let platform:PlatformDto=serde_json::from_value(serde_json::json!({"id":1,"slug":"switch","fs_slug":"switch","name":"Switch","custom_name":null,"rom_count":1})).unwrap();
    let rom:RomDto=serde_json::from_value(serde_json::json!({"id":7,"platform_fs_slug":"switch","platform_slug":"switch","fs_name":"Game.nsp","fs_size_bytes":3,"has_simple_single_file":true,"has_nested_single_file":false,"has_multiple_files":false,"missing_from_fs":false,"is_physical":false,"updated_at":"now","fs_path":"roms/switch","files":[{"id":80,"file_name":"Game.cht","file_size_bytes":5,"is_top_level":true,"file_path":"roms/switch","category":"cheat"},{"id":81,"file_name":"unknown.zip","file_size_bytes":4,"is_top_level":true,"file_path":"roms/switch","category":"cheat"}]})).unwrap();
    let firmware = serde_json::json!([
        {"id":1,"platform_id":1,"file_name":"prod.keys","file_size_bytes":4,"updated_at":"v1"},
        {"id":2,"platform_id":1,"file_name":"firmware.nca","file_size_bytes":8,"updated_at":"v1"},
        {"id":3,"platform_id":1,"file_name":"firmware.zip","file_size_bytes":8,"updated_at":"v1"},
        {"id":4,"platform_id":1,"file_name":"../escape","file_size_bytes":8,"updated_at":"v1"}
    ]);
    fx.on(
        "GET",
        "/api/firmware",
        ResponseSpec::Json {
            status: 200,
            body: firmware.to_string(),
        },
    );
    fx.on(
        "GET",
        "/api/firmware/1/content/",
        ResponseSpec::Bytes {
            status: 200,
            bytes: b"keys".to_vec(),
            truncate_at: None,
            stall_after_bytes: None,
        },
    );
    fx.on(
        "GET",
        "/api/firmware/2/content/",
        ResponseSpec::Bytes {
            status: 200,
            bytes: b"firmware".to_vec(),
            truncate_at: None,
            stall_after_bytes: None,
        },
    );
    fx.on(
        "GET",
        "/api/roms/80/files/content/",
        ResponseSpec::Bytes {
            status: 200,
            bytes: b"cheat".to_vec(),
            truncate_at: None,
            stall_after_bytes: None,
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let install = dir.path().join("RetroBat");
    fs::create_dir_all(install.join("emulationstation")).unwrap();
    fs::write(
        install.join("emulationstation/emulatorLauncher.cfg"),
        "home=.\\.emulationstation\nsaves=.\\..\\saves\nbios=.\\..\\bios\ncheats=.\\..\\cheats\n",
    )
    .unwrap();
    fs::create_dir_all(install.join("emulators/eden")).unwrap();
    fs::create_dir_all(install.join("emulators/citron")).unwrap();
    let client = RommClient::new(fx.url()).unwrap();
    let active = AtomicBool::new(true);
    let run = || {
        materialize_auxiliaries(
            &client,
            "server",
            7,
            &install,
            std::slice::from_ref(&platform),
            std::slice::from_ref(&rom),
            &active,
            |_, _| {},
        )
        .unwrap()
    };
    let result = run();
    assert_eq!(result.published, 5);
    assert_eq!(result.warnings.len(), 3);
    for emulator in ["eden", "citron"] {
        assert_eq!(
            fs::read(install.join(format!("emulators/{emulator}/user/keys/prod.keys"))).unwrap(),
            b"keys"
        );
        assert_eq!(
            fs::read(install.join(format!(
                "emulators/{emulator}/user/nand/system/Contents/registered/firmware.nca"
            )))
            .unwrap(),
            b"firmware"
        );
    }
    let cheat = install.join("cheats/retroarch/switch/Game.cht");
    assert_eq!(fs::read(&cheat).unwrap(), b"cheat");
    let count = fx.count_requests("GET", "/api/roms/80/files/content/");
    assert_eq!(run().unchanged, 5);
    assert_eq!(
        fx.count_requests("GET", "/api/roms/80/files/content/"),
        count
    );
    fs::write(&cheat, b"user changes").unwrap();
    assert_eq!(run().unchanged, 4);
    assert_eq!(fs::read(&cheat).unwrap(), b"user changes");
    assert!(materialize_auxiliaries(
        &client,
        "server",
        8,
        &install,
        std::slice::from_ref(&platform),
        std::slice::from_ref(&rom),
        &active,
        |_, _| {}
    )
    .is_err());
    assert_eq!(fx.count_requests("GET", "/api/roms/81/files/content/"), 0);
    assert_eq!(fx.count_requests("GET", "/api/roms/7/content/"), 0);
}

#[test]
fn duplicate_auxiliary_destinations_dedupe_or_skip_instead_of_failing() {
    let fx = FixtureServer::start();
    let platform:PlatformDto=serde_json::from_value(serde_json::json!({"id":1,"slug":"gb","fs_slug":"gb","name":"Game Boy","custom_name":null,"rom_count":0})).unwrap();
    // The same BIOS uploaded under two library entries is one real file; a
    // genuinely different file with the same name is a conflict that must not
    // abort the other destinations.
    fx.on("GET","/api/firmware",ResponseSpec::Json { status:200,body:serde_json::json!([
        {"id":1,"platform_id":1,"file_name":"bios.bin","file_size_bytes":4,"updated_at":"v1"},
        {"id":2,"platform_id":1,"file_name":"BIOS.BIN","file_size_bytes":4,"updated_at":"v1"},
        {"id":3,"platform_id":1,"file_name":"conflict.bin","file_size_bytes":4,"updated_at":"v1"},
        {"id":4,"platform_id":1,"file_name":"conflict.bin","file_size_bytes":9,"updated_at":"v2"},
        {"id":5,"platform_id":1,"file_name":"other.bin","file_size_bytes":2,"updated_at":"v1"}
    ]).to_string() });
    for (id, bytes) in [
        (1, b"keys".as_slice()),
        (2, b"keys".as_slice()),
        (5, b"go".as_slice()),
    ] {
        fx.on(
            "GET",
            &format!("/api/firmware/{id}/content/"),
            ResponseSpec::Bytes {
                status: 200,
                bytes: bytes.to_vec(),
                truncate_at: None,
                stall_after_bytes: None,
            },
        );
    }
    let dir = tempfile::tempdir().unwrap();
    let install = dir.path().join("RetroBat");
    fs::create_dir_all(install.join("emulationstation")).unwrap();
    fs::write(
        install.join("emulationstation/emulatorLauncher.cfg"),
        "home=.\\.emulationstation\nsaves=.\\..\\saves\nbios=.\\..\\bios\ncheats=.\\..\\cheats\n",
    )
    .unwrap();
    let report = materialize_auxiliaries(
        &RommClient::new(fx.url()).unwrap(),
        "server",
        7,
        &install,
        &[platform],
        &[],
        &AtomicBool::new(true),
        |_, _| {},
    )
    .unwrap();
    assert_eq!(report.published, 2);
    assert_eq!(fs::read(install.join("bios/bios.bin")).unwrap(), b"keys");
    assert_eq!(fs::read(install.join("bios/other.bin")).unwrap(), b"go");
    assert!(!install.join("bios/conflict.bin").exists());
    let warnings = report.warnings.join("\n");
    assert!(warnings.contains("conflict.bin"), "{warnings}");
    assert!(warnings.contains("bios.bin"), "{warnings}");
    assert_eq!(fx.count_requests("GET", "/api/firmware/3/content/"), 0);
    assert_eq!(fx.count_requests("GET", "/api/firmware/4/content/"), 0);
    assert_eq!(fx.count_requests("GET", "/api/firmware/2/content/"), 1);
}

#[test]
fn duplicate_firmware_requires_equal_downloaded_content_not_size_or_timestamp() {
    // Missing/equal timestamps do not prove equality, and differing timestamps
    // do not make byte-identical BIOS files conflict.
    for (second_bytes, second_version, expected_published) in [
        (b"BBBB".as_slice(), "", 1),
        (b"BBBB".as_slice(), "v1", 1),
        (b"AAAA".as_slice(), "v2", 2),
    ] {
        let fx = FixtureServer::start();
        let first_version = if second_version.is_empty() { "" } else { "v1" };
        fx.on("GET", "/api/firmware", ResponseSpec::Json { status: 200, body: serde_json::json!([
            {"id":1,"platform_id":1,"file_name":"bios.bin","file_size_bytes":4,"updated_at":first_version},
            {"id":2,"platform_id":1,"file_name":"BIOS.BIN","file_size_bytes":4,"updated_at":second_version},
            {"id":3,"platform_id":1,"file_name":"other.bin","file_size_bytes":2}
        ]).to_string() });
        for (id, bytes) in [
            (1, b"AAAA".as_slice()),
            (2, second_bytes),
            (3, b"ok".as_slice()),
        ] {
            fx.on(
                "GET",
                &format!("/api/firmware/{id}/content/"),
                ResponseSpec::Bytes {
                    status: 200,
                    bytes: bytes.to_vec(),
                    truncate_at: None,
                    stall_after_bytes: None,
                },
            );
        }
        let platform: PlatformDto = serde_json::from_value(serde_json::json!({"id":1,"slug":"gb","fs_slug":"gb","name":"Game Boy","custom_name":null,"rom_count":0})).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let install = dir.path().join("RetroBat");
        fs::create_dir_all(install.join("emulationstation")).unwrap();
        fs::write(install.join("emulationstation/emulatorLauncher.cfg"),
            "home=.\\.emulationstation\nsaves=.\\..\\saves\nbios=.\\..\\bios\ncheats=.\\..\\cheats\n").unwrap();
        let report = materialize_auxiliaries(
            &RommClient::new(fx.url()).unwrap(),
            "s",
            7,
            &install,
            &[platform],
            &[],
            &AtomicBool::new(true),
            |_, _| {},
        )
        .unwrap();
        assert_eq!(report.published, expected_published);
        assert_eq!(fs::read(install.join("bios/other.bin")).unwrap(), b"ok");
        if expected_published == 2 {
            assert_eq!(fs::read(install.join("bios/bios.bin")).unwrap(), b"AAAA");
        } else {
            assert!(!install.join("bios/bios.bin").exists());
            assert!(report
                .warnings
                .iter()
                .any(|w| w.contains("Conflicting auxiliary contents")));
        }
        assert_eq!(fx.count_requests("GET", "/api/firmware/2/content/"), 1);
    }
}

#[test]
fn cancelled_or_truncated_auxiliary_download_does_not_publish_partial_files() {
    let fx = FixtureServer::start();
    let barrier = FixtureBodyBarrier::new();
    fx.on("GET","/api/firmware",ResponseSpec::Json { status:200,body:serde_json::json!([{"id":1,"platform_id":1,"file_name":"bios.bin","file_size_bytes":8,"updated_at":"v1"}]).to_string() });
    fx.on(
        "GET",
        "/api/firmware/1/content/",
        ResponseSpec::HeldBytes {
            status: 200,
            bytes: b"12345678".to_vec(),
            barrier: barrier.clone(),
        },
    );
    let platform:PlatformDto=serde_json::from_value(serde_json::json!({"id":1,"slug":"gb","fs_slug":"gb","name":"Game Boy","custom_name":null,"rom_count":0})).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let install = dir.path().join("RetroBat");
    fs::create_dir_all(install.join("emulationstation")).unwrap();
    fs::write(
        install.join("emulationstation/emulatorLauncher.cfg"),
        "home=.\\.emulationstation\nsaves=.\\..\\saves\nbios=.\\..\\bios\ncheats=.\\..\\cheats\n",
    )
    .unwrap();
    let active = std::sync::Arc::new(AtomicBool::new(true));
    let flag = active.clone();
    let root = install.clone();
    let server = fx.url().to_string();
    let p = platform.clone();
    let handle = std::thread::spawn(move || {
        materialize_auxiliaries(
            &RommClient::new(server).unwrap(),
            "server",
            7,
            &root,
            &[p],
            &[],
            &flag,
            |_, _| {},
        )
    });
    assert!(barrier.wait_until_blocked(std::time::Duration::from_secs(5)));
    active.store(false, std::sync::atomic::Ordering::Release);
    barrier.release();
    assert!(matches!(
        handle.join().unwrap(),
        Err(rommfs_core::Error::Cancelled)
    ));
    assert!(!install.join("bios/bios.bin").exists());
    assert_eq!(fs::read_dir(install.join("bios")).unwrap().count(), 0);
    active.store(true, std::sync::atomic::Ordering::Release);
    fx.on(
        "GET",
        "/api/firmware/1/content/",
        ResponseSpec::Bytes {
            status: 200,
            bytes: b"short".to_vec(),
            truncate_at: None,
            stall_after_bytes: None,
        },
    );
    let report = materialize_auxiliaries(
        &RommClient::new(fx.url()).unwrap(),
        "server",
        7,
        &install,
        &[platform],
        &[],
        &active,
        |_, _| {},
    )
    .unwrap();
    assert_eq!(report.published, 0);
    assert_eq!(report.warnings.len(), 1);
    assert!(!install.join("bios/bios.bin").exists());
    assert_eq!(fs::read_dir(install.join("bios")).unwrap().count(), 0);
}
