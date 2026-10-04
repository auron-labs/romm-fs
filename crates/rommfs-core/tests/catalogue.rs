//! Behavioral tests for `build_catalogue` + `server_id_of` (PRD R1/R2):
//! platform dirs from sanitized fs_slug (dedupe + disambiguation), the
//! single-file rule, visible-name sanitization, per-dir collision handling,
//! version fingerprints, and a deterministic inode plan.

use rommfs_core::catalog::{build_catalogue, server_id_of, Catalogue, VersionKey};
use rommfs_core::romm::{PlatformDto, RomDto};
use rommfs_fixture::contract;
use serde_json::{json, Value};

fn platforms(items: &[(i64, &str, &str, &str)]) -> Vec<PlatformDto> {
    serde_json::from_str(&contract::platforms(items)).unwrap()
}

fn rom(id: i64, fs_slug: &str, file_name: &str, size: u64, sha1: &str) -> RomDto {
    serde_json::from_value(contract::rom(id, fs_slug, file_name, size, sha1)).unwrap()
}

fn rom_multi(id: i64, fs_slug: &str, names: &[&str], sizes: &[u64]) -> RomDto {
    serde_json::from_value(contract::rom_multi(id, fs_slug, names, sizes)).unwrap()
}

/// A single-file ROM whose one file carries custom hash/modified metadata.
fn rom_with_file(id: i64, fs_slug: &str, file_name: &str, file: Value) -> RomDto {
    serde_json::from_value(json!({
        "id": id,
        "platform_fs_slug": fs_slug,
        "platform_slug": fs_slug,
        "fs_name": file_name,
        "fs_size_bytes": file["file_size_bytes"],
        "has_simple_single_file": true,
        "has_nested_single_file": false,
        "has_multiple_files": false,
        "missing_from_fs": false,
        "is_physical": true,
        "updated_at": "2026-10-01T00:00:00",
        "files": [file],
    }))
    .unwrap()
}

fn file_json(id: i64, name: &str, size: u64) -> Value {
    json!({
        "id": id,
        "file_name": name,
        "file_size_bytes": size,
        "last_modified": "2026-10-01T12:00:00",
        "crc_hash": null,
        "md5_hash": null,
        "sha1_hash": null,
        "is_top_level": true,
    })
}

fn entry_names<'a>(cat: &'a Catalogue, dir: &str) -> Vec<&'a str> {
    cat.entries
        .iter()
        .filter(|e| e.platform_dir == dir)
        .map(|e| e.file_name.as_str())
        .collect()
}

#[test]
fn single_file_roms_become_visible_entries() {
    let cat = build_catalogue(
        "srv",
        &platforms(&[(1, "nes", "nes", "Nintendo"), (2, "snes", "snes", "Super")]),
        &[
            rom(10, "nes", "Example Game.nes", 4096, "aaa"),
            rom(11, "snes", "Another Game.sfc", 8192, "bbb"),
        ],
        |_| {},
    )
    .unwrap();

    assert_eq!(cat.platforms, vec!["nes", "snes"]);
    assert_eq!(cat.skipped_unsupported, 0);
    assert_eq!(entry_names(&cat, "nes"), vec!["Example Game.nes"]);
    assert_eq!(entry_names(&cat, "snes"), vec!["Another Game.sfc"]);
    let e = &cat.entries[0];
    assert_eq!(e.size, 4096);
    assert_eq!(e.content_name, "Example Game.nes");
    assert_eq!(e.key.rom_id, 10);
    assert_eq!(e.key.server_id, "srv");
    assert_eq!(e.version, Some(VersionKey("sha1:aaa".into())));
}

#[test]
fn multi_file_roms_are_skipped_and_counted() {
    let mut warned = Vec::new();
    let cat = build_catalogue(
        "srv",
        &platforms(&[(1, "psx", "psx", "PlayStation")]),
        &[
            rom_multi(20, "psx", &["disc.cue", "disc.bin"], &[100, 5000]),
            rom(21, "psx", "Single.bin", 5000, "x"),
        ],
        |m| warned.push(m),
    )
    .unwrap();

    assert_eq!(cat.skipped_unsupported, 1);
    assert_eq!(entry_names(&cat, "psx"), vec!["Single.bin"]);
    assert!(!warned.is_empty(), "a skip must be logged");
}

#[test]
fn invalid_names_skip_while_invalid_chars_adjust() {
    let cat = build_catalogue(
        "srv",
        &platforms(&[(1, "nes", "nes", "NES")]),
        &[
            rom_with_file(30, "nes", "a/b.nes", file_json(30, "a/b.nes", 10)), // separator: rejected
            rom_with_file(31, "nes", "con.nes", file_json(31, "con.nes", 10)), // reserved: adjusted
            rom_with_file(32, "nes", "bad:name.nes", file_json(32, "bad:name.nes", 10)), // ':' mapped
        ],
        |_| {},
    )
    .unwrap();

    assert_eq!(cat.skipped_unsupported, 1);
    let names = entry_names(&cat, "nes");
    assert_eq!(names, vec!["bad_name.nes", "con_.nes"]);
}

