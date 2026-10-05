use super::*;
use rommfs_core::catalog::build_catalogue;
use rommfs_core::events::{channel, AppEvent, EventSink};
use rommfs_core::romm::{
    Credentials, PlatformDto, RomDto, RomFileDto, RommClient, SaveSyncIdentity,
};
use rommfs_core::save_sync::{
    map_catalogue, sha256_content_hash, MappingReport, SaveMapping, SaveSyncJournal, SaveSyncScope,
    SnapshotRecord, SnapshotState, RETROBAT_GB_SRM_PROFILE,
};
use rommfs_fixture::{FixtureBodyBarrier, FixtureSaveRecord, FixtureServer, ResponseSpec};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use super::incoming_transfer::IncomingJob;
use super::transfer::{run_transfer_worker, TransferUpdate, WorkerJob};

const ACCOUNT_ID: i64 = 42;
const ROM_ID: i64 = 7;
const FILE_ID: i64 = 70;
const SAVE_BYTES: &[u8] = b"local battery save";
const INVENTORY_PATH: &str = "/api/saves?rom_id=7&slot=rommfs-retrobat-gb-srm-v1";
const CONTENT_PATH: &str = "/api/saves/31/content?optimistic=false";

struct AgentCase {
    scope: SaveSyncScope,
    mapping: SaveMapping,
    database: PathBuf,
    spool: PathBuf,
    snapshot: Option<SnapshotRecord>,
}

fn case(directory: &Path, fixture: &FixtureServer, capture: bool) -> AgentCase {
    let install_root = directory.join("RetroBat");
    let save_root = install_root.join("saves");
    fs::create_dir_all(&save_root).unwrap();
    fs::create_dir_all(install_root.join("system")).unwrap();
    fs::create_dir_all(install_root.join("emulators/retroarch")).unwrap();
    let launcher_home = install_root.join("emulationstation/.emulationstation");
    fs::create_dir_all(&launcher_home).unwrap();
    fs::write(install_root.join("RetroBat.exe"), b"exe").unwrap();
    fs::write(install_root.join("system/version.info"), "8.2.1\n").unwrap();
    fs::write(
            install_root.join("emulators/retroarch/retroarch.cfg"),
            "savefile_directory = \":\\saves\"\nsavefiles_in_content_dir = \"false\"\nsort_savefiles_enable = \"false\"\n",
        )
        .unwrap();
    fs::write(
        install_root.join("emulationstation/emulatorLauncher.cfg"),
        "home=.\\.emulationstation\nsaves=.\\..\\saves\n",
    )
    .unwrap();
    fs::write(launcher_home.join("es_settings.cfg"), "<config/>\n").unwrap();
    fs::write(
            launcher_home.join("es_systems.cfg"),
            r#"<systemList><system><name>gb</name><command>"%HOME%\emulatorLauncher.exe" -gameinfo %GAMEINFOXML% %CONTROLLERSCONFIG% -system %SYSTEM% -emulator %EMULATOR% -core %CORE% -rom %ROM%</command><emulators><emulator name="libretro"><cores><core>gambatte</core></cores></emulator></emulators></system><system><name>snes</name></system></systemList>"#,
        )
        .unwrap();
    let platforms = [PlatformDto {
        id: 1,
        slug: "gb".into(),
        fs_slug: "gb".into(),
        name: "Game Boy".into(),
        custom_name: None,
        rom_count: 1,
    }];
    let roms = [RomDto {
        id: ROM_ID,
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
        files: vec![RomFileDto {
            id: FILE_ID,
            file_name: "Game.gb".into(),
            file_size_bytes: 1,
            last_modified: None,
            crc_hash: None,
            md5_hash: None,
            sha1_hash: None,
            is_top_level: true,
        }],
    }];
    let catalogue = build_catalogue(fixture.url(), &platforms, &roms, |_| {}).unwrap();
    let mapping = map_catalogue(&catalogue, &save_root)
        .unwrap()
        .mappings
        .remove(0);
    fs::create_dir_all(mapping.target_path.parent().unwrap()).unwrap();
    fs::write(&mapping.target_path, SAVE_BYTES).unwrap();

    let scope = SaveSyncScope {
        server_id: mapping.rom_key.server_id.clone(),
        account_id: ACCOUNT_ID.to_string(),
        installation_root: install_root,
        effective_saves_root: save_root,
    };
    let database = directory.join("settings/save-sync-journal.db");
    let spool = directory.join("settings/save-sync-snapshots");
    let mut journal = SaveSyncJournal::open(&database, &spool, scope.clone()).unwrap();
    journal
        .reconcile_mappings(&MappingReport {
            mappings: vec![mapping.clone()],
            ..MappingReport::default()
        })
        .unwrap();
    journal
        .confirm_mapping(&mapping.rom_key, &mapping.relative_path)
        .unwrap();
    let snapshot = if capture {
        let observation = journal.observe_local_save(&mapping).unwrap();
        Some(
            journal
                .capture_snapshot(
                    &mapping,
                    observation.generation,
                    observation.content_hash.as_deref().unwrap(),
                )
                .unwrap(),
        )
    } else {
        None
    };
    AgentCase {
        scope,
        mapping,
        database,
        spool,
        snapshot,
    }
}

fn authenticated_client(fixture: &FixtureServer) -> Arc<RommClient> {
    fixture.on(
        "POST",
        "/api/token",
        ResponseSpec::Json {
            status: 200,
            body: rommfs_fixture::contract::token_ok(),
        },
    );
    let client = Arc::new(RommClient::new(fixture.url()).unwrap());
    client
        .authenticate(Credentials {
            username: "fixture-user",
            password: "fixture-password",
        })
        .unwrap();
    client
}

fn start_agent(
    case: &AgentCase,
    fixture: &FixtureServer,
    debounce_secs: u32,
    enabled: Arc<AtomicBool>,
    sink: EventSink,
) -> SaveSyncAgent {
    start_agent_with_mappings(
        case,
        vec![case.mapping.clone()],
        fixture,
        debounce_secs,
        enabled,
        sink,
    )
}

fn start_agent_with_mappings(
    case: &AgentCase,
    mappings: Vec<SaveMapping>,
    fixture: &FixtureServer,
    debounce_secs: u32,
    enabled: Arc<AtomicBool>,
    sink: EventSink,
) -> SaveSyncAgent {
    SaveSyncAgent::start(
        case.scope.clone(),
        mappings,
        rommfs_core::romm::SaveSyncIdentity {
            account_id: ACCOUNT_ID,
            scopes: vec!["assets.read".into(), "assets.write".into()],
        },
        authenticated_client(fixture),
        case.database.clone(),
        case.spool.clone(),
        debounce_secs,
        1,
        enabled,
        sink,
    )
    .unwrap()
}

fn remote_save(revision: &str, id: i64) -> String {
    remote_save_with_bytes(revision, id, SAVE_BYTES)
}

fn remote_save_with_bytes(revision: &str, id: i64, bytes: &[u8]) -> String {
    json!({
        "id": id,
        "rom_id": ROM_ID,
        "user_id": ACCOUNT_ID,
        "file_name": format!("rommfs-{revision} [2026-10-05_12-34-56].srm"),
        "file_size_bytes": bytes.len(),
        "missing_from_fs": false,
        "created_at": "2026-10-05T12:34:56Z",
        "updated_at": "2026-10-05T12:34:56Z",
        "emulator": "retroarch-gambatte",
        "slot": RETROBAT_GB_SRM_PROFILE,
    })
    .to_string()
}

fn install_content_route(fixture: &FixtureServer) {
    fixture.on(
        "GET",
        CONTENT_PATH,
        ResponseSpec::Bytes {
            status: 200,
            bytes: SAVE_BYTES.to_vec(),
            truncate_at: None,
            stall_after_bytes: None,
        },
    );
}

fn seed_remote(fixture: &FixtureServer, id: i64, revision: &str, bytes: &[u8], timestamp: &str) {
    fixture.seed_save(FixtureSaveRecord {
        id,
        rom_id: ROM_ID,
        user_id: ACCOUNT_ID,
        file_name: format!("rommfs-{revision} [2026-10-05_12-34-56].srm"),
        file_size_bytes: bytes.len(),
        slot: RETROBAT_GB_SRM_PROFILE.into(),
        bytes: bytes.to_vec(),
        created_at: timestamp.into(),
        updated_at: timestamp.into(),
    });
}

