use rommfs_core::catalog::build_catalogue;
use rommfs_core::romm::{PlatformDto, RomDto};
use rommfs_core::save_sync::{file_slot, SaveFileRules};
use std::{collections::BTreeMap, fs, path::Path};

#[test]
fn scan_associates_multiple_files_by_unique_stem_or_server_target_and_reports_ambiguity() {
    let platform:PlatformDto=serde_json::from_value(serde_json::json!({"id":1,"slug":"switch","fs_slug":"switch","name":"Switch","custom_name":null,"rom_count":2})).unwrap();
    let rom = |id, name, target| {
        serde_json::from_value::<RomDto>(serde_json::json!({"id":id,"platform_fs_slug":"switch","platform_slug":"switch","fs_name":name,"fs_size_bytes":3,"has_simple_single_file":true,"has_nested_single_file":false,"has_multiple_files":false,"missing_from_fs":false,"is_physical":false,"updated_at":"now","save_target":target,"save_target_layout":"folder-exact","files":[{"id":id*10,"file_name":name,"file_size_bytes":3,"is_top_level":true}]})).unwrap()
    };
    let roms = [
        rom(1, "Game.nsp", "0100ABCD/user"),
        rom(2, "Other.nsp", "0100DCBA/user"),
    ];
    let catalogue =
        build_catalogue("server", std::slice::from_ref(&platform), &roms, |_| {}).unwrap();
    let dir = tempfile::tempdir().unwrap();
    for name in [
        "switch/Game.sav",
        "switch/Game.rtc",
        "switch/0100ABCD/user/0",
        "switch/Other/empty",
        "switch/unknown.dat",
        "switch/memcards/card",
        "switch/.Game.sav.rommfs-backup-uuid",
    ] {
        let path = dir.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, []).unwrap();
    }
    let rules = SaveFileRules::from_catalogue(&catalogue, &roms, BTreeMap::new()).unwrap();
    let plan = rules.scan(dir.path()).unwrap();
    assert_eq!(plan.report.mappings.len(), 4);
    assert_eq!(plan.unmatched.len(), 3);
    let explicit = BTreeMap::from([("switch/unknown.dat".into(), 2)]);
    let rules = SaveFileRules::from_catalogue(&catalogue, &roms, explicit).unwrap();
    assert_eq!(rules.scan(dir.path()).unwrap().report.mappings.len(), 5);
    assert!(rules
        .associate(Path::new("switch/unknown.dat"), dir.path())
        .unwrap()
        .is_some());
    assert!(file_slot(Path::new("../outside")).is_err());
    assert!(rules
        .mapping(1, Path::new("switch/shared/card"), dir.path())
        .is_err());
    assert!(rules
        .mapping(
            1,
            Path::new("switch/Game/.Game.sav.rommfs-backup-uuid"),
            dir.path()
        )
        .is_err());
    assert!(rules
        .mapping(1, Path::new("switch/Game/.hidden-save"), dir.path())
        .is_ok());
    let too_long = format!("switch/Game/{}/data", "a".repeat(240));
    let path = dir.path().join(&too_long);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, []).unwrap();
    assert!(rules
        .scan(dir.path())
        .unwrap()
        .unmatched
        .iter()
        .any(|s| s.display().starts_with(&too_long)));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/", dir.path().join("switch/redirect")).unwrap();
        assert!(rules
            .scan(dir.path())
            .unwrap()
            .unmatched
            .iter()
            .any(|s| s.display().contains("redirect")));
    }
}

fn gb_catalogue() -> (rommfs_core::catalog::Catalogue, Vec<RomDto>) {
    let platform = serde_json::from_value::<PlatformDto>(serde_json::json!({
        "id": 1, "slug": "gb", "fs_slug": "gb", "name": "Game Boy", "rom_count": 2
    }))
    .unwrap();
    let roms = [(1, "Game.gb"), (2, "Other.gb")].into_iter().map(|(id, name)| {
        serde_json::from_value::<RomDto>(serde_json::json!({
            "id": id, "platform_fs_slug": "gb", "platform_slug": "gb", "fs_name": name,
            "fs_size_bytes": 3, "has_simple_single_file": true,
            "has_nested_single_file": false, "has_multiple_files": false,
            "missing_from_fs": false, "is_physical": false, "updated_at": "now",
            "files": [{"id": id * 10, "file_name": name, "file_size_bytes": 3, "is_top_level": true}]
        })).unwrap()
    }).collect::<Vec<_>>();
    (
        build_catalogue("server", &[platform], &roms, |_| {}).unwrap(),
        roms,
    )
}

