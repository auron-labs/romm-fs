//! "Hide RomM copies of games the user already has": local matching, and the
//! hidden-ownership rules that keep the user's own entries untouched.
use rommfs_core::catalogue::{build_catalogue, Catalogue};
use rommfs_core::retrobat::{
    generate_gamelists, FrontendGamelists, LocalFiles, MetadataIndex, MetadataPublication,
    PlayPush, PlaySync, HASH_CAP_BYTES,
};
use rommfs_core::romm::{PlatformDto, RomDto};
use rommfs_fixture::contract;
use serde_json::json;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;

/// CRC32, MD5 and SHA-1 of `abc`, the three verification paths RomM can supply.
const CRC_ABC: &str = "352441c2";
const MD5_ABC: &str = "900150983cd24fb0d6963f7d28e17f72";
const SHA1_ABC: &str = "a9993e364706816aba3e25717850c26c9cd0d89d";

fn rom(
    id: i64,
    name: &str,
    size: u64,
    crc: Option<&str>,
    md5: Option<&str>,
    sha1: Option<&str>,
) -> RomDto {
    let mut value = contract::rom(id, "gb", name, size, sha1.unwrap_or(""));
    value["files"][0]["crc_hash"] = json!(crc);
    value["files"][0]["md5_hash"] = json!(md5);
    value["files"][0]["sha1_hash"] = json!(sha1);
    serde_json::from_value(value).unwrap()
}

fn catalogue(roms: &[RomDto]) -> Catalogue {
    let platforms: Vec<PlatformDto> =
        serde_json::from_str(&contract::platforms(&[(1, "gb", "gb", "Game Boy")])).unwrap();
    build_catalogue("s", &platforms, roms, |_| {}).unwrap()
}

/// One metadata pass with play-data sync off.
fn generate(
    cat: &Catalogue,
    roms: &[RomDto],
    index: &MetadataIndex,
    frontend: &FrontendGamelists,
    owned: &HashSet<(i64, i64)>,
) -> MetadataPublication {
    generate_gamelists(
        cat,
        roms,
        "s",
        7,
        index,
        frontend,
        &BTreeMap::new(),
        &HashMap::new(),
        owned,
        &mut PlaySync {
            enabled: false,
            remote: &HashMap::new(),
            push: &mut |_: &PlayPush| Ok(()),
            now: 0,
        },
    )
    .unwrap()
}

fn gamelist(publication: &MetadataPublication) -> String {
    String::from_utf8(publication.gamelists()["gb/gamelist.xml"].clone()).unwrap()
}

/// The published gamelists fed back as the next pass's frontend state.
fn frontend(publication: &MetadataPublication) -> FrontendGamelists {
    let files = publication
        .gamelists()
        .iter()
        .map(|(path, bytes)| {
            (
                path.strip_suffix("/gamelist.xml").unwrap().to_string(),
                String::from_utf8(bytes.clone()).unwrap(),
            )
        })
        .collect();
    FrontendGamelists::new(files)
}