fn second_game_mapping(case: &AgentCase) -> SaveMapping {
    let platform = PlatformDto {
        id: 1,
        slug: "gb".into(),
        fs_slug: "gb".into(),
        name: "Game Boy".into(),
        custom_name: None,
        rom_count: 1,
    };
    let rom = RomDto {
        id: 8,
        platform_fs_slug: "gb".into(),
        platform_slug: "gb".into(),
        fs_name: "Healthy.gb".into(),
        fs_size_bytes: 1,
        has_simple_single_file: true,
        has_nested_single_file: false,
        has_multiple_files: false,
        missing_from_fs: false,
        is_physical: false,
        updated_at: String::new(),
        files: vec![RomFileDto {
            id: 80,
            file_name: "Healthy.gb".into(),
            file_size_bytes: 1,
            last_modified: None,
            crc_hash: None,
            md5_hash: None,
            sha1_hash: None,
            is_top_level: true,
        }],
    };
    let catalogue = build_catalogue(&case.scope.server_id, &[platform], &[rom], |_| {}).unwrap();
    map_catalogue(&catalogue, &case.scope.effective_saves_root)
        .unwrap()
        .mappings
        .remove(0)
}

fn accept_remote_baseline(case: &AgentCase, remote_id: &str, bytes: &[u8]) {
    let mut journal =
        SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let observation = journal.observe_local_save(&case.mapping).unwrap();
    let id = remote_id.to_owned();
    journal
        .record_reconciled_baseline(
            &case.mapping.rom_key,
            observation.content_hash.as_deref(),
            Some(remote_id),
            Some(&sha256_content_hash(bytes)),
            &[id],
        )
        .unwrap();
}

fn wait_for_log(
    events: &Receiver<AppEvent>,
    timeout: Duration,
    predicate: impl Fn(&str) -> bool,
) -> Option<String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match events.recv_timeout(Duration::from_millis(50)) {
            Ok(AppEvent::Log(line)) if predicate(&line.message) => return Some(line.message),
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return None,
        }
    }
    None
}

fn wait_for_queue_status(
    events: &Receiver<AppEvent>,
    timeout: Duration,
    predicate: impl Fn(&rommfs_core::events::SaveSyncQueueStatus) -> bool,
) -> Option<rommfs_core::events::SaveSyncQueueStatus> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match events.recv_timeout(Duration::from_millis(50)) {
            Ok(AppEvent::SaveSyncQueueUpdated(status)) if predicate(&status) => {
                return Some(status)
            }
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return None,
        }
    }
    None
}

fn snapshot_after_stop(case: &AgentCase) -> (SnapshotRecord, rommfs_core::save_sync::JournalSlot) {
    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let snapshot = journal.snapshots().unwrap().into_iter().last().unwrap();
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    (snapshot, slot)
}

#[test]
fn production_agent_uploads_only_after_inventory_and_byte_readback() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    let case = case(directory.path(), &fixture, true);
    let (sink, events) = channel();
    let enabled = Arc::new(AtomicBool::new(true));
    let mut agent = start_agent(&case, &fixture, 0, enabled, sink);

    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("verified upload revision")
    })
    .is_some());
    agent.stop();

    let (snapshot, slot) = snapshot_after_stop(&case);
    assert_eq!(snapshot.state, SnapshotState::RemoteComplete);
    assert_eq!(snapshot.remote_slot_id.as_deref(), Some("31"));
    assert_eq!(slot.remote_slot_id.as_deref(), Some("31"));
    assert_eq!(
        slot.remote_baseline_hash.as_deref(),
        Some(snapshot.content_hash.as_str())
    );
    assert_eq!(fixture.count_requests("GET", INVENTORY_PATH), 3);
    assert_eq!(
        fixture.count_requests("POST", "/api/saves?rom_id=7&slot="),
        1
    );
    assert_eq!(fixture.count_requests("GET", CONTENT_PATH), 1);
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);
    assert!(fixture
        .requests()
        .iter()
        .filter(|request| request.target != "/api/token")
        .all(|request| request
            .headers
            .get("authorization")
            .is_some_and(|value| value == "Bearer fixture-access-token")));
}

#[test]
fn queue_status_counts_an_uncaptured_local_generation_as_pending_outbound() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let case = case(directory.path(), &fixture, false);
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 3600, Arc::new(AtomicBool::new(true)), sink);

    let status = wait_for_queue_status(&events, Duration::from_secs(5), |status| {
        status.mapped_games == 1 && status.pending_outbound > 0
    })
    .expect("dirty local generation should be visible in queue status before capture");
    assert_eq!(status.reconciled_games, 0);
    assert_eq!(status.pending_outbound, 1);
    assert!(
        SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone())
            .unwrap()
            .snapshots()
            .unwrap()
            .is_empty()
    );
    agent.stop();
}

#[test]
fn reconciliation_failure_for_one_game_survives_another_games_success() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    fixture.on(
        "GET",
        "/api/saves?rom_id=7&slot=",
        ResponseSpec::Json {
            status: 400,
            body: r#"{"detail":"game-specific inventory failure"}"#.into(),
        },
    );
    let case = case(directory.path(), &fixture, false);
    let other_mapping = second_game_mapping(&case);
    let (sink, events) = channel();
    let mut agent = start_agent_with_mappings(
        &case,
        vec![case.mapping.clone(), other_mapping],
        &fixture,
        3600,
        Arc::new(AtomicBool::new(true)),
        sink,
    );

    let status = wait_for_queue_status(&events, Duration::from_secs(5), |status| {
        status.mapped_games == 2 && status.reconciled_games == 1 && status.failure.is_some()
    })
    .expect("game 7 failure should remain visible after game 8 succeeds");
    assert!(status.failure.as_deref().unwrap().contains("400"));
    assert!(status.games.iter().any(|game| game.rom_id == ROM_ID
        && game
            .issue
            .as_deref()
            .is_some_and(|issue| issue.contains("400"))));
    assert!(status
        .games
        .iter()
        .any(|game| game.rom_id == 8 && game.issue.is_none()));
    agent.stop();
}

#[test]
fn one_game_upload_retry_remains_visible_after_another_game_verifies() {
    const SECOND_GAME_SAVE: &[u8] = b"healthy game save";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    fixture.on_sequence(
        "POST",
        "/api/saves?rom_id=7&slot=",
        vec![
            ResponseSpec::JsonWithHeaders {
                status: 503,
                body: r#"{"detail":"temporarily unavailable"}"#.into(),
                headers: vec![("Retry-After".into(), "1".into())],
            },
            ResponseSpec::RomMSaveUpload {
                status: 201,
                user_id: ACCOUNT_ID,
            },
        ],
    );
    let case = case(directory.path(), &fixture, true);
    let other_mapping = second_game_mapping(&case);
    fs::create_dir_all(other_mapping.target_path.parent().unwrap()).unwrap();
    fs::write(&other_mapping.target_path, SECOND_GAME_SAVE).unwrap();
    let (sink, events) = channel();
    let mut agent = start_agent_with_mappings(
        &case,
        vec![case.mapping.clone(), other_mapping],
        &fixture,
        0,
        Arc::new(AtomicBool::new(true)),
        sink,
    );

    let status = wait_for_queue_status(&events, Duration::from_secs(8), |status| {
        status
            .failure
            .as_deref()
            .is_some_and(|failure| failure.contains("503"))
            && status.pending_outbound > 0
            && fixture.saved_saves().iter().any(|save| save.rom_id == 8)
    })
    .expect("game 7's retry should remain visible while game 8 succeeds");
    let failed_game = status
        .games
        .iter()
        .find(|game| game.rom_id == ROM_ID)
        .unwrap();
    assert!(failed_game
        .issue
        .as_deref()
        .is_some_and(|issue| issue.contains("503")));
    assert!(status
        .games
        .iter()
        .find(|game| game.rom_id == 8)
        .unwrap()
        .issue
        .is_none());
    assert_eq!(
        fixture.count_requests("POST", "/api/saves?rom_id=7&slot="),
        1
    );
    assert_eq!(
        fixture.count_requests("POST", "/api/saves?rom_id=8&slot="),
        1
    );
    let recovered_status = wait_for_queue_status(&events, Duration::from_secs(8), |status| {
        status.failure.is_none() && status.pending_outbound == 0 && fixture.saved_saves().len() == 2
    })
    .expect("the retry status should clear after its revision verifies");
    assert!(recovered_status
        .games
        .iter()
        .find(|game| game.rom_id == ROM_ID)
        .unwrap()
        .issue
        .is_none());
    assert_eq!(
        fixture.count_requests("POST", "/api/saves?rom_id=7&slot="),
        2
    );
    agent.stop();
}

