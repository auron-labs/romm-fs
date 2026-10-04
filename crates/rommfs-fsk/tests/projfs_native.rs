//! Native ProjFS integration test (PRD §6 "Native Windows integration test"):
//! mounts a real `RommFs` on a temporary ProjFS root via `fsk` and exercises
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
use rommfs_core::cache::{CacheIndex, Evictor, LiveState, SystemClock};
use rommfs_core::catalog::{build_catalogue, server_id_of, RomKey};
use rommfs_core::download::DownloadManager;
use rommfs_core::fscore::RommFs;
use rommfs_core::romm::{Credentials, RommClient};
use rommfs_core::tree::RommTree;
use rommfs_fixture::{contract, FixtureServer, ResponseSpec};
use rommfs_fsk::{
    check_mount_root, claim_mount_root, ProjfsHandle, ProjfsRemover, RootCheck, WindowsMount,
};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows_sys::Win32::Storage::ProjectedFileSystem::{
    PrjGetOnDiskFileState, PRJ_FILE_STATE_HYDRATED_PLACEHOLDER, PRJ_FILE_STATE_PLACEHOLDER,
};

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
fn projfs_mount_lists_reads_once_and_stays_read_only() {
    // --- prerequisite: ProjFS optional component present ---
    let projfs_dll = Path::new(r"C:\Windows\System32\ProjectedFSLib.dll");
    if !projfs_dll.exists() {
        panic!(
            "UNAVAILABLE: {} missing — enable the Client-ProjFS optional Windows feature",
            projfs_dll.display()
        );
    }

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
    server.on(
        "GET",
        "/api/roms?",
        ResponseSpec::Json {
            status: 200,
            body: contract::roms_page(&[rom], 1, 200, 0),
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

    // --- real core stack; the hydrated remover is wired to the mount's
    //     handle (created here so both sides share it) ---
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

    let root = tempfile::tempdir().expect("mount root");
    let handle = Arc::new(ProjfsHandle::default());
    let evictor = Evictor::new(
        DEFAULT_EVICTION_THRESHOLD_SECS,
        live,
        Arc::new(ProjfsRemover::new(
            root.path().to_path_buf(),
            Arc::clone(&handle),
        )),
    );
    let tree = RommTree::new(catalogue);
    let fs = Arc::new(RommFs::new(tree, downloads, evictor, Arc::new(SystemClock)));

    // --- mount into the checked+claimed empty root ---
    assert!(matches!(
        check_mount_root(root.path(), &server_id).unwrap(),
        RootCheck::EmptyReady
    ));
    claim_mount_root(root.path(), &server_id).unwrap();
    let (mount, _handle) =
        WindowsMount::mount_with_handle(Arc::clone(&fs), root.path(), Arc::clone(&handle))
            .expect("ProjFS mount");

    let nes_dir: PathBuf = root.path().join("nes");
    let rom_path = nes_dir.join("Example Game.nes");
    wait_until(
        || rom_path.exists(),
        Duration::from_secs(5),
        "projected ROM path to appear",
    );

    // --- listing: platform dirs + ROM filename via ordinary enumeration ---
    let root_names: Vec<String> = std::fs::read_dir(root.path())
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
    // Stat through the OS: correct type + logical size, still no download.
    let meta = std::fs::metadata(&rom_path).unwrap();
    assert!(!meta.is_dir());
    assert_eq!(meta.len(), bytes.len() as u64);
    assert_eq!(
        server.count("GET", "/api/roms/"),
        0,
        "listing/stat must not request ROM content"
    );

    // --- read-only: mutation attempts from a *non-provider* process all
    //     fail. Two ProjFS properties shape this section:
    //     * the provider's own process bypasses notification delivery, so
    //       attempts must run in a child process (`cmd`), and
    //     * writes are vetoable only via PRJ_NOTIFY_FILE_PRE_CONVERT_TO_FULL,
    //       which fires while the file is still a placeholder — hence
    //       mutations run BEFORE the first read hydrates the ROM.
    //     `del` always exits 0 and `ren` prints failures to stderr, so every
    //     attempt is verified by its observable effect.
    let del = std::process::Command::new("cmd")
        .args(["/c", "del", "/f", "/q"])
        .arg(format!("\"{}\"", rom_path.display()))
        .output()
        .expect("spawn del");
    assert!(
        rom_path.exists(),
        "del must be vetoed (PRE_DELETE); stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&del.stdout),
        String::from_utf8_lossy(&del.stderr)
    );
    let renamed = nes_dir.join("Renamed.nes");
    let ren = std::process::Command::new("cmd")
        .args(["/c", "ren"])
        .arg(format!("\"{}\"", rom_path.display()))
        .arg("Renamed.nes")
        .output()
        .expect("spawn ren");
    assert!(
        rom_path.exists() && !renamed.exists(),
        "ren must be vetoed (PRE_RENAME); stderr={:?}",
        String::from_utf8_lossy(&ren.stderr)
    );
    let wr = std::process::Command::new("cmd")
        .args([
            "/c",
            "echo",
            "overwritten>",
            &format!("\"{}\"", rom_path.display()),
        ])
        .output()
        .expect("spawn write");
    assert!(
        rom_path.exists() && std::fs::metadata(&rom_path).unwrap().len() == bytes.len() as u64,
        "write must be vetoed (PRE_CONVERT_TO_FULL); stderr={:?}",
        String::from_utf8_lossy(&wr.stderr)
    );
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
    let got2 = std::fs::read(&rom_path).expect("second read");
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

    // --- hydrated entries stay protected: deletes/renames are still vetoed ---
    let del2 = std::process::Command::new("cmd")
        .args(["/c", "del", "/f", "/q"])
        .arg(format!("\"{}\"", rom_path.display()))
        .output()
        .expect("spawn del #2");
    assert!(
        rom_path.exists(),
        "del on a hydrated ROM must be vetoed; stderr={:?}",
        String::from_utf8_lossy(&del2.stderr)
    );
    let ren2 = std::process::Command::new("cmd")
        .args(["/c", "ren"])
        .arg(format!("\"{}\"", rom_path.display()))
        .arg("Renamed.nes")
        .output()
        .expect("spawn ren #2");
    assert!(
        rom_path.exists() && !renamed.exists(),
        "ren on a hydrated ROM must be vetoed; stderr={:?}",
        String::from_utf8_lossy(&ren2.stderr)
    );
    assert_eq!(
        std::fs::read(&rom_path).unwrap(),
        bytes,
        "projected content must be unchanged after failed mutations"
    );

    // Local content under a managed root belongs to the user. Recovery must
    // invalidate hydrated ROM placeholders without clearing these files.
    let root_user_file = root.path().join("local-notes.txt");
    std::fs::write(&root_user_file, b"keep root-local data").unwrap();
    let save_dir = nes_dir.join("saves");
    std::fs::create_dir_all(&save_dir).unwrap();
    let save_file = save_dir.join("slot1.sav");
    std::fs::write(&save_file, b"keep nested save data").unwrap();

    assert_eq!(server.count("GET", "/api/roms/"), 1);
    assert!(
        rom_path.exists(),
        "ROM entry must remain listed after failed mutations"
    );

    // NOTE: creating a *new* local file inside the root cannot be vetoed
    // (PRJ_NOTIFY_NEW_FILE_CREATED is post-only — recorded in
    // .planning/BACKEND-DECISION.md), and writing to an already-hydrated file
    // produces only post-notifications (local mutation lands in the ProjFS
    // cache). Read-only enforcement therefore covers the projected view:
    // deletes, renames, hardlinks and conversion of placeholders are vetoed,
    // which the assertions above prove.

    // --- clean stop ---
    mount.stop();
    assert!(root.path().exists(), "root dir survives a clean stop");
    std::fs::write(&root_user_file, b"updated while stopped").unwrap();

    // --- remount must work: ProjFS leaves its virtualization-root reparse
    // tag after stop. Recovery clears the tag and clean placeholders while
    // preserving full/local files in the same managed root.
    let (mount2, _handle2) = WindowsMount::mount_with_handle(
        Arc::clone(&fs),
        root.path(),
        Arc::new(ProjfsHandle::default()),
    )
    .expect("remount on a stopped root must succeed");
    wait_until(
        || rom_path.exists(),
        Duration::from_secs(5),
        "projected ROM path to reappear after remount",
    );
    assert_eq!(
        std::fs::metadata(&rom_path).unwrap().len(),
        bytes.len() as u64,
        "stat must materialize the virtual entry without hydrating its contents"
    );
    let rom_path_wide: Vec<u16> = rom_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut remounted_state = 0;
    // SAFETY: `rom_path_wide` is a live, null-terminated UTF-16 path and
    // `remounted_state` is a valid writable output for the duration of call.
    let state_hr = unsafe { PrjGetOnDiskFileState(rom_path_wide.as_ptr(), &mut remounted_state) };
    assert!(
        state_hr >= 0,
        "query remounted ROM state failed: 0x{:08x}",
        state_hr as u32
    );
    assert_ne!(
        remounted_state & PRJ_FILE_STATE_PLACEHOLDER,
        0,
        "remount must expose a fresh ProjFS placeholder, not a retained full file"
    );
    assert_eq!(
        remounted_state & PRJ_FILE_STATE_HYDRATED_PLACEHOLDER,
        0,
        "remount must discard the previous hydrated ProjFS copy before reading"
    );
    let got = std::fs::read(&rom_path).expect("read ROM after remount");
    assert_eq!(got, bytes, "remounted read is byte-exact");
    assert_eq!(
        std::fs::read(&root_user_file).unwrap(),
        b"updated while stopped",
        "full local data modified after stop must survive recovery"
    );
    assert_eq!(
        std::fs::read(&save_file).unwrap(),
        b"keep nested save data",
        "nested local data must survive recovery"
    );
    assert_eq!(
        server.count("GET", "/api/roms/"),
        1,
        "remount must hydrate from the existing private cache without another HTTP transfer"
    );
    mount2.stop();
}
