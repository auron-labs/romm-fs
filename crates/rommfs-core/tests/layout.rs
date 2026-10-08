use rommfs_core::catalog::{build_catalogue, build_catalogue_for_layout};
use rommfs_core::layout::RetroBatSystems;
use rommfs_core::romm::{PlatformDto, RomDto};
use rommfs_core::tree::{EntryKind, RommTree};
use rommfs_fixture::contract;
use serde_json::json;

fn platforms(slug: &str, fs_slug: &str) -> Vec<PlatformDto> {
    serde_json::from_str(&contract::platforms(&[(1, slug, fs_slug, "Platform")])).unwrap()
}
fn nested() -> RomDto {
    let mut rom = contract::rom_multi(7, "Sony Games", &["Disc.cue", "Disc.bin"], &[23, 99]);
    rom["platform_slug"] = json!("ps");
    rom["fs_path"] = json!("roms/Sony Games");
    rom["fs_name"] = json!("Game");
    for file in rom["files"].as_array_mut().unwrap() {
        file["file_path"] = json!("roms/Sony Games/Game/discs");
        file["is_top_level"] = json!(false);
        file["category"] = json!("game");
    }
    serde_json::from_value(rom).unwrap()
}
fn systems() -> RetroBatSystems {
    RetroBatSystems::from_xml(r#"<systemList>
      <system><name>psx</name><path>~/../roms/psx</path><platform>psx</platform><extension>.cue .chd</extension></system>
      <system><name>switch</name><path>~/../roms/switch</path><platform>switch</platform><extension>.nsp .xci</extension></system>
    </systemList>"#).unwrap()
}
#[test]
fn nested_originals_preserve_paths_identity_and_tree_parentage_in_both_modes() {
    let rom = nested();
    let generic = build_catalogue(
        "server",
        &platforms("ps", "Sony Games"),
        std::slice::from_ref(&rom),
        |_| {},
    )
    .unwrap();
    let retro = build_catalogue_for_layout(
        "server",
        &platforms("ps", "Sony Games"),
        &[rom],
        Some(&systems()),
        |_| {},
    )
    .unwrap();
    for (cat, platform) in [(generic, "Sony Games"), (retro, "psx")] {
        assert_eq!(cat.entries.len(), 2);
        assert_eq!(cat.entries[0].file_name, "Game/discs/Disc.bin");
        assert_ne!(cat.entries[0].key.file_id, cat.entries[1].key.file_id);
        let tree = RommTree::new(cat);
        let directory = tree.inode_for_path(&[platform, "game", "DISCS"]).unwrap();
        let (page, next, eof) = tree.read_dir(directory, 0, 1).unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(next, 1);
        assert!(!eof);
        assert_eq!(page[0].name, "Disc.bin");
        assert_eq!(page[0].kind, EntryKind::File);
        assert_eq!(tree.metadata(page[0].inode).unwrap().parent, directory);
        assert_eq!(
            tree.path_of(page[0].inode).unwrap(),
            format!("{platform}/Game/discs/Disc.bin")
        );
        let (page, _, eof) = tree.read_dir(directory, next, 10).unwrap();
        assert_eq!(page[0].name, "Disc.cue");
        assert!(eof);
    }
}
#[test]
fn switch_auxiliary_packages_are_flat_and_not_launchable() {
    let mut rom = contract::rom_multi(
        8,
        "Nintendo",
        &["Game.nsp", "Update.nsp", "Dlc.nsp", "Manual.pdf"],
        &[1, 2, 3, 4],
    );
    rom["platform_slug"] = json!("switch");
    for (file, category) in rom["files"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(["game", "update", "dlc", "manual"])
    {
        file["category"] = json!(category);
    }
    let rom: RomDto = serde_json::from_value(rom).unwrap();
    let mut warnings = Vec::new();
    let cat = build_catalogue_for_layout(
        "s",
        &platforms("switch", "Nintendo"),
        std::slice::from_ref(&rom),
        Some(&systems()),
        |w| warnings.push(w),
    )
    .unwrap();
    assert_eq!(cat.entries.len(), 3);
    assert_eq!(
        cat.entries
            .iter()
            .filter(|e| cat.is_launchable(&e.key))
            .count(),
        1
    );
    assert!(cat
        .entries
        .iter()
        .filter(|e| e.platform_dir == "switchupdates")
        .all(|e| !e.file_name.contains('/') && !cat.is_launchable(&e.key)));
    assert!(warnings.iter().any(|w| w.contains("manual")));
    let generic = build_catalogue("s", &platforms("switch", "Nintendo"), &[rom], |_| {}).unwrap();
    assert_eq!(generic.entries.len(), 4);
    assert_eq!(
        generic
            .entries
            .iter()
            .filter(|e| generic.is_launchable(&e.key))
            .count(),
        1
    );
    assert!(generic
        .entries
        .iter()
        .any(|e| e.file_name == "_library/manual/8/Manual.pdf"));
}
#[test]
fn unsafe_or_outside_source_paths_are_rejected_without_losing_healthy_files() {
    for path in [
        "/roms/Sony Games",
        "roms/Sony Games/../outside",
        "roms/Sony GamesOther",
        "C:/roms/Sony Games",
        r"roms\Sony Games",
    ] {
        let mut rom = nested();
        rom.files[0].file_path = path.into();
        let mut warnings = Vec::new();
        let cat = build_catalogue("s", &platforms("ps", "Sony Games"), &[rom], |w| {
            warnings.push(w)
        })
        .unwrap();
        assert_eq!(cat.entries.len(), 1, "{path}");
        assert_eq!(cat.skipped_unsupported, 1);
        assert!(!warnings.is_empty());
    }
}
#[test]
fn directory_case_and_sanitization_collisions_are_stable_across_input_order() {
    let mut a = nested();
    a.files.truncate(1);
    a.files[0].file_path = "roms/Sony Games/A?".into();
    let mut b = a.clone();
    b.id = 9;
    b.files[0].id = 9000;
    b.files[0].file_path = "roms/Sony Games/a*".into();
    let p = platforms("ps", "Sony Games");
    let first = build_catalogue("s", &p, &[a.clone(), b.clone()], |_| {}).unwrap();
    let second = build_catalogue("s", &p, &[b, a], |_| {}).unwrap();
    assert_eq!(
        first
            .entries
            .iter()
            .map(|e| (&e.key, &e.file_name))
            .collect::<Vec<_>>(),
        second
            .entries
            .iter()
            .map(|e| (&e.key, &e.file_name))
            .collect::<Vec<_>>()
    );
    assert_ne!(
        first.entries[0].file_name.to_lowercase(),
        first.entries[1].file_name.to_lowercase()
    );
    assert_eq!(first.inode_plan(), second.inode_plan());
}
#[test]
fn frontend_overrides_are_not_mistaken_for_supported_managed_paths() {
    let xml = r#"<systemList><system><name>switch</name><path>D:Games</path></system><system><name>psx</name><path>~/../roms/psx</path></system></systemList>"#;
    let s = RetroBatSystems::from_xml(xml).unwrap();
    assert!(s.folder("switch", "switch").is_none());
    assert!(RetroBatSystems::from_xml("<systemList>").is_err());
}