#[test]
fn terminal_actor_failure_emits_session_queue_with_durable_counts() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let case = case(directory.path(), &fixture, true);
    let (sink, events) = channel();
    let agent = SaveSyncAgent::start(
        case.scope.clone(),
        vec![case.mapping.clone()],
        SaveSyncIdentity {
            account_id: ACCOUNT_ID,
            scopes: vec!["assets.read".into()],
        },
        authenticated_client(&fixture),
        case.database.clone(),
        case.spool.clone(),
        0,
        1,
        Arc::new(AtomicBool::new(true)),
        sink,
    )
    .unwrap();

    let status = wait_for_queue_status(&events, Duration::from_secs(5), |status| {
        status.actor_failed
    })
    .expect("a stopped actor must emit its terminal failure status");
    assert_eq!(status.session_id, 1);
    assert_eq!(status.mapped_games, 1);
    assert_eq!(status.reconciled_games, 0);
    assert_eq!(status.pending_outbound, 1);
    assert!(status.failure.as_deref().unwrap().contains("assets.write"));
    drop(agent);
}

#[test]
fn real_filesystem_notification_drives_the_agent_without_waiting_for_fallback_scan() {
    const NEXT_SAVE: &[u8] = b"native watcher save generation";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    let case = case(directory.path(), &fixture, false);
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("verified upload revision")
    })
    .is_some());
    let initial_posts = fixture.count_requests("POST", "/api/saves?rom_id=7&slot=");
    assert_eq!(initial_posts, 1);

    fs::write(&case.mapping.target_path, NEXT_SAVE).unwrap();
    assert!(wait_for_log(&events, Duration::from_secs(10), |message| {
        message.contains("verified upload revision")
    })
    .is_some());
    agent.stop();

    assert_eq!(
        fixture.count_requests("POST", "/api/saves?rom_id=7&slot="),
        2
    );
    assert_eq!(fixture.saved_saves()[1].bytes, NEXT_SAVE);
}

#[test]
fn successive_revisions_wait_for_the_prior_baseline_to_be_verified() {
    const NEXT_SAVE_BYTES: &[u8] = b"other battery save";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    let case = case(directory.path(), &fixture, true);
    let first = case.snapshot.as_ref().unwrap();
    let mut journal =
        SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    fs::write(&case.mapping.target_path, NEXT_SAVE_BYTES).unwrap();
    let observation = journal.observe_local_save(&case.mapping).unwrap();
    let second = journal
        .capture_snapshot(
            &case.mapping,
            observation.generation,
            observation.content_hash.as_deref().unwrap(),
        )
        .unwrap();
    drop(journal);

    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);

    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains(&format!("verified upload revision {}", second.revision))
    })
    .is_some());
    agent.stop();

    let (latest, slot) = snapshot_after_stop(&case);
    assert_eq!(latest.state, SnapshotState::RemoteComplete);
    assert_eq!(latest.revision, second.revision);
    assert_eq!(slot.remote_slot_id.as_deref(), Some("32"));
    assert_eq!(
        fixture.count_requests("POST", "/api/saves?rom_id=7&slot="),
        2
    );
    assert_eq!(fixture.count_requests("GET", INVENTORY_PATH), 5);
    assert_eq!(fixture.count_requests("GET", CONTENT_PATH), 2);
    assert_eq!(
        fixture.count_requests("GET", "/api/saves/32/content?optimistic=false"),
        1
    );
    let remote_saves = fixture.saved_saves();
    assert_eq!(remote_saves.len(), 2);
    assert_eq!(
        remote_saves[0].file_name,
        format!("rommfs-{} [2026-10-05_12-34-56].srm", first.revision)
    );
    assert_eq!(
        remote_saves[1].file_name,
        format!("rommfs-{} [2026-10-05_12-34-56].srm", second.revision)
    );
    assert_ne!(first.revision, second.revision);
    assert_eq!(remote_saves[0].bytes, SAVE_BYTES);
    assert_eq!(remote_saves[1].bytes, NEXT_SAVE_BYTES);
    assert_eq!(
        fs::read(&case.mapping.target_path).unwrap(),
        NEXT_SAVE_BYTES
    );
}

#[test]
fn local_write_while_first_upload_response_is_blocked_is_captured_and_uploaded_next() {
    const NEXT_SAVE_BYTES: &[u8] = b"write made during the first HTTP upload";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    let barrier = FixtureBodyBarrier::new();
    fixture.on_sequence(
        "POST",
        "/api/saves?rom_id=7&slot=",
        vec![
            ResponseSpec::HeldRomMSaveUpload {
                user_id: ACCOUNT_ID,
                barrier: barrier.clone(),
            },
            ResponseSpec::RomMSaveUpload {
                status: 201,
                user_id: ACCOUNT_ID,
            },
        ],
    );
    let case = case(directory.path(), &fixture, true);
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(barrier.wait_until_blocked(Duration::from_secs(5)));

    // The fixture has consumed and parsed the real first POST body, but is
    // holding its response. The actor remains free to watch and journal this
    // later emulator generation while the single HTTP worker is blocked.
    fs::write(&case.mapping.target_path, NEXT_SAVE_BYTES).unwrap();
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("captured local revision")
    })
    .is_some());
    barrier.release();
    for _ in 0..2 {
        assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
            message.contains("verified upload revision")
        })
        .is_some());
    }
    agent.stop();

    let remote = fixture.saved_saves();
    assert_eq!(remote.len(), 2);
    assert_eq!(remote[0].bytes, SAVE_BYTES);
    assert_eq!(remote[1].bytes, NEXT_SAVE_BYTES);
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 2);
    assert_eq!(
        fs::read(&case.mapping.target_path).unwrap(),
        NEXT_SAVE_BYTES
    );
}

#[test]
fn ambiguous_post_is_reconciled_after_restart_without_duplicate_upload() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 503);
    let case = case(directory.path(), &fixture, true);
    let (sink, events) = channel();
    let enabled = Arc::new(AtomicBool::new(true));
    let mut first_agent = start_agent(&case, &fixture, 0, Arc::clone(&enabled), sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("503")
    })
    .is_some());
    first_agent.stop();
    let (interrupted, _) = snapshot_after_stop(&case);
    assert_eq!(interrupted.state, SnapshotState::RemoteAmbiguous);

    let (sink, events) = channel();
    let mut restarted_agent = start_agent(&case, &fixture, 0, enabled, sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("reconciled saves for")
    })
    .is_some());
    restarted_agent.stop();

    let (recovered, _) = snapshot_after_stop(&case);
    assert_eq!(recovered.state, SnapshotState::RemoteComplete);
    assert_eq!(
        fixture.count_requests("POST", "/api/saves?rom_id=7&slot="),
        1
    );
    assert_eq!(fixture.count_requests("GET", CONTENT_PATH), 1);
    let stored = fixture.saved_saves();
    assert_eq!(stored.len(), 1);
    assert_eq!(
        stored[0].file_name,
        format!("rommfs-{} [2026-10-05_12-34-56].srm", recovered.revision)
    );
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);
}