fn remote(rom_id: i64, slot: String) -> rommfs_core::romm::RemoteSave {
    serde_json::from_value(serde_json::json!({
        "id": rom_id, "rom_id": rom_id, "user_id": 42, "file_name": "revision.bin",
        "file_size_bytes": 3, "slot": slot
    }))
    .unwrap()
}

#[test]
fn legacy_slot_is_only_accepted_by_the_original_gb_sram() {
    use rommfs_core::save_sync::RETROBAT_GB_SRM_PROFILE;
    let (catalogue, roms) = gb_catalogue();
    let root = tempfile::tempdir().unwrap();
    let rules = SaveFileRules::from_catalogue(&catalogue, &roms, BTreeMap::new()).unwrap();
    let original = rules
        .mapping(1, Path::new("gb/Game.srm"), root.path())
        .unwrap();
    let other = rules
        .mapping(1, Path::new("gb/Extra.srm"), root.path())
        .unwrap();
    let companion = rules
        .mapping(1, Path::new("gb/Game.rtc"), root.path())
        .unwrap();
    assert!(original.accepts_remote_slot(Some(RETROBAT_GB_SRM_PROFILE)));
    for mapping in [other, companion] {
        assert_eq!(mapping.legacy_slot(), None);
        assert!(!mapping.accepts_remote_slot(Some(RETROBAT_GB_SRM_PROFILE)));
        assert!(mapping.accepts_remote_slot(Some(&mapping.remote_slot().unwrap())));
    }
    let recovered = rules
        .remote_mappings(
            &[remote(1, RETROBAT_GB_SRM_PROFILE.into())],
            42,
            root.path(),
        )
        .unwrap();
    assert_eq!(recovered, vec![original]);
}

#[test]
fn correcting_an_owner_excludes_its_old_journal_and_remote_candidates() {
    use rommfs_core::save_sync::RETROBAT_GB_SRM_PROFILE;
    let (catalogue, roms) = gb_catalogue();
    let root = tempfile::tempdir().unwrap();
    let path = Path::new("gb/Game.srm");
    let rules = SaveFileRules::from_catalogue(
        &catalogue,
        &roms,
        BTreeMap::from([("gb/Game.srm".into(), 2)]),
    )
    .unwrap();
    assert!(rules
        .tracked_mapping(1, path, root.path())
        .unwrap()
        .is_none());
    assert_eq!(
        rules
            .associate(path, root.path())
            .unwrap()
            .unwrap()
            .rom_key
            .rom_id,
        2
    );
    let mappings = rules
        .remote_mappings(
            &[
                remote(1, file_slot(path).unwrap()),
                remote(1, RETROBAT_GB_SRM_PROFILE.into()),
                remote(2, file_slot(path).unwrap()),
            ],
            42,
            root.path(),
        )
        .unwrap();
    assert_eq!(mappings.len(), 1);
    assert_eq!(mappings[0].rom_key.rom_id, 2);
    assert_eq!(mappings[0].legacy_slot(), None);
}

#[test]
fn unavailable_explicit_owner_is_preserved_and_diagnosed_without_blocking_other_files() {
    let (catalogue, roms) = gb_catalogue();
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("gb")).unwrap();
    fs::write(root.path().join("gb/Game.srm"), b"good").unwrap();
    fs::write(root.path().join("gb/Other.srm"), b"preserved").unwrap();
    let explicit = BTreeMap::from([("gb/Other.srm".into(), 99), ("gb/absent.srm".into(), 99)]);
    let rules = SaveFileRules::from_catalogue(&catalogue, &roms, explicit).unwrap();
    let plan = rules.scan(root.path()).unwrap();
    assert_eq!(plan.report.mappings.len(), 1);
    assert_eq!(plan.report.mappings[0].rom_key.rom_id, 1);
    assert_eq!(plan.unmatched.len(), 2);
    assert!(plan
        .unmatched
        .iter()
        .all(|issue| issue.reason.contains("game 99 is unavailable")));
    assert!(rules
        .associate(Path::new("gb/Other.srm"), root.path())
        .is_err());
    assert!(rules
        .tracked_mapping(2, Path::new("gb/Other.srm"), root.path())
        .unwrap()
        .is_none());
    assert_eq!(
        fs::read(root.path().join("gb/Other.srm")).unwrap(),
        b"preserved"
    );
    let corrected = SaveFileRules::from_catalogue(
        &catalogue,
        &roms,
        BTreeMap::from([("gb/Other.srm".into(), 2)]),
    )
    .unwrap();
    assert_eq!(
        corrected.scan(root.path()).unwrap().report.mappings.len(),
        2
    );
}

