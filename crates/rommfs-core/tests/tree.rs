//! Behavioral tests for `RommTree` (PRD R2): enumeration of
//! root/platforms/files with correct types and sizes, case-insensitive
//! lookup, stable inodes, and cookie-paged `read_dir` that neither loses
//! nor duplicates entries.

use rommfs_core::catalog::build_catalogue;
use rommfs_core::romm::{PlatformDto, RomDto};
use rommfs_core::tree::{EntryKind, RommTree, ROOT_INODE};
use rommfs_fixture::contract;

fn platforms(items: &[(i64, &str, &str, &str)]) -> Vec<PlatformDto> {
    serde_json::from_str(&contract::platforms(items)).unwrap()
}

fn rom(id: i64, fs_slug: &str, file_name: &str, size: u64) -> RomDto {
    serde_json::from_value(contract::rom(id, fs_slug, file_name, size, "hash")).unwrap()
}

fn sample_tree() -> RommTree {
    let cat = build_catalogue(
        "srv",
        &platforms(&[(1, "nes", "nes", "NES"), (2, "snes", "snes", "SNES")]),
        &[
            rom(10, "nes", "a.nes", 100),
            rom(11, "nes", "b.nes", 200),
            rom(12, "nes", "c.nes", 300),
            rom(20, "snes", "d.sfc", 400),
        ],
        |_| {},
    )
    .unwrap();
    RommTree::new(cat)
}

/// Fully drain a directory through `read_dir`, returning the names.
fn drain(tree: &RommTree, inode: u64, page: usize) -> Vec<String> {
    let mut names = Vec::new();
    let mut cookie = 0;
    loop {
        let (entries, next, eof) = tree.read_dir(inode, cookie, page).unwrap();
        names.extend(entries.into_iter().map(|e| e.name));
        if eof {
            break;
        }
        assert!(next > cookie, "paging must make progress");
        cookie = next;
    }
    names
}

#[test]
fn root_and_platform_dirs_enumerate_without_downloads() {
    let tree = sample_tree();

    // Root lists platform dirs, sorted and marked Directory.
    let (entries, _next, eof) = tree.read_dir(ROOT_INODE, 0, 64).unwrap();
    assert!(eof);
    assert_eq!(
        entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        vec!["nes", "snes"]
    );
    assert!(entries.iter().all(|e| e.kind == EntryKind::Directory));

    // A platform dir lists its ROM files sorted, with file kind.
    let nes_dir = tree.lookup(ROOT_INODE, "nes").unwrap();
    let names = drain(&tree, nes_dir.inode, 64);
    assert_eq!(names, vec!["a.nes", "b.nes", "c.nes"]);

    let snes_dir = tree.lookup(ROOT_INODE, "snes").unwrap();
    assert_eq!(drain(&tree, snes_dir.inode, 64), vec!["d.sfc"]);
}

#[test]
fn metadata_reports_contract_sizes_and_kinds() {
    let tree = sample_tree();

    let root = tree.metadata(ROOT_INODE).unwrap();
    assert_eq!(root.kind, EntryKind::Directory);

    let nes_dir = tree.lookup(ROOT_INODE, "nes").unwrap();
    assert_eq!(
        tree.metadata(nes_dir.inode).unwrap().kind,
        EntryKind::Directory
    );

    let b = tree.lookup(nes_dir.inode, "b.nes").unwrap();
    assert_eq!(b.kind, EntryKind::File);
    assert_eq!(b.size, 200);
    assert_eq!(b.parent, nes_dir.inode);
    assert_eq!(tree.metadata(b.inode).unwrap().size, 200);

    // Unknown inode / unknown name fail clean.
    assert!(tree.metadata(9999).is_none());
    assert!(tree.lookup(nes_dir.inode, "nope.nes").is_none());
    assert!(tree.lookup(9999, "x").is_none());
}

#[test]
fn read_dir_cookie_paging_loses_and_duplicates_nothing() {
    let tree = sample_tree();
    let nes_dir = tree.lookup(ROOT_INODE, "nes").unwrap();

    // Page size 1 over 3 files + root over 2 dirs: every entry exactly once.
    assert_eq!(
        drain(&tree, nes_dir.inode, 1),
        vec!["a.nes", "b.nes", "c.nes"]
    );
    assert_eq!(drain(&tree, ROOT_INODE, 1), vec!["nes", "snes"]);

    // A page boundary mid-directory resumes at the right place.
    let (first, next, eof) = tree.read_dir(nes_dir.inode, 0, 2).unwrap();
    assert_eq!(first.len(), 2);
    assert!(!eof);
    let (rest, _next, eof) = tree.read_dir(nes_dir.inode, next, 2).unwrap();
    assert_eq!(
        rest.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        vec!["c.nes"]
    );
    assert!(eof);
}

#[test]
fn lookup_and_paths_are_case_insensitive() {
    let tree = sample_tree();

    let lower = tree.lookup(ROOT_INODE, "nes").unwrap();
    let upper = tree.lookup(ROOT_INODE, "NES").unwrap();
    assert_eq!(lower.inode, upper.inode);

    let file_a = tree.lookup(lower.inode, "a.nes").unwrap();
    let file_b = tree.lookup(lower.inode, "A.NES").unwrap();
    assert_eq!(file_a.inode, file_b.inode);

    assert_eq!(tree.inode_for_path(&["NES", "A.NES"]), Some(file_a.inode));
    assert_eq!(tree.inode_for_path(&[]), Some(ROOT_INODE));
    assert_eq!(tree.inode_for_path(&["nope"]), None);

    // Path round-trip used by the adapter and eviction.
    assert_eq!(tree.path_of(file_a.inode).as_deref(), Some("nes/a.nes"));
    // File inode resolves back to its catalogue row for download wiring.
    assert_eq!(tree.rom_index_of(file_a.inode), Some(0));
    assert_eq!(tree.rom_index_of(ROOT_INODE), None);
}

#[test]
fn inodes_are_stable_across_rebuilds() {
    let a = sample_tree();
    let b = sample_tree();

    for path in [
        &["nes"][..],
        &["snes"][..],
        &["nes", "a.nes"][..],
        &["nes", "b.nes"][..],
        &["snes", "d.sfc"][..],
    ] {
        assert_eq!(a.inode_for_path(path), b.inode_for_path(path));
    }
}