#[test]
fn ambiguous_older_snapshot_is_verified_without_uploading_over_a_newer_conflict() {
    const NEWER_SAVE: &[u8] = b"newer local generation";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 503);
    let case = case(directory.path(), &fixture, true);
    let older = case.snapshot.as_ref().unwrap().clone();
    let enabled = Arc::new(AtomicBool::new(true));

    let (sink, events) = channel();
    let mut first_agent = start_agent(&case, &fixture, 0, Arc::clone(&enabled), sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("503")
    })
    .is_some());
    first_agent.stop();

    fs::write(&case.mapping.target_path, NEWER_SAVE).unwrap();
    let mut journal =
        SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let observation = journal.observe_local_save(&case.mapping).unwrap();
    assert_ne!(observation.generation, older.generation);
    let newer = journal
        .capture_snapshot(
            &case.mapping,
            observation.generation,
            observation.content_hash.as_deref().unwrap(),
        )
        .unwrap();
    drop(journal);

    let (sink, events) = channel();
    let mut restarted_agent = start_agent(&case, &fixture, 0, enabled, sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("first sync found different local and remote saves")
    })
    .is_some());
    restarted_agent.stop();

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let snapshots = journal.snapshots().unwrap();
    let recovered_older = snapshots
        .iter()
        .find(|snapshot| snapshot.revision == older.revision)
        .unwrap();
    let still_conflicted_newer = snapshots
        .iter()
        .find(|snapshot| snapshot.revision == newer.revision)
        .unwrap();
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    assert_eq!(recovered_older.state, SnapshotState::RemoteComplete);
    assert_eq!(recovered_older.generation, older.generation);
    assert_eq!(recovered_older.content_hash, older.content_hash);
    assert_eq!(recovered_older.remote_slot_id.as_deref(), Some("31"));
    assert_eq!(still_conflicted_newer.state, SnapshotState::Ready);
    assert!(slot.needs_attention);
    let expected_local_hash = sha256_content_hash(NEWER_SAVE);
    assert_eq!(
        slot.current_local_hash.as_deref(),
        Some(expected_local_hash.as_str())
    );
    let expected_baseline_hash = sha256_content_hash(SAVE_BYTES);
    assert_eq!(
        slot.local_baseline_hash.as_deref(),
        Some(expected_baseline_hash.as_str())
    );
    assert_eq!(slot.remote_slot_id.as_deref(), Some("31"));
    assert_eq!(
        slot.remote_baseline_hash.as_deref(),
        Some(older.content_hash.as_str())
    );
    assert_eq!(journal.incoming_saves().unwrap().len(), 1);
    assert_eq!(
        fixture.count_requests("POST", "/api/saves?rom_id=7&slot="),
        1
    );
    assert_eq!(fixture.saved_saves().len(), 1);
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), NEWER_SAVE);
}

#[test]
fn ambiguous_accepted_snapshot_does_not_resurrect_a_locally_removed_save() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 503);
    let case = case(directory.path(), &fixture, true);
    let accepted = case.snapshot.as_ref().unwrap().clone();
    let enabled = Arc::new(AtomicBool::new(true));

    let (sink, events) = channel();
    let mut first_agent = start_agent(&case, &fixture, 0, Arc::clone(&enabled), sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("503")
    })
    .is_some());
    first_agent.stop();
    fs::remove_file(&case.mapping.target_path).unwrap();

    let (sink, events) = channel();
    let mut restarted_agent = start_agent(&case, &fixture, 0, enabled, sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("tracked local save was removed")
    })
    .is_some());
    restarted_agent.stop();

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let recovered = journal
        .snapshots()
        .unwrap()
        .into_iter()
        .find(|snapshot| snapshot.revision == accepted.revision)
        .unwrap();
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    assert_eq!(recovered.state, SnapshotState::RemoteComplete);
    assert_eq!(recovered.remote_slot_id.as_deref(), Some("31"));
    assert!(slot.local_removed);
    assert!(slot.needs_attention);
    assert_eq!(journal.incoming_saves().unwrap().len(), 1);
    assert!(!case.mapping.target_path.exists());
    assert_eq!(
        fixture.count_requests("POST", "/api/saves?rom_id=7&slot="),
        1
    );
}

#[test]
fn malformed_success_response_is_treated_as_an_ambiguous_post() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let case = case(directory.path(), &fixture, true);
    let snapshot = case.snapshot.as_ref().unwrap();
    let saved_json = remote_save(&snapshot.revision, 31);
    fixture.on_sequence(
        "GET",
        INVENTORY_PATH,
        vec![
            ResponseSpec::Json {
                status: 200,
                body: "[]".into(),
            },
            ResponseSpec::Json {
                status: 200,
                body: "[]".into(),
            },
            ResponseSpec::Json {
                status: 200,
                body: format!("[{saved_json}]"),
            },
        ],
    );
    fixture.on(
        "POST",
        "/api/saves?rom_id=7&slot=",
        ResponseSpec::Json {
            status: 201,
            body: "not a save response".into(),
        },
    );
    install_content_route(&fixture);
    let (sink, events) = channel();
    let enabled = Arc::new(AtomicBool::new(true));
    let mut first_agent = start_agent(&case, &fixture, 0, Arc::clone(&enabled), sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("invalid catalogue payload")
    })
    .is_some());
    first_agent.stop();
    let (interrupted, _) = snapshot_after_stop(&case);
    assert_eq!(interrupted.state, SnapshotState::RemoteAmbiguous);

    let (sink, events) = channel();
    let mut restarted_agent = start_agent(&case, &fixture, 0, enabled, sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("reconciled saves for")
    })
    .is_some());
    restarted_agent.stop();

    let (recovered, _) = snapshot_after_stop(&case);
    assert_eq!(recovered.state, SnapshotState::RemoteComplete);
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 1);
    assert_eq!(fixture.count("GET", CONTENT_PATH), 1);
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);
}

#[test]
fn preexisting_remote_save_requires_attention_and_is_never_overwritten() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let case = case(directory.path(), &fixture, true);
    fixture.on(
        "GET",
        INVENTORY_PATH,
        ResponseSpec::Json {
            status: 200,
            body: format!(
                "[{}]",
                json!({
                    "id": 91,
                    "rom_id": ROM_ID,
                    "user_id": ACCOUNT_ID,
                    "file_name": "another-user-save.srm",
                    "file_size_bytes": SAVE_BYTES.len(),
                    "slot": RETROBAT_GB_SRM_PROFILE,
                })
            ),
        },
    );
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("outside the verified owned revision format")
    })
    .is_some());
    agent.stop();

    let (snapshot, slot) = snapshot_after_stop(&case);
    assert_eq!(snapshot.state, SnapshotState::Ready);
    assert!(slot.needs_attention);
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);
}

#[test]
fn upload_metadata_must_match_the_exact_revision_identifier() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let case = case(directory.path(), &fixture, true);
    let snapshot = case.snapshot.as_ref().unwrap();
    let expected_filename = format!("rommfs-{} [2026-10-05_12-34-56].srm", snapshot.revision);
    let invalid_response =
        remote_save(&snapshot.revision, 31).replace(&expected_filename, "different-save.srm");
    fixture.on(
        "GET",
        INVENTORY_PATH,
        ResponseSpec::Json {
            status: 200,
            body: "[]".into(),
        },
    );
    fixture.on(
        "POST",
        "/api/saves?rom_id=7&slot=",
        ResponseSpec::Json {
            status: 201,
            body: invalid_response,
        },
    );
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);

    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("filename")
    })
    .is_some());
    agent.stop();

    let (snapshot, slot) = snapshot_after_stop(&case);
    assert_eq!(snapshot.state, SnapshotState::Failed);
    assert!(slot.needs_attention);
    assert_eq!(fixture.count("GET", CONTENT_PATH), 0);
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);
}

#[test]
fn runtime_debounce_delays_inventory_and_keeps_conflicting_local_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let case = case(directory.path(), &fixture, false);
    fixture.on(
        "GET",
        INVENTORY_PATH,
        ResponseSpec::Json {
            status: 200,
            body: format!(
                "[{}]",
                json!({
                    "id": 92,
                    "rom_id": ROM_ID,
                    "user_id": ACCOUNT_ID,
                    "file_name": "already-on-server.srm",
                    "file_size_bytes": SAVE_BYTES.len(),
                    "slot": RETROBAT_GB_SRM_PROFILE,
                })
            ),
        },
    );
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 1, Arc::new(AtomicBool::new(true)), sink);

    assert!(wait_for_log(&events, Duration::from_secs(4), |message| {
        message.contains("outside the verified owned revision format")
    })
    .is_some());
    assert_eq!(fixture.count_requests("GET", INVENTORY_PATH), 1);
    agent.stop();

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    assert!(journal.snapshots().unwrap().is_empty());
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    assert!(slot.needs_attention);
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);
}