/// The cheap pass (name + size) is confirmed by whichever hash RomM provides,
/// a different size or hash never matches, and above the cap size is enough.
#[test]
fn matching_covers_name_size_hash_and_the_size_cap() {
    let dir = tempfile::tempdir().unwrap();
    let system = dir.path().join("roms/gb");
    fs::create_dir_all(&system).unwrap();
    // Same name and content: matches on crc, md5 or sha1.
    for name in ["Crc.gb", "Md5.gb", "Sha1.gb"] {
        fs::write(system.join(name), b"abc").unwrap();
    }
    // Same name, different content: the hash confirmation must fail.
    fs::write(system.join("Bad.gb"), b"xyz").unwrap();
    // Same name, different size: filtered out before any hashing.
    fs::write(system.join("Same.gb"), b"abc").unwrap();
    // RomM has no hash: name and size are all there is to compare.
    fs::write(system.join("None.gb"), b"abc").unwrap();
    // A game with several files counts as owned when its launch file matches,
    // even when that file sits in the game's own subfolder.
    fs::create_dir(system.join("Launch")).unwrap();
    fs::write(system.join("Launch/Launch.gb"), b"abc").unwrap();
    // Above the cap: never read, a name and size match is enough.
    let huge = std::fs::File::create(system.join("Huge.gb")).unwrap();
    huge.set_len(HASH_CAP_BYTES + 1).unwrap();

    let roms = vec![
        rom(1, "Crc.gb", 3, Some(CRC_ABC), None, None),
        rom(2, "Md5.gb", 3, None, Some(MD5_ABC), None),
        rom(3, "Sha1.gb", 3, None, None, Some(SHA1_ABC)),
        rom(4, "Bad.gb", 3, None, None, Some(SHA1_ABC)),
        rom(5, "Same.gb", 4, None, None, Some(SHA1_ABC)),
        rom(6, "None.gb", 3, None, None, None),
        rom(
            7,
            "Huge.gb",
            HASH_CAP_BYTES + 1,
            None,
            None,
            Some("0000000000000000000000000000000000000000"),
        ),
        serde_json::from_value(contract::rom_multi(
            8,
            "gb",
            &["Launch.gb", "Extra.dat"],
            &[3, 3],
        ))
        .unwrap(),
    ];
    let cat = catalogue(&roms);
    let mut files = LocalFiles::scan(dir.path(), &["gb".into()], None);
    let owned = files.owned(&cat, &roms);

    // contract::rom gives each file the ROM's own id.
    assert!(owned.contains(&(1, 1)), "crc match");
    assert!(owned.contains(&(2, 2)), "md5 match");
    assert!(owned.contains(&(3, 3)), "sha1 match");
    assert!(owned.contains(&(7, 7)), "above the cap: name and size only");
    assert!(!owned.contains(&(4, 4)), "hash mismatch");
    assert!(!owned.contains(&(5, 5)), "different size");
    // No hash to confirm with: the name and size match stands.
    assert!(owned.contains(&(6, 6)));
    // A game with several files: the matching launch file counts, the other
    // file does not have to be present.
    assert!(owned.contains(&(8, 8000)));
    assert!(!owned.contains(&(8, 8001)));
}

/// A local copy hides the RomM entry; removing the copy unhides it, and a
/// `hidden` the user set survives. The user's own entry is written verbatim.
#[test]
fn romm_copies_are_hidden_then_restored_without_touching_the_user() {
    let roms = vec![rom(1, "Game.gb", 3, None, None, Some(SHA1_ABC))];
    let cat = catalogue(&roms);
    let user_entry = "<game><path>./Mine.gb</path><name>Mine</name></game>";
    let user_frontend = FrontendGamelists::new(BTreeMap::from([(
        "gb".into(),
        format!("<gameList>{user_entry}</gameList>"),
    )]));
    let owned = HashSet::from([(1, 1)]);

    // The local copy exists: our entry is hidden, the user's is untouched.
    let hidden = generate(
        &cat,
        &roms,
        &MetadataIndex::default(),
        &user_frontend,
        &owned,
    );
    let xml = gamelist(&hidden);
    assert!(xml.contains("<hidden>true</hidden>"), "{xml}");
    assert!(xml.contains(user_entry), "{xml}");
    assert_eq!(hidden.index().auto_hidden_count(), 1);

    // The local copy goes away: only our own hidden flag is removed.
    let restored = generate(
        &cat,
        &roms,
        hidden.index(),
        &frontend(&hidden),
        &HashSet::new(),
    );
    let xml = gamelist(&restored);
    assert!(!xml.contains("<hidden>"), "{xml}");
    assert!(xml.contains(user_entry), "{xml}");
    assert_eq!(restored.index().auto_hidden_count(), 0);

    // A `hidden` the user set themselves (no auto-hide on record) survives.
    let user_hid = FrontendGamelists::new(BTreeMap::from([(
        "gb".into(),
        "<gameList><game><path>./RomM/Game.gb</path><hidden>true</hidden></game></gameList>".into(),
    )]));
    let kept = generate(
        &cat,
        &roms,
        &MetadataIndex::default(),
        &user_hid,
        &HashSet::new(),
    );
    assert!(gamelist(&kept).contains("<hidden>true</hidden>"));
    assert_eq!(kept.index().auto_hidden_count(), 0);
}

