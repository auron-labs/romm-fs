//! End-to-end test against a REAL RomM server (PRD: validated against the
//! live contract). Excluded from the default suite (`#[ignore]`) because it
//! needs a running RomM; run it with:
//!
//! ```bash
//! # cd testing/romm-harness && docker compose up -d   (see its README)
//! cargo test -p rommfs-fsk --test live_romm -- --ignored
//! ```
//!
//! Env overrides: ROMM_URL (default http://localhost:8080), ROMM_USER,
//! ROMM_PASS (defaults admin/admin123 — the harness's local-only creds).
//! It mounts via real ProjFS and byte-compares every projected ROM against
//! the deterministic placeholder files committed under
//! `testing/romm-harness/library/roms/<fs_slug>/<file>`.

#![cfg(windows)]

use rommfs_core::cache::clock::DEFAULT_EVICTION_THRESHOLD_SECS;
use rommfs_core::cache::{CacheIndex, Evictor, LiveState, SystemClock};
use rommfs_core::catalog::{build_catalogue, server_id_of, RomKey};
use rommfs_core::download::DownloadManager;
use rommfs_core::fscore::RommFs;
use rommfs_core::romm::{Credentials, RommClient};
use rommfs_core::tree::RommTree;
use rommfs_fsk::{check_mount_root, claim_mount_root, ProjfsHandle, ProjfsRemover, WindowsMount};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn harness_rom(platform_fs_slug: &str, file_name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testing/romm-harness/library/roms")
        .join(platform_fs_slug)
        .join(file_name)
}

#[test]
#[ignore = "requires a live RomM server; see header docs"]
fn live_romm_mounts_lists_and_reads_byte_exact() {
    let url = std::env::var("ROMM_URL").unwrap_or_else(|_| "http://localhost:8080".into());
    let user = std::env::var("ROMM_USER").unwrap_or_else(|_| "admin".into());
    let pass = std::env::var("ROMM_PASS").unwrap_or_else(|_| "admin123".into());
    if !Path::new(r"C:\Windows\System32\ProjectedFSLib.dll").exists() {
        panic!("UNAVAILABLE: Client-ProjFS optional feature is not installed");
    }

    let client = RommClient::new(&url).expect("client builds");
    if let Err(e) = client.authenticate(Credentials {
        username: &user,
        password: &pass,
    }) {
        panic!(
            "UNAVAILABLE: cannot authenticate to {url}: {e} \
             (is the harness up? see testing/romm-harness/README.md)"
        );
    }
    let platforms = client.platforms().expect("platforms");
    let roms = client.all_roms().expect("roms");
    assert!(
        !roms.is_empty(),
        "live library is empty — scan the harness library first"
    );

    let server_id = server_id_of(&url);
    let catalogue = build_catalogue(&server_id, &platforms, &roms, |w| eprintln!("catalogue: {w}"))
        .expect("catalogue builds");
    eprintln!(
        "catalogue: {} platforms, {} roms, {} skipped",
        catalogue.platforms.len(),
        catalogue.entries.len(),
        catalogue.skipped_unsupported
    );
    assert!(catalogue.entries.len() >= 3, "expected >=3 single-file roms");

    // Verify every projected ROM has a committed source file to compare to.
    let harness_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testing/romm-harness/library/roms");
    if !harness_root.exists() {
        panic!("UNAVAILABLE: harness library not found at {}", harness_root.display());
    }

    let cache = tempfile::tempdir().expect("cache dir");
    let index = CacheIndex::open(cache.path()).expect("index");
    let live = Arc::new(LiveState::default());
    let expected: HashMap<RomKey, u64> =
        catalogue.entries.iter().map(|e| (e.key.clone(), e.size)).collect();
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
        Arc::new(ProjfsRemover::new(root.path().to_path_buf(), Arc::clone(&handle))),
    );
    let fs = Arc::new(RommFs::new(
        RommTree::new(catalogue),
        downloads,
        evictor,
        Arc::new(SystemClock),
    ));

    assert!(check_mount_root(root.path(), &server_id).is_ok());
    claim_mount_root(root.path(), &server_id).unwrap();
    let (mount, _h) =
        WindowsMount::mount_with_handle(fs, root.path(), handle).expect("mount on live catalogue");

    // Wait for projection, then enumerate + read every ROM through the OS.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let n = std::fs::read_dir(root.path()).map(|it| it.count()).unwrap_or(0);
        if n >= fs_platform_count(root.path()) && n > 0 {
            break;
        }
        assert!(Instant::now() < deadline, "projected dirs did not appear");
        std::thread::sleep(Duration::from_millis(100));
    }
    // Rebuild expectations from the live catalogue snapshot inside the FS:
    // read every platform dir, read every file, compare to harness source.
    let mut checked = 0usize;
    for plat in std::fs::read_dir(root.path()).unwrap().flatten() {
        let pname = plat.file_name().to_string_lossy().into_owned();
        if !plat.path().is_dir() {
            continue;
        }
        for rom in std::fs::read_dir(plat.path()).unwrap().flatten() {
            let rname = rom.file_name().to_string_lossy().into_owned();
            let expected = harness_rom(&pname, &rname);
            let want = std::fs::read(&expected)
                .unwrap_or_else(|_| panic!("no harness source for {pname}/{rname}"));
            let got = std::fs::read(rom.path()).expect("read through ProjFS");
            assert_eq!(got, want, "byte mismatch for {pname}/{rname}");
            // warm read hits the cache path, still byte-exact
            assert_eq!(std::fs::read(rom.path()).unwrap(), want);
            checked += 1;
            eprintln!("verified {pname}/{rname} ({} bytes)", want.len());
        }
    }
    assert!(checked >= 3, "expected >=3 roms verified, got {checked}");

    mount.stop();
    eprintln!("live E2E OK: {checked} roms byte-exact through ProjFS");
}

fn fs_platform_count(root: &Path) -> usize {
    std::fs::read_dir(root).map(|it| it.count()).unwrap_or(0)
}