#[test]
fn startup_restores_a_durable_generation_that_was_not_yet_snapshotted() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let case = case(directory.path(), &fixture, false);
    let mut journal =
        SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    assert!(journal.observe_local_save(&case.mapping).unwrap().changed);
    drop(journal);
    fixture.on(
        "GET",
        INVENTORY_PATH,
        ResponseSpec::Json {
            status: 200,
            body: format!(
                "[{}]",
                json!({
                    "id": 93,
                    "rom_id": ROM_ID,
                    "user_id": ACCOUNT_ID,
                    "file_name": "preexisting-save.srm",
                    "file_size_bytes": SAVE_BYTES.len(),
                    "slot": RETROBAT_GB_SRM_PROFILE,
                })
            ),
        },
    );
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);

    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("outside the verified owned revision format")
    })
    .is_some());
    agent.stop();

    let (snapshot, slot) = snapshot_after_stop(&case);
    assert_eq!(snapshot.state, SnapshotState::Ready);
    assert!(slot.needs_attention);
    assert_eq!(fixture.count("GET", INVENTORY_PATH), 1);
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);
}

#[test]
fn disabled_consent_does_not_start_watch_or_offer_a_durable_revision() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let case = case(directory.path(), &fixture, true);
    let (sink, _events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(false)), sink);
    thread::sleep(Duration::from_millis(200));
    agent.stop();

    let (snapshot, _) = snapshot_after_stop(&case);
    assert_eq!(snapshot.state, SnapshotState::Ready);
    assert_eq!(fixture.count("GET", INVENTORY_PATH), 0);
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);
}

#[test]
fn inventory_retry_honors_retry_after_without_losing_the_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let case = case(directory.path(), &fixture, true);
    let snapshot = case.snapshot.as_ref().unwrap();
    let saved_json = remote_save(&snapshot.revision, 31);
    fixture.on_sequence(
        "GET",
        INVENTORY_PATH,
        vec![
            ResponseSpec::JsonWithHeaders {
                status: 503,
                body: r#"{"detail":"temporarily unavailable"}"#.into(),
                headers: vec![("Retry-After".into(), "1".into())],
            },
            ResponseSpec::Json {
                status: 200,
                body: "[]".into(),
            },
            ResponseSpec::Json {
                status: 200,
                body: format!("[{saved_json}]"),
            },
        ],
    );
    fixture.on(
        "POST",
        "/api/saves?rom_id=7&slot=",
        ResponseSpec::Json {
            status: 201,
            body: saved_json,
        },
    );
    install_content_route(&fixture);
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(wait_for_log(&events, Duration::from_secs(3), |message| {
        message.contains("temporarily unavailable")
    })
    .is_some());
    thread::sleep(Duration::from_millis(350));
    assert_eq!(fixture.count("GET", INVENTORY_PATH), 1);
    assert!(wait_for_log(&events, Duration::from_secs(4), |message| {
        message.contains("verified upload revision")
    })
    .is_some());
    agent.stop();

    let (snapshot, _) = snapshot_after_stop(&case);
    assert_eq!(snapshot.state, SnapshotState::RemoteComplete);
    assert_eq!(fixture.count("GET", INVENTORY_PATH), 4);
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);
}

#[test]
fn revision_matching_accepts_only_exact_app_uuid_and_verified_server_tag() {
    let revision = "550e8400-e29b-41d4-a716-446655440000";
    assert!(remote_filename_matches_revision(
        &format!("rommfs-{revision}.srm"),
        revision
    ));
    assert!(remote_filename_matches_revision(
        &format!("rommfs-{revision} [2026-10-05_12-34-56].srm"),
        revision
    ));
    for filename in [
        format!("prefix-rommfs-{revision} [2026-10-05_12-34-56].srm"),
        format!("rommfs-{revision} [2026-10-5_12-34-56].srm"),
        format!("rommfs-{revision} [2026-10-05 12-34-56].srm"),
        format!("rommfs-{revision} [2026-10-05_12-34-56].srm.backup"),
        "rommfs-550e8400-e29b-41d4-a716-446655440001 [2026-10-05_12-34-56].srm".into(),
    ] {
        assert!(
            !remote_filename_matches_revision(&filename, revision),
            "unexpected revision adoption: {filename}"
        );
    }
}

#[test]
fn runtime_revalidation_rejects_redirected_root_or_changed_mapping_and_identity() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let case = case(directory.path(), &fixture, false);
    let identity = SaveSyncIdentity {
        account_id: ACCOUNT_ID,
        scopes: vec!["assets.read".into(), "assets.write".into()],
    };
    validate_configuration(&case.scope, std::slice::from_ref(&case.mapping), &identity).unwrap();

    fs::create_dir_all(case.scope.installation_root.join("redirected-saves")).unwrap();
    fs::write(
        case.scope
            .installation_root
            .join("emulationstation/emulatorLauncher.cfg"),
        "home=.\\.emulationstation\nsaves=redirected-saves\n",
    )
    .unwrap();
    assert!(
        validate_configuration(&case.scope, std::slice::from_ref(&case.mapping), &identity)
            .unwrap_err()
            .to_string()
            .contains("root changed")
    );

    fs::write(
        case.scope
            .installation_root
            .join("emulationstation/emulatorLauncher.cfg"),
        "home=.\\.emulationstation\nsaves=.\\..\\saves\n",
    )
    .unwrap();
    let mut changed_mapping = case.mapping.clone();
    changed_mapping.target_path = case.scope.effective_saves_root.join("gb/Other.srm");
    assert!(validate_configuration(&case.scope, &[changed_mapping], &identity).is_err());

    let wrong_identity = SaveSyncIdentity {
        account_id: ACCOUNT_ID + 1,
        scopes: identity.scopes.clone(),
    };
    assert!(validate_configuration(
        &case.scope,
        std::slice::from_ref(&case.mapping),
        &wrong_identity
    )
    .is_err());
}

#[test]
fn identical_first_sync_establishes_a_baseline_without_posting_or_replacing_local_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        SAVE_BYTES,
        "2026-10-05T12:34:56Z",
    );
    let case = case(directory.path(), &fixture, false);
    let original = fs::read(&case.mapping.target_path).unwrap();
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);

    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("reconciled saves for Game.gb")
    })
    .is_some());
    agent.stop();

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), original);
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
    assert_eq!(slot.remote_slot_id.as_deref(), Some("31"));
    assert_eq!(slot.local_baseline_hash, slot.remote_baseline_hash);
    assert!(journal.incoming_saves().unwrap().is_empty());
}

#[test]
fn different_first_sync_keeps_both_and_explicit_export_is_separate_and_no_clobber() {
    const REMOTE_BYTES: &[u8] = b"remote first-sync battery";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        REMOTE_BYTES,
        "2026-10-05T12:34:56Z",
    );
    let case = case(directory.path(), &fixture, false);
    let original_local = fs::read(&case.mapping.target_path).unwrap();
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("first sync found different local and remote saves")
    })
    .is_some());

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let incoming = journal.incoming_saves().unwrap();
    assert_eq!(incoming.len(), 1);
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), original_local);
    assert_eq!(fs::read(&incoming[0].path).unwrap(), REMOTE_BYTES);
    drop(journal);
    let export = directory.path().join("reviewed-remote.srm");
    agent
        .export_incoming(&incoming[0].id, export.clone())
        .unwrap();
    assert_eq!(fs::read(&export).unwrap(), REMOTE_BYTES);
    assert!(agent
        .export_incoming(&incoming[0].id, export.clone())
        .is_err());
    assert_eq!(fs::read(&export).unwrap(), REMOTE_BYTES);
    agent.stop();

    let outbound_before = fixture.count_requests("POST", "/api/saves?rom_id=7&slot=");
    let inventory_before = fixture.count_requests("GET", INVENTORY_PATH);
    let offline_export = directory
        .path()
        .join("rommfs-incoming-reviewed.rommfs-incoming");
    export_pending_incoming(
        &case.scope,
        std::slice::from_ref(&case.mapping),
        &SaveSyncIdentity {
            account_id: ACCOUNT_ID,
            scopes: Vec::new(),
        },
        &case.database,
        &case.spool,
        &incoming[0].id,
        &offline_export,
    )
    .unwrap();
    assert_eq!(fs::read(&offline_export).unwrap(), REMOTE_BYTES);
    assert_eq!(
        fixture.count_requests("POST", "/api/saves?rom_id=7&slot="),
        outbound_before
    );
    assert_eq!(
        fixture.count_requests("GET", INVENTORY_PATH),
        inventory_before
    );

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    assert_eq!(journal.incoming_saves().unwrap().len(), 1);
    assert!(
        journal
            .slot(&case.mapping.rom_key)
            .unwrap()
            .unwrap()
            .needs_attention
    );
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
}