#[test]
fn legacy_recovery_requires_one_launchable_gb_filename() {
    use rommfs_core::save_sync::RETROBAT_GB_SRM_PROFILE;
    let (_, mut roms) = gb_catalogue();
    let platform = serde_json::from_value::<PlatformDto>(serde_json::json!({
        "id": 1, "slug": "gb", "fs_slug": "gb", "name": "Game Boy", "rom_count": 2
    }))
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    for name in ["Game.gbc", "Game.gb"] {
        roms[0].fs_name = name.into();
        roms[0].files[0].file_name = name.into();
        if name == "Game.gb" {
            let mut extra = roms[0].files[0].clone();
            extra.id = 99;
            extra.file_name = "Extra.gb".into();
            roms[0].files.push(extra);
            roms[0].has_multiple_files = true;
            roms[0].has_simple_single_file = false;
        }
        let catalogue =
            build_catalogue("server", std::slice::from_ref(&platform), &roms, |_| {}).unwrap();
        let rules = SaveFileRules::from_catalogue(&catalogue, &roms, BTreeMap::new()).unwrap();
        assert_eq!(
            rules
                .mapping(1, Path::new("gb/Game.srm"), root.path())
                .unwrap()
                .legacy_slot(),
            None
        );
        assert!(rules
            .remote_mappings(
                &[remote(1, RETROBAT_GB_SRM_PROFILE.into())],
                42,
                root.path()
            )
            .unwrap()
            .is_empty());
    }
}

#[test]
fn deleted_missing_and_unlaunchable_associations_leave_available_games_syncable() {
    use rommfs_core::catalog::build_catalogue_for_layout;
    use rommfs_core::layout::RetroBatSystems;
    let (_, original) = gb_catalogue();
    let platform = serde_json::from_value::<PlatformDto>(serde_json::json!({
        "id": 1, "slug": "gb", "fs_slug": "gb", "name": "Game Boy", "rom_count": 2
    }))
    .unwrap();
    let systems = RetroBatSystems::from_xml(r#"<systemList><system><name>gb</name><path>~/../roms/gb</path><platform>gb</platform><extension>.gb</extension></system></systemList>"#).unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("gb")).unwrap();
    fs::write(root.path().join("gb/Game.srm"), b"good").unwrap();
    fs::write(root.path().join("gb/Other.srm"), b"preserved").unwrap();
    for scenario in ["deleted", "missing", "unlaunchable"] {
        let mut roms = original.clone();
        match scenario {
            "deleted" => roms.retain(|rom| rom.id != 2),
            "missing" => roms[1].missing_from_fs = true,
            _ => {
                roms[1].fs_name = "Other.txt".into();
                roms[1].files[0].file_name = "Other.txt".into();
            }
        }
        let catalogue = build_catalogue_for_layout(
            "server",
            std::slice::from_ref(&platform),
            &roms,
            Some(&systems),
            |_| {},
        )
        .unwrap();
        let rules = SaveFileRules::from_catalogue(
            &catalogue,
            &roms,
            BTreeMap::from([("gb/Other.srm".into(), 2)]),
        )
        .unwrap();
        let plan = rules.scan(root.path()).unwrap();
        assert_eq!(plan.report.mappings.len(), 1, "{scenario}");
        assert_eq!(plan.report.mappings[0].rom_key.rom_id, 1);
        assert!(
            plan.unmatched
                .iter()
                .any(|issue| issue.reason.contains("game 2 is unavailable")),
            "{scenario}"
        );
    }
}
