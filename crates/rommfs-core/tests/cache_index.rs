//! Behavioral tests for the durable `CacheIndex` (PRD R4, §6): completed
//! state survives reopen, version metadata invalidates, failed rows are
//! never servable, last-use tracking is explicit, and startup recovery
//! drops interrupted state.

use rommfs_core::cache::{CacheIndex, EntryState};
use rommfs_core::catalog::RomKey;

fn key(rom_id: i64, file_id: i64) -> RomKey {
    RomKey {
        server_id: "http://fixture".into(),
        rom_id,
        file_id,
    }
}

#[test]
fn ready_entry_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let k = key(7, 70);
    {
        let mut index = CacheIndex::open(dir.path()).unwrap();
        index
            .mark_ready(&k, 4096, Some("sha1:abc"), "nes/Game.nes")
            .unwrap();
    }
    let index = CacheIndex::open(dir.path()).unwrap();
    let rec = index
        .ready_if_current(&k, Some("sha1:abc"))
        .unwrap()
        .expect("row must be ready after reopen");
    assert_eq!(rec.state, EntryState::Ready);
    assert_eq!(rec.size_bytes, 4096);
    assert_eq!(rec.version.as_deref(), Some("sha1:abc"));
    assert_eq!(rec.rel_path, "nes/Game.nes");
    assert_eq!(rec.path, index.bin_path(&k));
}

#[test]
fn version_change_makes_entry_not_ready() {
    let dir = tempfile::tempdir().unwrap();
    let mut index = CacheIndex::open(dir.path()).unwrap();
    let k = key(7, 70);
    index
        .mark_ready(&k, 10, Some("v1"), "nes/Game.nes")
        .unwrap();

    // Same version: still current.
    assert!(index.ready_if_current(&k, Some("v1")).unwrap().is_some());
    // A newer catalogue version: stale bytes are never served.
    assert!(index.ready_if_current(&k, Some("v2")).unwrap().is_none());
    // A catalogue that supplies no version cannot invalidate.
    assert!(index.ready_if_current(&k, None).unwrap().is_some());

    // An unversioned row is not current for a now-versioned catalogue.
    let k2 = key(8, 80);
    index.mark_ready(&k2, 10, None, "nes/Other.nes").unwrap();
    assert!(index.ready_if_current(&k2, Some("v2")).unwrap().is_none());
    assert!(index.ready_if_current(&k2, None).unwrap().is_some());
}

#[test]
fn failed_state_is_never_served() {
    let dir = tempfile::tempdir().unwrap();
    let mut index = CacheIndex::open(dir.path()).unwrap();
    let k = key(7, 70);
    index
        .mark_ready(&k, 10, Some("v1"), "nes/Game.nes")
        .unwrap();
    index.mark_failed(&k).unwrap();
    assert!(index.ready_if_current(&k, Some("v1")).unwrap().is_none());
    assert!(index.ready_entries().unwrap().is_empty());

    // mark_failed on a never-seen key also yields no ready entry.
    let k2 = key(9, 90);
    index.mark_failed(&k2).unwrap();
    assert!(index.ready_if_current(&k2, None).unwrap().is_none());
}

#[test]
fn touch_updates_last_use() {
    let dir = tempfile::tempdir().unwrap();
    let mut index = CacheIndex::open(dir.path()).unwrap();
    let k = key(7, 70);
    index.mark_ready(&k, 10, None, "nes/Game.nes").unwrap();

    index.touch(&k, 123_456).unwrap();
    let rec = index.ready_if_current(&k, None).unwrap().unwrap();
    assert_eq!(rec.last_used_unix_secs, 123_456);
}

#[test]
fn reopen_recovers_interrupted_state() {
    let dir = tempfile::tempdir().unwrap();
    let k = key(7, 70);
    let mut index = CacheIndex::open(dir.path()).unwrap();
    index.mark_failed(&k).unwrap();
    // A staging file left behind by a crash mid-download.
    let part = index.part_path(&k);
    std::fs::write(&part, b"partial").unwrap();
    drop(index);

    let index = CacheIndex::open(dir.path()).unwrap();
    assert!(!part.exists(), "leftover .part must be removed on open");
    assert!(index.ready_if_current(&k, None).unwrap().is_none());
    assert!(index.ready_entries().unwrap().is_empty());
}

#[test]
fn entries_are_scoped_to_identity() {
    // Different server/rom/file identities never share a ready entry —
    // the aliasing guard PRD R2/R4 requires.
    let dir = tempfile::tempdir().unwrap();
    let mut index = CacheIndex::open(dir.path()).unwrap();
    let k = key(7, 70);
    index.mark_ready(&k, 10, None, "nes/Game.nes").unwrap();

    let other_server = RomKey {
        server_id: "http://other".into(),
        ..k.clone()
    };
    let other_file = key(7, 71);
    assert!(index
        .ready_if_current(&other_server, None)
        .unwrap()
        .is_none());
    assert!(index.ready_if_current(&other_file, None).unwrap().is_none());
    assert!(bin_and_part_names_differ(&index, &k, &other_server));
}

fn bin_and_part_names_differ(index: &CacheIndex, a: &RomKey, b: &RomKey) -> bool {
    index.bin_path(a) != index.bin_path(b) && index.part_path(a) != index.part_path(b)
}
