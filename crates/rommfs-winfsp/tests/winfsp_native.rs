//! Native WinFsp integration test (PRD §6 "Native Windows integration test"):
//! mounts a real `RommFs` on a temporary WinFsp root via `winfsp` and exercises
//! ordinary Windows file operations — not trait calls.
//!
//! Proves: enumeration/stat never request ROM bodies; the first content read
//! performs exactly one fixture download and returns byte-exact data; warm
//! reads do not re-download; delete/rename/write attempts on projected ROM
//! entries are rejected; stop is clean.
//!
//! Missing prerequisites are reported as UNAVAILABLE, never silently passed.

#![cfg(windows)]

use rommfs_core::cache::clock::DEFAULT_EVICTION_THRESHOLD_SECS;
use rommfs_core::cache::{CacheIndex, Evictor, FakeClock, LiveState, NoopHydratedRemover};
use rommfs_core::catalog::{build_catalogue, server_id_of, RomKey};
use rommfs_core::download::DownloadManager;
use rommfs_core::fscore::RommFs;
use rommfs_core::romm::{Credentials, RommClient};
use rommfs_core::tree::RommTree;
use rommfs_fixture::{contract, FixtureServer, ResponseSpec};
use rommfs_winfsp::{check_mount_root, claim_mount_root, RootCheck, WindowsMount};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
/// Deterministic, nontrivial content: 64 KiB of a repeating ramp plus a
/// recognizable header/footer so offset reads are verifiable.
fn rom_bytes() -> Vec<u8> {
    let mut v = b"NESROM-HEADER-".to_vec();
    v.extend((0..=u8::MAX).cycle().take(64 * 1024));
    v.extend_from_slice(b"-NESROM-TAIL");
    v
}

fn wait_until(mut ready: impl FnMut() -> bool, timeout: Duration, what: &str) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if ready() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out after {timeout:?} waiting for: {what}");
}