#[test]
fn invalid_export_components_and_redirected_parent_fail_before_filesystem_mutation() {
    const REMOTE_BYTES: &[u8] = b"private staged save for export validation";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    let case = case(directory.path(), &fixture, false);
    let export_root = directory.path().join("exports");
    fs::create_dir_all(&export_root).unwrap();
    let staged = case.spool.join("export-validation.srm");
    fs::write(&staged, REMOTE_BYTES).unwrap();
    let expected_hash = sha256_content_hash(REMOTE_BYTES);

    for filename in [
        "unsafe.srm:stream",
        "CON.srm",
        "CONIN$.srm",
        "trailing-dot..",
        "trailing-space.srm ",
    ] {
        let result = super::files::export_incoming(
            &staged,
            &export_root.join(filename),
            &expected_hash,
            &case.scope,
            std::slice::from_ref(&case.mapping.target_path),
        );
        assert!(result.is_err(), "accepted invalid name: {filename:?}");
    }
    let traversal = export_root.join("..").join("escaped-export.srm");
    assert!(super::files::export_incoming(
        &staged,
        &traversal,
        &expected_hash,
        &case.scope,
        std::slice::from_ref(&case.mapping.target_path),
    )
    .is_err());
    assert_eq!(fs::read_dir(&export_root).unwrap().count(), 0);
    assert!(!directory.path().join("escaped-export.srm").exists());
    let missing_parent = directory.path().join("missing-export-parent");
    assert!(super::files::export_incoming(
        &staged,
        &missing_parent.join("remote.srm"),
        &expected_hash,
        &case.scope,
        std::slice::from_ref(&case.mapping.target_path),
    )
    .is_err());
    assert!(!missing_parent.exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let redirected = directory.path().join("redirected-parent");
        let external = directory.path().join("outside-export");
        fs::create_dir(&external).unwrap();
        symlink(&external, &redirected).unwrap();
        assert!(super::files::export_incoming(
            &staged,
            &redirected.join("remote.srm"),
            &expected_hash,
            &case.scope,
            std::slice::from_ref(&case.mapping.target_path),
        )
        .is_err());
        assert_eq!(fs::read_dir(&external).unwrap().count(), 0);
    }
}

#[test]
fn save_sync_401_emits_scoped_auth_status_and_pauses_further_requests() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.on_sequence(
        "GET",
        INVENTORY_PATH,
        vec![
            ResponseSpec::Json {
                status: 200,
                body: "[]".into(),
            },
            ResponseSpec::Json {
                status: 401,
                body: "expired token".into(),
            },
        ],
    );
    let case = case(directory.path(), &fixture, true);
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut authentication_required = false;
    while Instant::now() < deadline {
        match events.recv_timeout(Duration::from_millis(50)) {
            Ok(AppEvent::SaveSyncAuthenticationRequired { session_id: 1 }) => {
                authentication_required = true;
                break;
            }
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    assert!(
        authentication_required,
        "401 did not mark the current save-sync session as requiring authentication"
    );
    let status = wait_for_queue_status(&events, Duration::from_secs(3), |status| {
        status.authentication_required && status.network_paused
    })
    .expect("401 should be present in the current queue status");
    assert_eq!(status.session_id, 1);
    agent.stop();
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
    assert_eq!(fixture.count_requests("GET", INVENTORY_PATH), 2);
    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    assert!(journal.incoming_saves().unwrap().is_empty());
}

#[test]
fn conflicted_game_does_not_block_a_healthy_local_only_game() {
    const REMOTE_CONFLICT: &[u8] = b"game one's remote conflict";
    const HEALTHY_LOCAL: &[u8] = b"game two local SRAM";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        91,
        "550e8400-e29b-41d4-a716-446655440000",
        REMOTE_CONFLICT,
        "2026-10-05T12:34:56Z",
    );
    let case = case(directory.path(), &fixture, false);
    let conflict_mapping = case.mapping.clone();
    let healthy_mapping = second_game_mapping(&case);
    fs::create_dir_all(healthy_mapping.target_path.parent().unwrap()).unwrap();
    fs::write(&healthy_mapping.target_path, HEALTHY_LOCAL).unwrap();
    let mappings = vec![conflict_mapping.clone(), healthy_mapping.clone()];
    let mut journal =
        SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    journal
        .reconcile_mappings(&MappingReport {
            mappings: mappings.clone(),
            ..MappingReport::default()
        })
        .unwrap();
    journal
        .confirm_mapping(&healthy_mapping.rom_key, &healthy_mapping.relative_path)
        .unwrap();
    drop(journal);

    let (sink, events) = channel();
    let mut agent = start_agent_with_mappings(
        &case,
        mappings,
        &fixture,
        0,
        Arc::new(AtomicBool::new(true)),
        sink,
    );
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("verified upload revision")
    })
    .is_some());
    agent.stop();

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    assert!(
        journal
            .slot(&conflict_mapping.rom_key)
            .unwrap()
            .unwrap()
            .needs_attention
    );
    assert_eq!(journal.incoming_saves().unwrap().len(), 1);
    assert_eq!(fs::read(&conflict_mapping.target_path).unwrap(), SAVE_BYTES);
    assert_eq!(
        fs::read(&healthy_mapping.target_path).unwrap(),
        HEALTHY_LOCAL
    );
    let uploaded = fixture
        .saved_saves()
        .into_iter()
        .find(|save| save.rom_id == 8)
        .unwrap();
    assert_eq!(uploaded.bytes, HEALTHY_LOCAL);
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
}

#[test]
fn first_remote_save_is_published_only_when_target_never_existed() {
    const REMOTE_BYTES: &[u8] = b"remote-only first save";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        REMOTE_BYTES,
        "2026-10-05T12:34:56Z",
    );
    let case = case(directory.path(), &fixture, false);
    fs::remove_file(&case.mapping.target_path).unwrap();
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("installed first remote save for Game.gb")
    })
    .is_some());
    agent.stop();

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), REMOTE_BYTES);
    assert_eq!(slot.remote_slot_id.as_deref(), Some("31"));
    assert_eq!(
        slot.local_baseline_hash,
        Some(sha256_content_hash(REMOTE_BYTES))
    );
    assert!(journal.snapshots().unwrap().is_empty());
    assert!(journal.incoming_saves().unwrap().is_empty());
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
}

#[test]
fn truncated_incoming_body_is_rejected_without_staging_or_local_publication() {
    const REMOTE_BYTES: &[u8] = b"truncated remote SRAM body";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        REMOTE_BYTES,
        "2026-10-05T12:34:56Z",
    );
    fixture.on(
        "GET",
        CONTENT_PATH,
        ResponseSpec::Bytes {
            status: 200,
            bytes: REMOTE_BYTES.to_vec(),
            truncate_at: Some(REMOTE_BYTES.len() - 2),
            stall_after_bytes: None,
        },
    );
    let case = case(directory.path(), &fixture, false);
    fs::remove_file(&case.mapping.target_path).unwrap();
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);

    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("incoming reconciliation for Game.gb will retry")
    })
    .is_some());
    agent.stop();

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    assert!(!case.mapping.target_path.exists());
    assert!(journal.incoming_saves().unwrap().is_empty());
    assert_eq!(fixture.count("GET", CONTENT_PATH), 1);
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
}

#[test]
fn repeated_reconciliation_reuses_verified_content_until_metadata_changes() {
    const FIRST_BYTES: &[u8] = b"cached remote bytes";
    const UPDATED_BYTES: &[u8] = b"updated remote metadata and bytes";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        FIRST_BYTES,
        "2026-10-05T12:34:56Z",
    );
    let first_case = case(directory.path(), &fixture, false);
    let (jobs, updates, active, worker, _client) = start_reconciliation_worker(&fixture);

    for _ in 0..2 {
        jobs.send(reconciliation_job(&first_case)).unwrap();
        let inventory = receive_inventory(&updates);
        assert_eq!(inventory.saves[0].bytes, FIRST_BYTES);
    }
    assert_eq!(fixture.count_requests("GET", "/api/saves/31/content"), 1);

    fixture.seed_save(FixtureSaveRecord {
        id: 31,
        rom_id: ROM_ID,
        user_id: ACCOUNT_ID,
        file_name: "rommfs-550e8400-e29b-41d4-a716-446655440000.srm".into(),
        file_size_bytes: UPDATED_BYTES.len(),
        slot: RETROBAT_GB_SRM_PROFILE.into(),
        bytes: UPDATED_BYTES.to_vec(),
        created_at: "2026-10-05T12:34:56Z".into(),
        updated_at: "2026-10-05T12:34:57Z".into(),
    });
    jobs.send(reconciliation_job(&first_case)).unwrap();
    let inventory = receive_inventory(&updates);
    assert_eq!(inventory.saves[0].bytes, UPDATED_BYTES);
    assert_eq!(fixture.count_requests("GET", "/api/saves/31/content"), 2);

    let other_installation = tempfile::tempdir().unwrap();
    let other_case = case(other_installation.path(), &fixture, false);
    jobs.send(reconciliation_job(&other_case)).unwrap();
    assert_eq!(receive_inventory(&updates).saves[0].bytes, UPDATED_BYTES);
    assert_eq!(fixture.count_requests("GET", "/api/saves/31/content"), 3);

    active.store(false, std::sync::atomic::Ordering::SeqCst);
    drop(jobs);
    worker.join().unwrap();
}

