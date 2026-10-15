//! Favourites and play-history sync: the client routes and the gamelist merge.
use rommfs_core::catalogue::build_catalogue;
use rommfs_core::retrobat::{
    build_remote_play, generate_gamelists, FrontendGamelists, MetadataIndex, MetadataPublication,
    PlayPush, PlaySync,
};
use rommfs_core::romm::{PlatformDto, RomDto, RomUserDto, RommClient};
use rommfs_fixture::{contract, FixtureServer, ResponseSpec};
use serde_json::json;
use std::collections::{BTreeMap, HashMap, HashSet};

fn rom() -> RomDto {
    let mut value = contract::rom(1, "gb", "Game.gb", 3, "hash");
    value["rom_user"] = json!({"last_played": null, "hidden": false});
    serde_json::from_value(value).unwrap()
}

fn catalogue(roms: &[RomDto]) -> rommfs_core::catalogue::Catalogue {
    let platforms: Vec<PlatformDto> =
        serde_json::from_str(&contract::platforms(&[(1, "gb", "gb", "Game Boy")])).unwrap();
    build_catalogue("s", &platforms, roms, |_| {}).unwrap()
}

fn reload(index: &MetadataIndex) -> MetadataIndex {
    serde_json::from_slice(&serde_json::to_vec(index).unwrap()).unwrap()
}

fn gamelist(publication: &MetadataPublication, system: &str) -> String {
    String::from_utf8(publication.gamelists()[&format!("{system}/gamelist.xml")].clone()).unwrap()
}

/// The client reads favourites and history and writes the local changes back.
#[test]
fn the_client_reads_favourites_and_history_and_writes_changes_back() {
    let fx = FixtureServer::start();
    fx.on(
        "GET",
        "/api/collections",
        ResponseSpec::Json {
            status: 200,
            body:
                json!([{ "id": 5, "name": "Favourites", "is_favorite": true, "rom_ids": [1, 2] }])
                    .to_string(),
        },
    );
    fx.on(
        "GET",
        "/api/play-sessions?",
        ResponseSpec::Json {
            status: 200,
            body: json!([
                {"id": 1, "rom_id": 1, "start_time": "2026-01-01T10:00:00Z", "end_time": "2026-01-01T10:30:00Z", "duration_ms": 1_800_000},
                {"id": 2, "rom_id": 1, "start_time": "2026-01-02T10:00:00Z", "end_time": "2026-01-02T10:05:00Z", "duration_ms": 300_000}
            ])
            .to_string(),
        },
    );
    fx.on(
        "POST",
        "/api/play-sessions",
        ResponseSpec::Json {
            status: 201,
            body: json!({"results": [], "created_count": 1, "skipped_count": 0}).to_string(),
        },
    );
    fx.on(
        "PUT",
        "/api/roms/",
        ResponseSpec::Json {
            status: 200,
            body: json!({}).to_string(),
        },
    );
    fx.on(
        "POST",
        "/api/collections/5/roms",
        ResponseSpec::Json {
            status: 200,
            body: json!({"id": 5, "name": "Favourites", "is_favorite": true, "rom_ids": [1, 2, 1]})
                .to_string(),
        },
    );

    let client = RommClient::new(fx.url()).unwrap();
    let collections = client.collections().unwrap();
    assert!(collections[0].is_favorite);
    let sessions = client.all_play_sessions().unwrap();
    assert_eq!(sessions.len(), 2);
    let remote = build_remote_play(&HashMap::new(), &collections[0].rom_ids, &sessions);
    assert_eq!(remote[&1].play_count, 2);
    assert_eq!(remote[&1].play_time_ms, 2_100_000);

    client.add_collection_roms(5, &[1]).unwrap();
    client.remove_collection_roms(5, &[1]).ok();
    client.set_rom_hidden(1, true).unwrap();
    client
        .ingest_play_sessions(&[rommfs_core::romm::PlaySessionUpload {
            rom_id: 1,
            start_time: "2026-01-03T10:00:00Z".into(),
            end_time: "2026-01-03T10:10:00Z".into(),
            duration_ms: 600_000,
        }])
        .unwrap();

    let requests = fx.requests();
    let bodies: Vec<String> = requests
        .iter()
        .map(|request| String::from_utf8_lossy(&request.body).to_string())
        .collect();
    assert!(bodies.iter().any(|body| body.contains("\"rom_ids\":[1]")));
    assert!(bodies.iter().any(|body| body.contains("\"hidden\":true")));
    assert!(bodies
        .iter()
        .any(|body| body.contains("\"duration_ms\":600000")));
}