#[test]
fn winfsp_mount_lists_reads_once_and_stays_read_only() {
    winfsp::winfsp_init().expect("UNAVAILABLE: install WinFsp 2.1 or later");

    // --- fixture RomM server implementing the verified contract routes ---
    let server = FixtureServer::start();
    let bytes = rom_bytes();
    server.on(
        "POST",
        "/api/token",
        ResponseSpec::Json {
            status: 200,
            body: contract::token_ok(),
        },
    );
    server.on(
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
    let rom = contract::rom(
        7,
        "nes",
        "Example Game.nes",
        bytes.len() as u64,
        "0123456789abcdef0123456789abcdef01234567",
    );
    let mut fixture_roms = vec![rom];
    for id in 100..700 {
        fixture_roms.push(contract::rom(
            id,
            "nes",
            &format!("Enumeration {id:04}.nes"),
            1,
            "0123456789abcdef0123456789abcdef01234567",
        ));
    }
    server.on(
        "GET",
        "/api/roms?",
        ResponseSpec::Json {
            status: 200,
            body: contract::roms_page(&fixture_roms, fixture_roms.len(), 1000, 0),
        },
    );
    server.on(
        "GET",
        "/api/roms/",
        ResponseSpec::Bytes {
            status: 200,
            bytes: bytes.clone(),
            truncate_at: None,
            stall_after_bytes: None,
        },
    );

    // --- real client against the fixture ---
    let client = RommClient::new(server.url()).expect("client builds");
    client
        .authenticate(Credentials {
            username: "fixture-user",
            password: "fixture-password",
        })
        .expect("fixture auth succeeds");
    let platforms = client.platforms().expect("platforms");
    let roms = client.all_roms().expect("roms page");
    let server_id = server_id_of(server.url());
    let catalogue = build_catalogue(&server_id, &platforms, &roms, |w| {
        panic!("unexpected catalogue warning: {w}")
    })
    .expect("catalogue builds");
    assert_eq!(catalogue.skipped_unsupported, 0);

    // --- real core stack; private cache is the only persistent ROM copy ---
    let cache = tempfile::tempdir().expect("cache dir");
    let index = CacheIndex::open(cache.path()).expect("cache index");
    let live = Arc::new(LiveState::default());
    let expected: HashMap<RomKey, u64> = catalogue
        .entries
        .iter()
        .map(|e| (e.key.clone(), e.size))
        .collect();
    let versions: HashMap<RomKey, Option<String>> = catalogue
        .entries
        .iter()
        .map(|e| (e.key.clone(), e.version.as_ref().map(|v| v.0.clone())))
        .collect();
    let (sink, _rx) = rommfs_core::events::channel();
    let downloads = Arc::new(DownloadManager::new(
        index,
        Arc::clone(&live),
        Arc::new(client),
        sink,
        expected,
        versions,
    ));

    let workspace = tempfile::tempdir().expect("test workspace");
    let root = workspace.path().join("mount");
    std::fs::create_dir(&root).unwrap();
    let evictor = Evictor::new(
        DEFAULT_EVICTION_THRESHOLD_SECS,
        Arc::clone(&live),
        Arc::new(NoopHydratedRemover),
    );
    let clock = Arc::new(FakeClock::new(1_000_000));
    let tree = RommTree::new(catalogue);
    let fs = Arc::new(RommFs::new(tree, downloads, evictor, clock.clone()));

    // --- mount into the checked+claimed empty root ---
    assert!(matches!(
        check_mount_root(&root, &server_id).unwrap(),
        RootCheck::EmptyReady
    ));
    claim_mount_root(&root, &server_id).unwrap();
    let mount = WindowsMount::mount(Arc::clone(&fs), &root).expect("WinFsp mount");

    let nes_dir: PathBuf = root.join("nes");
    let rom_path = nes_dir.join("Example Game.nes");
    wait_until(
        || rom_path.exists(),
        Duration::from_secs(5),
        "projected ROM path to appear",
    );

    // --- listing: platform dirs + ROM filename via ordinary enumeration ---
    let root_names: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        root_names.iter().any(|n| n.eq_ignore_ascii_case("nes")),
        "platform dir missing from root listing: {root_names:?}"
    );
    assert!(
        root_names.iter().any(|n| n.eq_ignore_ascii_case("snes")),
        "empty platform dir must still list: {root_names:?}"
    );
    let rom_names: Vec<String> = std::fs::read_dir(&nes_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        rom_names.iter().any(|n| n == "Example Game.nes"),
        "ROM missing from platform dir: {rom_names:?}"
    );
    assert_eq!(
        rom_names.len(),
        601,
        "enumeration must continue beyond a core page"
    );
    let unique: std::collections::HashSet<_> = rom_names.iter().collect();
    assert_eq!(
        unique.len(),
        601,
        "directory continuation must not duplicate entries"
    );
    for id in 100..700 {
        assert!(unique.contains(&format!("Enumeration {id:04}.nes")));
    }
    // Stat through the OS: correct type + logical size, still no download.
    let meta = std::fs::metadata(&rom_path).unwrap();
    let alias = root.join("NES").join("EXAMPLE GAME.NES");
    assert_eq!(std::fs::metadata(&alias).unwrap().len(), bytes.len() as u64);
    assert!(!meta.is_dir());
    assert_eq!(meta.len(), bytes.len() as u64);
    assert_eq!(
        server.count("GET", "/api/roms/"),
        0,
        "listing/stat must not request ROM content"
    );

    assert_read_only(&root, &rom_path);
    assert_eq!(
        server.count("GET", "/api/roms/"),
        0,
        "failed mutations must not download content"
    );

    // --- first content read: one download, byte-exact ---
    let got = std::fs::read(&rom_path).expect("first read");
    assert_eq!(got, bytes, "first read must return exact fixture bytes");
    assert_eq!(
        server.count("GET", "/api/roms/"),
        1,
        "first read performs exactly one content download"
    );

    // --- warm reads: byte-exact, no second transfer ---
    let canonical = std::fs::canonicalize(&alias).expect("canonical name through WinFsp");
    assert_eq!(canonical.file_name().unwrap(), "Example Game.nes");
    assert_eq!(canonical.parent().unwrap().file_name().unwrap(), "nes");
    let got2 = std::fs::read(&alias).expect("second read through a differently cased path");
    assert_eq!(got2, bytes);
    let mut file = std::fs::File::open(&rom_path).unwrap();
    file.seek(SeekFrom::Start(14)).unwrap();
    let mut mid = [0u8; 32];
    file.read_exact(&mut mid).unwrap();
    assert_eq!(&mid[..], &bytes[14..46], "seeked read must be byte-exact");
    file.seek(SeekFrom::End(-12)).unwrap();
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).unwrap();
    assert_eq!(tail, bytes[bytes.len() - 12..]);
    drop(file);
    assert_eq!(
        server.count("GET", "/api/roms/"),
        1,
        "warm reads must be served without another download"
    );

    // The volume remains read-only after content is cached.
    assert_read_only(&root, &rom_path);
    assert_eq!(std::fs::read(&rom_path).unwrap(), bytes);

    // Multiple simultaneous handles protect the same entry from eviction.
    let first = std::fs::File::open(&rom_path).unwrap();
    let second = std::fs::File::open(&rom_path).unwrap();
    clock.advance(DEFAULT_EVICTION_THRESHOLD_SECS + 1);
    assert_eq!(fs.evict_stale().unwrap().evicted.len(), 0);
    drop(first);
    assert_eq!(fs.evict_stale().unwrap().evicted.len(), 0);
    drop(second);
    wait_until(
        || fs.evict_stale().unwrap().evicted.len() == 1,
        Duration::from_secs(5),
        "closed ROM to be evicted",
    );
    assert!(rom_path.exists(), "eviction keeps the catalogue entry");
    assert_eq!(std::fs::read(&rom_path).unwrap(), bytes);
    assert_eq!(
        server.count("GET", "/api/roms/"),
        2,
        "evicted bytes must download again"
    );

    mount.stop();
    assert!(
        !rom_path.exists(),
        "unmount leaves no ROM copies at the mount root"
    );
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    assert!(matches!(
        check_mount_root(&root, &server_id).unwrap(),
        RootCheck::RecognizedOwned
    ));
    let mount2 = WindowsMount::mount(Arc::clone(&fs), &root).expect("remount");
    assert_eq!(std::fs::read(&rom_path).unwrap(), bytes);
    assert_eq!(
        server.count("GET", "/api/roms/"),
        2,
        "remount uses the private cache"
    );
    mount2.stop();

    // Local files introduced while stopped must remain untouched.
    let local = root.join("local-notes.txt");
    std::fs::write(&local, b"keep local data").unwrap();
    assert!(check_mount_root(&root, &server_id).is_err());
    assert!(WindowsMount::mount(fs, &root).is_err());
    assert_eq!(std::fs::read(local).unwrap(), b"keep local data");
}

fn assert_read_only(root: &Path, rom: &Path) {
    assert!(std::fs::remove_file(rom).is_err());
    assert!(std::fs::rename(rom, rom.with_file_name("Renamed.nes")).is_err());
    assert!(std::fs::write(rom, b"overwrite").is_err());
    assert!(std::fs::OpenOptions::new().append(true).open(rom).is_err());
    assert!(std::fs::hard_link(rom, rom.with_file_name("Linked.nes")).is_err());
    assert!(std::fs::write(root.join("new.txt"), b"new").is_err());
    assert!(std::fs::create_dir(root.join("new-dir")).is_err());
    assert!(std::fs::remove_dir(rom.parent().unwrap()).is_err());
    assert!(rom.exists());
}