/// A system folder that can't be read is reported once and skipped; the other
/// systems still match. A local file that can't be hashed is skipped too.
#[cfg(unix)]
#[test]
fn unreadable_systems_and_files_are_skipped_without_failing() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("roms/gb")).unwrap();
    fs::create_dir_all(dir.path().join("roms/nes")).unwrap();
    fs::write(dir.path().join("roms/gb/Game.gb"), b"abc").unwrap();
    let unreadable_file = dir.path().join("roms/gb/Locked.gb");
    fs::write(&unreadable_file, b"abc").unwrap();
    // Unreadable with no hash to compare, and above the cap: still no match.
    let unreadable_nohash = dir.path().join("roms/gb/Nohash.gb");
    fs::write(&unreadable_nohash, b"abc").unwrap();
    let unreadable_huge = dir.path().join("roms/gb/Huge.gb");
    std::fs::File::create(&unreadable_huge)
        .unwrap()
        .set_len(HASH_CAP_BYTES + 1)
        .unwrap();
    // A second system that cannot be listed at all.
    let nes = dir.path().join("roms/nes");
    fs::set_permissions(&nes, fs::Permissions::from_mode(0o000)).unwrap();
    let nes_unreadable = fs::read_dir(&nes).is_err();

    let roms = vec![
        rom(1, "Game.gb", 3, None, None, Some(SHA1_ABC)),
        rom(2, "Locked.gb", 3, None, None, Some(SHA1_ABC)),
        rom(3, "Linked.gb", 3, None, None, Some(SHA1_ABC)),
        rom(4, "Linked.gb", 3, None, None, None),
        rom(5, "Shortcut.gb", 3, None, None, Some(SHA1_ABC)),
        rom(6, "Nohash.gb", 3, None, None, None),
        rom(7, "Huge.gb", HASH_CAP_BYTES + 1, None, None, None),
    ];
    let cat = catalogue(&roms);

    // A plain, real `RomM` folder is RomMFS's own, not the user's, whatever it
    // contains. A symlinked folder is never followed either, even when a hash
    // would have confirmed the match.
    fs::create_dir_all(dir.path().join("roms/gb/RomM")).unwrap();
    fs::write(dir.path().join("roms/gb/RomM/Linked.gb"), b"abc").unwrap();
    let elsewhere = dir.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::write(elsewhere.join("Shortcut.gb"), b"abc").unwrap();
    std::os::unix::fs::symlink(&elsewhere, dir.path().join("roms/gb/Shortcut")).unwrap();

    // Lock the files after they are scanned; reading them then fails.
    fs::set_permissions(&unreadable_file, fs::Permissions::from_mode(0o000)).unwrap();
    fs::set_permissions(&unreadable_nohash, fs::Permissions::from_mode(0o000)).unwrap();
    fs::set_permissions(&unreadable_huge, fs::Permissions::from_mode(0o000)).unwrap();
    let file_unreadable = fs::File::open(&unreadable_file).is_err();
    let nohash_unreadable = fs::File::open(&unreadable_nohash).is_err();
    let huge_unreadable = fs::File::open(&unreadable_huge).is_err();
    let mut files = LocalFiles::scan(dir.path(), &["gb".into(), "nes".into()], None);
    // Keep the locks through matching, then restore so the tempdir can be removed.
    let owned = files.owned(&cat, &roms);
    for path in [&unreadable_file, &unreadable_nohash, &unreadable_huge] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap();
    }
    fs::set_permissions(&nes, fs::Permissions::from_mode(0o755)).unwrap();

    assert!(owned.contains(&(1, 1)), "the readable match still counts");
    assert!(
        !owned.contains(&(3, 3)),
        "a real RomM folder is not the user's own"
    );
    assert!(
        !owned.contains(&(4, 4)),
        "a name+size match inside RomM is still excluded"
    );
    assert!(
        !owned.contains(&(5, 5)),
        "a symlinked folder is never followed"
    );
    if nes_unreadable {
        assert!(files.problems().iter().any(|(system, _)| system == "nes"));
    }
    if file_unreadable {
        assert!(!owned.contains(&(2, 2)), "an unreadable file never matches");
    }
    if nohash_unreadable {
        assert!(
            !owned.contains(&(6, 6)),
            "an unreadable file with no hash never matches"
        );
    }
    if huge_unreadable {
        assert!(
            !owned.contains(&(7, 7)),
            "an unreadable file above the cap never matches"
        );
    }
}