/// The first reconciliation adopts the union, and a later local play becomes a
/// pushed session without recounting a session already recorded.
#[test]
fn play_data_reconciles_into_our_entries_and_pushes_new_launches() {
    let roms = vec![rom()];
    let cat = catalogue(&roms);
    let system = "<?xml version=\"1.0\"?>\n<gameList><game><path>./Local.gb</path><name>Mine</name><favorite>true</favorite></game><game><path>./RomM/Game.gb</path><favorite>true</favorite><playcount>2</playcount><lastplayed>20260102T120000</lastplayed><gametime>45</gametime></game></gameList>";
    let frontend = FrontendGamelists::new(BTreeMap::from([("gb".into(), system.into())]));

    // The favourite already exists on both sides, so nothing is pushed yet.
    let remote = build_remote_play(&HashMap::new(), &[1], &[]);
    let mut pushes: Vec<PlayPush> = Vec::new();
    let mut push = |plan: &PlayPush| {
        pushes.push(plan.clone());
        Ok(())
    };
    let first = generate_gamelists(
        &cat,
        &roms,
        "s",
        7,
        &MetadataIndex::default(),
        &frontend,
        &BTreeMap::new(),
        &HashMap::new(),
        &HashSet::new(),
        &mut PlaySync {
            enabled: true,
            remote: &remote,
            push: &mut push,
            now: 1_700_000_000,
        },
    )
    .unwrap();
    let xml = gamelist(&first, "gb");
    assert!(xml.contains("<playcount>2</playcount>"), "{xml}");
    assert!(xml.contains("<favorite>true</favorite>"), "{xml}");
    // The user's own entry is untouched.
    assert!(xml.contains("<path>./Local.gb</path>"), "{xml}");
    drop(push);
    assert!(pushes.is_empty());

    // A local launch after the first sync: one session is pushed.
    let mut launches: Vec<PlayPush> = Vec::new();
    let mut push = |plan: &PlayPush| {
        launches.push(plan.clone());
        Ok(())
    };
    let played = "<?xml version=\"1.0\"?>\n<gameList><game><path>./Local.gb</path><name>Mine</name></game><game><path>./RomM/Game.gb</path><favorite>true</favorite><playcount>3</playcount><lastplayed>20260103T120000</lastplayed><gametime>60</gametime></game></gameList>";
    let frontend = FrontendGamelists::new(BTreeMap::from([("gb".into(), played.into())]));
    let second = generate_gamelists(
        &cat,
        &roms,
        "s",
        7,
        &reload(first.index()),
        &frontend,
        &BTreeMap::new(),
        &HashMap::new(),
        &HashSet::new(),
        &mut PlaySync {
            enabled: true,
            remote: &remote,
            push: &mut push,
            now: 1_700_000_000,
        },
    )
    .unwrap();
    drop(push);
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].sessions.len(), 1);
    assert_eq!(launches[0].sessions[0].rom_id, 1);
    assert!(launches[0].sessions[0].duration_ms > 0);
    assert!(gamelist(&second, "gb").contains("<playcount>3</playcount>"));

    // Re-running against the pubished XML pushes nothing and never recounts.
    let published = gamelist(&second, "gb");
    let frontend = FrontendGamelists::new(BTreeMap::from([("gb".into(), published)]));
    let mut again: Vec<PlayPush> = Vec::new();
    let mut push = |plan: &PlayPush| {
        again.push(plan.clone());
        Ok(())
    };
    let third = generate_gamelists(
        &cat,
        &roms,
        "s",
        7,
        &reload(second.index()),
        &frontend,
        &BTreeMap::new(),
        &HashMap::new(),
        &HashSet::new(),
        &mut PlaySync {
            enabled: true,
            remote: &remote,
            push: &mut push,
            now: 1_700_000_000,
        },
    )
    .unwrap();
    drop(push);
    assert!(again.is_empty(), "{again:?}");
    assert!(third.gamelists().is_empty(), "published state is stable");
}