#[test]
fn filename_collisions_disambiguate_case_insensitively() {
    let cat = build_catalogue(
        "srv",
        &platforms(&[(1, "nes", "nes", "NES")]),
        &[
            rom(40, "nes", "game.nes", 1, "a"),
            rom(41, "nes", "GAME.nes", 2, "b"),
            rom(42, "nes", "Game.nes", 3, "c"),
            rom(43, "nes", "other.nes", 4, "d"),
        ],
        |_| {},
    )
    .unwrap();

    let mut names = entry_names(&cat, "nes");
    names.sort();
    // Three case-insensitive colliders each get a distinct deterministic name;
    // nothing merges or overwrites.
    assert_eq!(
        names,
        vec!["GAME (2).nes", "Game (3).nes", "game.nes", "other.nes"]
    );
    assert_eq!(cat.skipped_unsupported, 0);
}

#[test]
fn platform_dirs_dedupe_case_insensitively() {
    let cat = build_catalogue(
        "srv",
        &platforms(&[
            (1, "nes", "nes", "NES"),
            (2, "nes2", "NES", "NES alt"),
            (3, "gb", "gb", "Game Boy"),
        ]),
        &[rom(50, "NES", "x.nes", 1, "h")],
        |_| {},
    )
    .unwrap();

    // "nes" + "NES" collide case-insensitively -> second gets a suffix.
    assert_eq!(cat.platforms, vec!["NES (2)", "gb", "nes"]);
    // The ROM on the second platform lands in its deduped dir.
    assert_eq!(entry_names(&cat, "NES (2)"), vec!["x.nes"]);
}

#[test]
fn roms_on_rejected_platforms_are_skipped() {
    let cat = build_catalogue(
        "srv",
        &platforms(&[(1, "bad", "a/b", "Broken")]),
        &[rom(60, "a/b", "x.nes", 1, "h")],
        |_| {},
    )
    .unwrap();

    assert_eq!(cat.skipped_unsupported, 1);
    assert!(cat.entries.is_empty());
    assert!(cat.platforms.is_empty());
}

#[test]
fn version_key_prefers_strongest_hash_then_composite() {
    let f = |id: i64,
             name: &str,
             crc: Option<&str>,
             md5: Option<&str>,
             sha1: Option<&str>,
             lm: Option<&str>| {
        let file = json!({
            "id": id, "file_name": name, "file_size_bytes": 7,
            "last_modified": lm, "crc_hash": crc, "md5_hash": md5,
            "sha1_hash": sha1, "is_top_level": true,
        });
        rom_with_file(id, "nes", name, file)
    };
    let cat = build_catalogue(
        "srv",
        &platforms(&[(1, "nes", "nes", "NES")]),
        &[
            f(81, "a", Some("c"), Some("m"), Some("s"), Some("t")),
            f(82, "b", Some("c"), Some("m"), None, Some("t")),
            f(83, "c", Some("c"), None, None, Some("t")),
            f(84, "d", None, None, None, Some("t")),
            f(85, "e", None, None, None, None),
        ],
        |_| {},
    )
    .unwrap();

    let v: Vec<Option<VersionKey>> = cat.entries.iter().map(|e| e.version.clone()).collect();
    assert_eq!(v[0], Some(VersionKey("sha1:s".into())));
    assert_eq!(v[1], Some(VersionKey("md5:m".into())));
    assert_eq!(v[2], Some(VersionKey("crc:c".into())));
    assert_eq!(v[3], Some(VersionKey("lastmod:t+size:7".into())));
    assert_eq!(v[4], None);
}

#[test]
fn inode_plan_is_deterministic_for_an_unchanged_catalogue() {
    let build = || {
        build_catalogue(
            "srv",
            &platforms(&[(1, "nes", "nes", "NES"), (2, "gb", "gb", "GB")]),
            &[
                rom(70, "nes", "b.nes", 1, "h"),
                rom(71, "nes", "a.nes", 1, "h"),
                rom(72, "gb", "c.gb", 1, "h"),
            ],
            |_| {},
        )
        .unwrap()
    };
    let a = build();
    let b = build();
    assert_eq!(a.platforms, b.platforms);
    assert_eq!(a.inode_plan(), b.inode_plan());

    // Sorted dirs start at 2; files follow sequentially, grouped per dir.
    assert_eq!(a.platforms, vec!["gb", "nes"]);
    let plan = a.inode_plan();
    assert_eq!(plan.len(), 3);
    // gb dir = inode 2, nes dir = inode 3; files start at 4.
    assert_eq!(plan[0].2, 0); // first entry is gb/c.gb (dirs sort first)
    for &(inode, parent, _) in plan {
        assert!(inode >= 4);
        assert!(parent == 2 || parent == 3);
    }
}

#[test]
fn server_id_normalizes_scheme_host_port() {
    assert_eq!(server_id_of("http://example.com"), "http://example.com");
    assert_eq!(server_id_of("http://example.com/"), "http://example.com");
    assert_eq!(server_id_of("HTTP://Example.COM:80/"), "http://example.com");
    assert_eq!(
        server_id_of("https://Example.COM:443/x"),
        "https://example.com"
    );
    assert_eq!(
        server_id_of("http://example.com:8080/api"),
        "http://example.com:8080"
    );
    assert_eq!(server_id_of("romm.local"), "http://romm.local");
    assert_eq!(
        server_id_of("https://user:secret@example.com:9443/"),
        "https://example.com:9443"
    );
}