#[test]
fn incoming_403_pauses_the_worker_without_invalidating_rom_session_or_repeating_requests() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    fixture.on(
        "GET",
        INVENTORY_PATH,
        ResponseSpec::Json {
            status: 403,
            body: "save permission denied".into(),
        },
    );
    let case = case(directory.path(), &fixture, false);
    let (jobs, updates, active, worker, client) = start_reconciliation_worker(&fixture);
    jobs.send(reconciliation_job(&case)).unwrap();
    assert!(matches!(
        updates.recv_timeout(Duration::from_secs(3)).unwrap(),
        TransferUpdate::Reconciled {
            result: Err(rommfs_core::romm::SaveApiFailure {
                error: rommfs_core::error::Error::Forbidden(_),
                ..
            }),
            ..
        }
    ));
    assert!(
        client.has_token(),
        "HTTP 403 must not invalidate ROM access"
    );
    jobs.send(reconciliation_job(&case)).unwrap();
    thread::sleep(Duration::from_millis(350));
    assert_eq!(fixture.count_requests("GET", INVENTORY_PATH), 1);

    active.store(false, std::sync::atomic::Ordering::SeqCst);
    drop(jobs);
    worker.join().unwrap();

    fixture.on("GET", INVENTORY_PATH, ResponseSpec::RomMSaveInventory);
    let (jobs, updates, active, worker, _client) = start_reconciliation_worker(&fixture);
    jobs.send(reconciliation_job(&case)).unwrap();
    assert!(receive_inventory(&updates).saves.is_empty());
    assert_eq!(fixture.count_requests("GET", INVENTORY_PATH), 2);
    active.store(false, std::sync::atomic::Ordering::SeqCst);
    drop(jobs);
    worker.join().unwrap();
}

#[test]
fn inventory_429_honors_retry_after_on_the_real_worker() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    fixture.on(
        "GET",
        INVENTORY_PATH,
        ResponseSpec::JsonWithHeaders {
            status: 429,
            body: "rate limited".into(),
            headers: vec![("Retry-After".into(), "3".into())],
        },
    );
    let case = case(directory.path(), &fixture, false);
    let (jobs, updates, active, worker, _client) = start_reconciliation_worker(&fixture);
    jobs.send(reconciliation_job(&case)).unwrap();
    assert!(matches!(
        updates.recv_timeout(Duration::from_secs(3)).unwrap(),
        TransferUpdate::ReconciliationRetry { .. }
    ));
    thread::sleep(Duration::from_millis(2300));
    assert_eq!(fixture.count_requests("GET", INVENTORY_PATH), 1);
    assert!(matches!(
        updates.recv_timeout(Duration::from_secs(2)).unwrap(),
        TransferUpdate::ReconciliationRetry { .. }
    ));
    assert_eq!(fixture.count_requests("GET", INVENTORY_PATH), 2);

    active.store(false, std::sync::atomic::Ordering::SeqCst);
    drop(jobs);
    worker.join().unwrap();
}

fn reconciliation_job(case: &AgentCase) -> WorkerJob {
    WorkerJob::Reconcile(Box::new(IncomingJob {
        mapping: case.mapping.clone(),
        identity: SaveSyncIdentity {
            account_id: ACCOUNT_ID,
            scopes: vec!["assets.read".into(), "assets.write".into()],
        },
        scope: case.scope.clone(),
    }))
}

type TestReconciliationWorker = (
    mpsc::Sender<WorkerJob>,
    Receiver<TransferUpdate>,
    Arc<AtomicBool>,
    thread::JoinHandle<()>,
    Arc<RommClient>,
);

fn start_reconciliation_worker(fixture: &FixtureServer) -> TestReconciliationWorker {
    let (jobs, receive_jobs) = mpsc::channel();
    let (send_updates, updates) = mpsc::channel();
    let active = Arc::new(AtomicBool::new(true));
    let worker_active = Arc::clone(&active);
    let client = authenticated_client(fixture);
    let worker_client = Arc::clone(&client);
    let worker = thread::spawn(move || {
        run_transfer_worker(
            receive_jobs,
            send_updates,
            worker_client,
            Arc::new(AtomicBool::new(true)).into(),
            worker_active,
        )
    });
    (jobs, updates, active, worker, client)
}

fn receive_inventory(
    updates: &Receiver<TransferUpdate>,
) -> super::incoming_transfer::IncomingInventory {
    match updates.recv_timeout(Duration::from_secs(3)).unwrap() {
        TransferUpdate::Reconciled {
            result: Ok(inventory),
            ..
        } => inventory,
        _ => panic!("unexpected worker update"),
    }
}

#[test]
fn unordered_remote_history_uses_unique_latest_timestamp_and_remembers_all_ids() {
    const OLDER_BYTES: &[u8] = b"older remote save";
    const LATEST_BYTES: &[u8] = b"latest remote save";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        OLDER_BYTES,
        "2026-10-04T12:34:56Z",
    );
    seed_remote(
        &fixture,
        32,
        "550e8400-e29b-41d4-a716-446655440001",
        LATEST_BYTES,
        "2026-10-05T12:34:56Z",
    );
    let case = case(directory.path(), &fixture, false);
    fs::remove_file(&case.mapping.target_path).unwrap();
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);

    let reconciliation = wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("Game.gb")
    });
    assert!(
        reconciliation
            .as_deref()
            .is_some_and(|message| message.contains("installed first remote save")),
        "unexpected reconciliation result: {reconciliation:?}"
    );
    agent.stop();

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), LATEST_BYTES);
    assert_eq!(slot.remote_slot_id.as_deref(), Some("32"));
    let mut history_ids = slot.remote_history_ids;
    history_ids.sort();
    assert_eq!(history_ids, ["31", "32"]);
}

#[test]
fn fractional_rfc3339_precision_selects_the_newest_same_second_record() {
    const EARLIER_BYTES: &[u8] = b"earlier fractional revision";
    const LATER_BYTES: &[u8] = b"later fractional revision";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        EARLIER_BYTES,
        "2026-10-05T12:34:56.123456788Z",
    );
    seed_remote(
        &fixture,
        32,
        "550e8400-e29b-41d4-a716-446655440001",
        LATER_BYTES,
        "2026-10-05T12:34:56.123456789Z",
    );
    let case = case(directory.path(), &fixture, false);
    fs::remove_file(&case.mapping.target_path).unwrap();
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);

    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("installed first remote save for Game.gb")
    })
    .is_some());
    agent.stop();

    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), LATER_BYTES);
    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    assert_eq!(
        journal
            .slot(&case.mapping.rom_key)
            .unwrap()
            .unwrap()
            .remote_slot_id
            .as_deref(),
        Some("32")
    );
}

#[test]
fn invalid_history_dates_and_offsets_fail_without_publishing_a_remote_save() {
    for timestamp in ["2026-02-30T12:34:56Z", "2026-10-05T12:34:56+25:00"] {
        let directory = tempfile::tempdir().unwrap();
        let fixture = FixtureServer::start();
        fixture.use_romm_save_store(ACCOUNT_ID, 201);
        seed_remote(
            &fixture,
            31,
            "550e8400-e29b-41d4-a716-446655440000",
            b"unorderable remote save",
            timestamp,
        );
        let case = case(directory.path(), &fixture, false);
        fs::remove_file(&case.mapping.target_path).unwrap();
        let (sink, events) = channel();
        let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);

        assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
            message.contains("inventory failed for Game.gb")
        })
        .is_some());
        agent.stop();

        let journal =
            SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
        assert!(!case.mapping.target_path.exists(), "{timestamp}");
        assert!(journal.incoming_saves().unwrap().is_empty(), "{timestamp}");
        assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
    }
}