/// A failed push keeps the local delta, and play data never reaches the user's
/// own entries or an unreadable gamelist.
#[test]
fn a_failed_push_keeps_its_delta_and_foreign_entries_are_untouched() {
    let roms = vec![rom()];
    let cat = catalogue(&roms);
    let first_frontend = FrontendGamelists::new(BTreeMap::from([(
        "gb".into(),
        "<gameList><game><path>./Local.gb</path><favorite>true</favorite><playcount>9</playcount></game><game><path>./RomM/Game.gb</path><favorite>true</favorite><playcount>2</playcount></game></gameList>".into(),
    )]));
    let remote = build_remote_play(&HashMap::new(), &[1], &[]);
    // Establish the baseline first; that pass pushes nothing for counts.
    let first = generate_gamelists(
        &cat,
        &roms,
        "s",
        7,
        &MetadataIndex::default(),
        &first_frontend,
        &BTreeMap::new(),
        &HashMap::new(),
        &HashSet::new(),
        &mut PlaySync {
            enabled: true,
            remote: &remote,
            push: &mut |_: &PlayPush| Ok(()),
            now: 1_700_000_000,
        },
    )
    .unwrap();

    // A new local launch whose push fails keeps the delta for next time.
    let frontend = FrontendGamelists::new(BTreeMap::from([(
        "gb".into(),
        "<gameList><game><path>./Local.gb</path><favorite>true</favorite><playcount>9</playcount></game><game><path>./RomM/Game.gb</path><favorite>true</favorite><playcount>3</playcount></game></gameList>".into(),
    )]));
    let mut push = |_plan: &PlayPush| Err("server offline".to_string());
    let output = generate_gamelists(
        &cat,
        &roms,
        "s",
        7,
        &reload(first.index()),
        &frontend,
        &BTreeMap::new(),
        &HashMap::new(),
        &HashSet::new(),
        &mut PlaySync {
            enabled: true,
            remote: &remote,
            push: &mut push,
            now: 1_700_000_000,
        },
    )
    .unwrap();
    let xml = gamelist(&output, "gb");
    // The user's own entry is never rewritten.
    assert!(
        xml.contains("<path>./Local.gb</path><favorite>true</favorite><playcount>9</playcount>")
    );
    // The push failure is reported against the game.
    assert!(output
        .skipped()
        .iter()
        .any(|(_, reason)| reason.contains("play history not synced")));
    // The local delta survives: the baseline still reads 2, so it is retried.
    assert_eq!(
        reload(output.index())
            .play_baseline(1, 1)
            .map(|baseline| baseline.synced_local.playcount),
        Some(2)
    );
}

/// An unreadable gamelist is skipped, so its play data is never rewritten.
#[test]
fn an_unreadable_gamelist_keeps_its_play_data() {
    let roms = vec![rom()];
    let cat = catalogue(&roms);
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("roms/gb");
    std::fs::create_dir_all(&folder).unwrap();
    // Not UTF-8: the frontend's own file must not be touched.
    std::fs::write(folder.join("gamelist.xml"), [0xff, 0xfe, 0x00]).unwrap();
    let frontend = rommfs_core::retrobat::read_gamelists(dir.path(), &["gb".to_string()]);
    let output = generate_gamelists(
        &cat,
        &roms,
        "s",
        7,
        &MetadataIndex::default(),
        &frontend,
        &BTreeMap::new(),
        &HashMap::new(),
        &HashSet::new(),
        &mut PlaySync {
            enabled: true,
            remote: &HashMap::new(),
            push: &mut |_: &PlayPush| Ok(()),
            now: 0,
        },
    )
    .unwrap();
    assert!(!output.gamelists().contains_key("gb/gamelist.xml"));
    assert!(output.skipped().iter().any(|(system, _)| system == "gb"));
    // The frontend file is byte-for-byte what it was.
    assert_eq!(
        std::fs::read(folder.join("gamelist.xml")).unwrap(),
        [0xff, 0xfe, 0x00]
    );
}

#[test]
fn remote_user_data_supplies_last_played() {
    let rom_user = HashMap::from([(
        1,
        RomUserDto {
            last_played: Some("2026-01-02T12:00:00Z".into()),
            hidden: true,
        },
    )]);
    let remote = build_remote_play(&rom_user, &[], &[]);
    assert_eq!(remote[&1].last_played, Some(1_767_355_200));
}