/// A linked system root, or a linked `roms` root from an earlier layout, is
/// never followed: its contents may be RomMFS's managed tree.
#[cfg(unix)]
#[test]
fn linked_roots_are_never_followed() {
    use std::os::unix::fs::symlink;

    let roms = vec![rom(1, "Game.gb", 3, None, None, Some(SHA1_ABC))];
    let cat = catalogue(&roms);

    // A linked system root.
    let dir = tempfile::tempdir().unwrap();
    let elsewhere = dir.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::write(elsewhere.join("Game.gb"), b"abc").unwrap();
    fs::create_dir_all(dir.path().join("roms")).unwrap();
    symlink(&elsewhere, dir.path().join("roms/gb")).unwrap();
    let mut files = LocalFiles::scan(dir.path(), &["gb".into()], None);
    assert!(!files.owned(&cat, &roms).contains(&(1, 1)));

    // A linked `roms` root (the earlier schema-1 layout).
    let dir = tempfile::tempdir().unwrap();
    let elsewhere = dir.path().join("elsewhere");
    fs::create_dir_all(elsewhere.join("gb")).unwrap();
    fs::write(elsewhere.join("gb/Game.gb"), b"abc").unwrap();
    symlink(&elsewhere, dir.path().join("roms")).unwrap();
    let mut files = LocalFiles::scan(dir.path(), &["gb".into()], None);
    assert!(!files.owned(&cat, &roms).contains(&(1, 1)));
}

/// The hash cache is keyed on a precise timestamp, and a file the user can't
/// read never matches even when a hash is already cached.
#[cfg(unix)]
#[test]
fn the_hash_cache_is_precise_and_never_trusts_an_unreadable_file() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::UNIX_EPOCH;

    let dir = tempfile::tempdir().unwrap();
    let system = dir.path().join("roms/gb");
    fs::create_dir_all(&system).unwrap();
    let game = system.join("Game.gb");
    fs::write(&game, b"abc").unwrap();
    let cache = dir.path().join("cache/local-hashes.json");
    let roms = vec![rom(1, "Game.gb", 3, None, None, Some(SHA1_ABC))];
    let cat = catalogue(&roms);
    let stamp = |path: &std::path::Path| {
        fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
    };

    // First pass hashes and caches the file.
    let mut files = LocalFiles::scan(dir.path(), &["gb".into()], Some(cache.clone()));
    assert!(files.owned(&cat, &roms).contains(&(1, 1)));
    files.save_cache();
    assert!(cache.is_file());
    let first = stamp(&game).unwrap();

    // A cached file that is now unreadable must not match: the open is the
    // gate, not the cache.
    fs::set_permissions(&game, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::File::open(&game).is_err() {
        let mut locked = LocalFiles::scan(dir.path(), &["gb".into()], Some(cache.clone()));
        assert!(!locked.owned(&cat, &roms).contains(&(1, 1)));
    }
    fs::set_permissions(&game, fs::Permissions::from_mode(0o644)).unwrap();

    // A same-size edit inside the same second still invalidates the cache.
    fs::write(&game, b"xyz").unwrap();
    if stamp(&game).unwrap() != first {
        let mut edited = LocalFiles::scan(dir.path(), &["gb".into()], Some(cache));
        assert!(
            !edited.owned(&cat, &roms).contains(&(1, 1)),
            "a same-size edit must be re-hashed, not served from the cache"
        );
    }
}