#[test]
fn tied_remote_history_requires_attention_without_publishing_either_save() {
    const FIRST_BYTES: &[u8] = b"first tied remote save";
    const SECOND_BYTES: &[u8] = b"second tied remote save";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        FIRST_BYTES,
        "2026-10-05T12:34:56Z",
    );
    seed_remote(
        &fixture,
        32,
        "550e8400-e29b-41d4-a716-446655440001",
        SECOND_BYTES,
        "2026-10-05T14:34:56+02:00",
    );
    let case = case(directory.path(), &fixture, false);
    fs::remove_file(&case.mapping.target_path).unwrap();
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);

    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("tie for the latest compatible revision")
    })
    .is_some());
    agent.stop();

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    assert!(slot.needs_attention);
    assert!(!case.mapping.target_path.exists());
    assert_eq!(journal.incoming_saves().unwrap().len(), 2);
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
}

#[test]
fn local_file_created_during_blocked_incoming_body_wins_without_remote_loss() {
    const REMOTE_BYTES: &[u8] = b"downloaded remote bytes";
    const EMULATOR_BYTES: &[u8] = b"new emulator SRAM";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        REMOTE_BYTES,
        "2026-10-05T12:34:56Z",
    );
    let barrier = FixtureBodyBarrier::new();
    fixture.on(
        "GET",
        "/api/saves/31/content?optimistic=false",
        ResponseSpec::HeldBytes {
            status: 200,
            bytes: REMOTE_BYTES.to_vec(),
            barrier: barrier.clone(),
        },
    );
    let case = case(directory.path(), &fixture, false);
    fs::remove_file(&case.mapping.target_path).unwrap();
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(barrier.wait_until_blocked(Duration::from_secs(5)));
    fs::write(&case.mapping.target_path, EMULATOR_BYTES).unwrap();
    barrier.release();
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("first sync found different local and remote saves")
    })
    .is_some());
    agent.stop();

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), EMULATOR_BYTES);
    assert_eq!(
        fs::read(&journal.incoming_saves().unwrap()[0].path).unwrap(),
        REMOTE_BYTES
    );
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
}

#[test]
fn disabling_while_incoming_body_is_blocked_prevents_late_publication() {
    const REMOTE_BYTES: &[u8] = b"blocked incoming bytes";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        REMOTE_BYTES,
        "2026-10-05T12:34:56Z",
    );
    let barrier = FixtureBodyBarrier::new();
    fixture.on(
        "GET",
        "/api/saves/31/content?optimistic=false",
        ResponseSpec::HeldBytes {
            status: 200,
            bytes: REMOTE_BYTES.to_vec(),
            barrier: barrier.clone(),
        },
    );
    let case = case(directory.path(), &fixture, false);
    fs::remove_file(&case.mapping.target_path).unwrap();
    let enabled = Arc::new(AtomicBool::new(true));
    let (sink, _events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::clone(&enabled), sink);
    assert!(barrier.wait_until_blocked(Duration::from_secs(5)));
    enabled.store(false, std::sync::atomic::Ordering::SeqCst);
    barrier.release();
    agent.stop();

    assert!(!case.mapping.target_path.exists());
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
}

#[test]
fn interrupted_incoming_publication_is_durable_but_never_retried_as_first_install() {
    const REMOTE_BYTES: &[u8] = b"survives staging interruption";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        REMOTE_BYTES,
        "2026-10-05T12:34:56Z",
    );
    let case = case(directory.path(), &fixture, false);
    fs::remove_file(&case.mapping.target_path).unwrap();
    let incoming_id = {
        let mut journal =
            SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
        let staged = journal
            .stage_incoming(
                &case.mapping,
                "31",
                REMOTE_BYTES,
                "first_remote_save",
                None,
                None,
                None,
            )
            .unwrap();
        let intent = journal
            .begin_incoming_publication(&staged.id, &case.mapping)
            .unwrap();
        intent.id
    };
    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    assert_eq!(journal.incoming_saves().unwrap()[0].id, incoming_id);
    assert_eq!(journal.incoming_saves().unwrap()[0].state, "interrupted");
    assert!(!case.mapping.target_path.exists());
    drop(journal);

    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("tracked local save was removed")
    })
    .is_some());
    agent.stop();
    assert!(!case.mapping.target_path.exists());
}

#[test]
fn external_change_matching_new_remote_bytes_resolves_staged_conflict_without_upload() {
    const REMOTE_BYTES: &[u8] = b"newer remote revision";
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        SAVE_BYTES,
        "2026-10-04T12:34:56Z",
    );
    seed_remote(
        &fixture,
        32,
        "550e8400-e29b-41d4-a716-446655440001",
        REMOTE_BYTES,
        "2026-10-05T12:34:56Z",
    );
    let case = case(directory.path(), &fixture, false);
    accept_remote_baseline(&case, "31", SAVE_BYTES);
    let (sink, events) = channel();
    let mut agent = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("newer save than the local baseline")
    })
    .is_some());
    agent.stop();
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), SAVE_BYTES);

    fs::write(&case.mapping.target_path, REMOTE_BYTES).unwrap();
    let (sink, events) = channel();
    let mut restarted = start_agent(&case, &fixture, 0, Arc::new(AtomicBool::new(true)), sink);
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("reconciled saves for Game.gb")
    })
    .is_some());
    restarted.stop();

    let journal = SaveSyncJournal::open(&case.database, &case.spool, case.scope.clone()).unwrap();
    let slot = journal.slot(&case.mapping.rom_key).unwrap().unwrap();
    assert!(!slot.needs_attention);
    assert_eq!(slot.remote_slot_id.as_deref(), Some("32"));
    assert!(journal.incoming_saves().unwrap().is_empty());
    assert_eq!(fs::read(&case.mapping.target_path).unwrap(), REMOTE_BYTES);
    assert_eq!(fixture.count("POST", "/api/saves?rom_id=7&slot="), 0);
}

#[test]
fn tracked_local_removal_and_missing_remote_never_delete_or_resurrect_local_saves() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_save_store(ACCOUNT_ID, 201);
    seed_remote(
        &fixture,
        31,
        "550e8400-e29b-41d4-a716-446655440000",
        SAVE_BYTES,
        "2026-10-05T12:34:56Z",
    );
    let removed_case = case(directory.path(), &fixture, false);
    accept_remote_baseline(&removed_case, "31", SAVE_BYTES);
    fs::remove_file(&removed_case.mapping.target_path).unwrap();
    let (sink, events) = channel();
    let mut agent = start_agent(
        &removed_case,
        &fixture,
        0,
        Arc::new(AtomicBool::new(true)),
        sink,
    );
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("tracked local save was removed")
    })
    .is_some());
    agent.stop();
    assert!(!removed_case.mapping.target_path.exists());
    let journal = SaveSyncJournal::open(
        &removed_case.database,
        &removed_case.spool,
        removed_case.scope.clone(),
    )
    .unwrap();
    assert!(
        journal
            .slot(&removed_case.mapping.rom_key)
            .unwrap()
            .unwrap()
            .local_removed
    );
    assert_eq!(journal.incoming_saves().unwrap().len(), 1);

    let missing_fixture = FixtureServer::start();
    missing_fixture.use_romm_save_store(ACCOUNT_ID, 201);
    let missing_directory = tempfile::tempdir().unwrap();
    let missing_case = case(missing_directory.path(), &missing_fixture, false);
    accept_remote_baseline(&missing_case, "44", SAVE_BYTES);
    let original = fs::read(&missing_case.mapping.target_path).unwrap();
    let (sink, events) = channel();
    let mut missing_agent = start_agent(
        &missing_case,
        &missing_fixture,
        0,
        Arc::new(AtomicBool::new(true)),
        sink,
    );
    assert!(wait_for_log(&events, Duration::from_secs(5), |message| {
        message.contains("previously accepted remote save is missing")
    })
    .is_some());
    missing_agent.stop();
    assert_eq!(
        fs::read(&missing_case.mapping.target_path).unwrap(),
        original
    );
    assert_eq!(
        missing_fixture.count("POST", "/api/saves?rom_id=7&slot="),
        0
    );
}
