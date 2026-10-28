use super::*;
use rommfs_core::catalogue::build_catalogue;
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
#[path = "tests/exports.rs"]
mod exports;
#[path = "tests/incoming.rs"]
mod incoming;
#[path = "tests/lifecycle.rs"]
mod lifecycle;
#[path = "tests/ordinary.rs"]
mod ordinary;
#[path = "tests/reconciliation.rs"]
mod reconciliation;
#[path = "tests/uploads.rs"]
mod uploads;

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
        metadata: Default::default(),
        rom_user: None,
        fs_path: String::new(),
        files: vec![RomFileDto {
            id: FILE_ID,
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
        None,
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
        metadata: Default::default(),
        rom_user: None,
        fs_path: String::new(),
        files: vec![RomFileDto {
            id: 80,
            file_name: "Healthy.gb".into(),
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
    let mut observed = Vec::new();
    while Instant::now() < deadline {
        match events.recv_timeout(Duration::from_millis(50)) {
            Ok(AppEvent::Log(line)) if predicate(&line.message) => return Some(line.message),
            Ok(AppEvent::Log(line)) => observed.push(line.message),
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return None,
        }
    }
    eprintln!("Timed out waiting for save-sync log; observed logs: {observed:#?}");
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
                return Some(status);
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

fn reconciliation_job(case: &AgentCase) -> WorkerJob {
    WorkerJob::Reconcile(Box::new(IncomingJob {
        mapping: case.mapping.clone(),
        identity: SaveSyncIdentity {
            account_id: ACCOUNT_ID,
            scopes: vec!["assets.read".into(), "assets.write".into()],
        },
        scope: case.scope.clone(),
        verify_reviewed_history: false,
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
