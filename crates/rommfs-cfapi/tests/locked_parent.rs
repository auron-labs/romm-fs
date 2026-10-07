//! Mount roots whose parent rejects new files (the `C:\RomM` default: Windows
//! lets standard users create *directories* at a drive root but not *files*,
//! and hardened/Server ACLs may deny both). Ownership manifests and the mount
//! lock must still work from the per-user fallback store.
//!
//! Exercises real ACLs through `icacls`; missing prerequisites fail loudly.

#![cfg(windows)]

use rommfs_cfapi::{check_mount_root, claim_mount_root, RootCheck, WindowsMount};
use rommfs_core::cache::{CacheIndex, Evictor, FakeClock, LiveState, NoopHydratedRemover};
use rommfs_core::catalog::{build_catalogue, RomKey};
use rommfs_core::download::{ContentSource, DownloadManager};
use rommfs_core::error::Result;
use rommfs_core::fscore::RommFs;
use rommfs_core::tree::RommTree;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

fn current_user() -> String {
    format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").expect("USERDOMAIN"),
        std::env::var("USERNAME").expect("USERNAME")
    )
}

fn icacls(dir: &Path, args: &[&str]) {
    let output = Command::new("icacls")
        .arg(dir)
        .args(args)
        .output()
        .expect("run icacls");
    assert!(
        output.status.success(),
        "icacls {args:?} failed on {}: {}",
        dir.display(),
        String::from_utf8_lossy(&output.stdout)
    );
}

/// Deny `perms` (e.g. `(WD)` files-only, `(WD,AD)` files and directories) on
/// the directory object itself — contents of pre-created children stay
/// writable, matching a locked-down parent directory. Restored on drop.
struct DenyGuard {
    dir: PathBuf,
    user: String,
}

impl DenyGuard {
    fn new(dir: &Path, perms: &str) -> Self {
        let user = current_user();
        icacls(dir, &["/deny", &format!("{user}:{perms}")]);
        Self {
            dir: dir.to_path_buf(),
            user,
        }
    }
}

impl Drop for DenyGuard {
    fn drop(&mut self) {
        icacls(&self.dir, &["/remove:d", &self.user]);
    }
}

struct NoSource;
impl ContentSource for NoSource {
    fn fetch(
        &self,
        _key: &RomKey,
        _writer: &mut dyn Write,
        _progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<u64> {
        unreachable!("no catalogue entries exist to fetch")
    }
}

fn empty_fs() -> Arc<RommFs> {
    let catalogue = build_catalogue("server", &[], &[], |w| panic!("{w}")).unwrap();
    let cache = tempfile::tempdir().unwrap().keep();
    let index = CacheIndex::open(cache).unwrap();
    let live = Arc::new(LiveState::default());
    let (sink, _rx) = rommfs_core::events::channel();
    let downloads = Arc::new(DownloadManager::new(
        index,
        Arc::clone(&live),
        Arc::new(NoSource),
        sink,
        HashMap::new(),
        HashMap::new(),
    ));
    let evictor = Evictor::new(u64::MAX, live, Arc::new(NoopHydratedRemover));
    Arc::new(RommFs::new(
        RommTree::new(catalogue),
        downloads,
        evictor,
        Arc::new(FakeClock::new(1_000_000)),
    ))
}

/// A parent that denies file creation but allows directories — exactly the
/// default ACL semantics standard users get at a client drive root.
#[test]
fn mount_root_under_file_locked_parent_mounts_via_fallback_sidecars() {
    let base = tempfile::tempdir().unwrap();
    let parent = base.path().join("locked");
    std::fs::create_dir(&parent).unwrap();
    let root = parent.join("RomM");
    std::fs::create_dir(&root).unwrap();
    let _deny = DenyGuard::new(&parent, "(WD)");

    // Ownership claim must not need a sibling file beside the root.
    claim_mount_root(&root, "server").unwrap();
    assert!(matches!(
        check_mount_root(&root, "server").unwrap(),
        RootCheck::RecognizedOwned
    ));
    assert!(
        !parent.join("RomM.rommfs-root").exists(),
        "a denied parent must not hold the ownership marker"
    );

    // Mount, remount, and stop all work without a sibling lock file.
    let mount = WindowsMount::mount(empty_fs(), &root).expect("mount under locked parent");
    mount.stop();
    let remount = WindowsMount::mount(empty_fs(), &root).expect("remount");
    remount.stop();
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
}

/// Claiming through an 8.3 short-name alias must not strand the sidecars:
/// the backend canonicalizes the root before reading, so the alias spelling
/// and the resolved path have to name the same files. A junction or symlink
/// root is refused as a reparse point, which makes the 8.3 name the one
/// spelling that can reach claim and mount differently.
#[test]
fn claiming_through_a_short_name_alias_resolves_with_the_canonical_path() {
    let base = tempfile::tempdir().unwrap();
    let real = base.path().join("Long Name Root");
    std::fs::create_dir(&real).unwrap();
    // fsutil assigns an explicit 8.3 alias even where auto-generation is off.
    let status = Command::new("fsutil")
        .args(["file", "setshortname"])
        .arg(&real)
        .arg("LONRNA~1")
        .output()
        .expect("run fsutil setshortname");
    if !status.status.success() {
        eprintln!(
            "short-name aliases unavailable on this host, skipping: {}",
            String::from_utf8_lossy(&status.stderr)
        );
        return;
    }
    let alias = base.path().join("LONRNA~1");
    assert!(alias.exists());
    assert_eq!(
        std::fs::canonicalize(&alias).unwrap(),
        std::fs::canonicalize(&real).unwrap()
    );

    claim_mount_root(&alias, "server").unwrap();
    assert!(
        base.path().join("Long Name Root.rommfs-root").exists(),
        "claim must land beside the canonical root, not the alias spelling"
    );
    assert!(
        matches!(
            check_mount_root(&real, "server").unwrap(),
            RootCheck::RecognizedOwned
        ),
        "mount-path lookup must find a claim made through the alias"
    );
}

/// With a writable parent the sidecars stay beside the root, preserving the
/// layout earlier releases created and every existing claim relies on.
#[test]
fn sidecars_stay_beside_writable_parent() {
    let base = tempfile::tempdir().unwrap();
    let parent = base.path().to_path_buf();
    let root = parent.join("RomM");
    std::fs::create_dir(&root).unwrap();

    claim_mount_root(&root, "server").unwrap();
    assert!(
        parent.join("RomM.rommfs-root").exists(),
        "ownership marker must stay beside the root when the parent is writable"
    );

    let mount = WindowsMount::mount(empty_fs(), &root).expect("mount");
    assert!(
        parent.join("RomM.rommfs-lock").exists(),
        "mount lock must stay beside the root when the parent is writable"
    );
    mount.stop();
}
