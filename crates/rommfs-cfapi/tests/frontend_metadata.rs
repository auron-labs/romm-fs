use rommfs_cfapi::{
    check_mount_root, claim_mount_root, publish_frontend_metadata, read_frontend_metadata,
};
use rommfs_core::catalogue::build_catalogue;
use rommfs_core::retrobat::{
    generate_gamelists, FrontendGamelists, MetadataIndex, MetadataPublication,
};
use rommfs_core::romm::{PlatformDto, RomDto};
use rommfs_fixture::contract;
use std::collections::{BTreeMap, HashMap};
use std::fs;

fn publication(legacy: &BTreeMap<String, String>, index: &MetadataIndex) -> MetadataPublication {
    let platforms: Vec<PlatformDto> =
        serde_json::from_str(&contract::platforms(&[(1, "gb", "gb", "Game Boy")])).unwrap();
    let roms: Vec<RomDto> =
        vec![serde_json::from_value(contract::rom(1, "gb", "Game.gb", 3, "hash")).unwrap()];
    let catalogue = build_catalogue("s", &platforms, &roms, |_| {}).unwrap();
    generate_gamelists(
        &catalogue,
        &roms,
        "s",
        7,
        index,
        &FrontendGamelists::default(),
        legacy,
        &HashMap::new(),
    )
    .unwrap()
}

fn claimed_root() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    fs::create_dir(&root).unwrap();
    claim_mount_root(&root, "s").unwrap();
    (tmp, root)
}

#[test]
fn artwork_batch_is_owned_before_publication_and_reusable() {
    let (tmp, root) = claimed_root();
    let output = publication(&BTreeMap::new(), &MetadataIndex::default());
    let source = tmp.path().join("cached.png");
    fs::write(&source, b"cached artwork").unwrap();
    let artwork = BTreeMap::from([
        ("gb/.rommfs-artwork/cover.png".into(), source.clone()),
        ("gb/.rommfs-artwork/screenshot.png".into(), source.clone()),
    ]);
    publish_frontend_metadata(&root, "s", output.index(), &artwork, || Ok(())).unwrap();
    check_mount_root(&root, "s").unwrap();
    publish_frontend_metadata(&root, "s", output.index(), &artwork, || Ok(())).unwrap();
    for path in artwork.keys() {
        assert_eq!(fs::read(root.join(path)).unwrap(), b"cached artwork");
    }

    let foreign = "gb/.rommfs-artwork/user.png";
    fs::write(root.join(foreign), b"user artwork").unwrap();
    assert!(publish_frontend_metadata(
        &root,
        "s",
        output.index(),
        &BTreeMap::from([(foreign.into(), source)]),
        || Ok(()),
    )
    .is_err());
    assert_eq!(fs::read(root.join(foreign)).unwrap(), b"user artwork");
}

#[test]
fn gamelists_are_written_between_the_staged_and_final_index() {
    let (_tmp, root) = claimed_root();
    let output = publication(&BTreeMap::new(), &MetadataIndex::default());
    let index_path = root.join(".rommfs-metadata/index.json");
    publish_frontend_metadata(&root, "s", output.index(), &BTreeMap::new(), || {
        let staged = fs::read_to_string(&index_path).unwrap();
        assert!(staged.contains("\"pending_publication\":true"), "{staged}");
        Ok(())
    })
    .unwrap();
    let committed = fs::read_to_string(&index_path).unwrap();
    assert!(committed.contains("\"pending_publication\":false"));

    // A failed gamelist write leaves the staged index, so old XML is not
    // recaptured against the new identity paths on retry.
    assert!(
        publish_frontend_metadata(&root, "s", output.index(), &BTreeMap::new(), || {
            anyhow::bail!("disk full")
        })
        .is_err()
    );
    assert!(fs::read_to_string(&index_path)
        .unwrap()
        .contains("\"pending_publication\":true"));
}

#[test]
fn legacy_managed_gamelists_are_read_without_an_identity_index() {
    let (_tmp, root) = claimed_root();
    fs::create_dir(root.join("gb")).unwrap();
    check_mount_root(&root, "s").unwrap();
    let xml = "<gameList><game><path>./Game.gb</path><favorite>true</favorite></game></gameList>";
    fs::write(root.join("gb/gamelist.xml"), xml).unwrap();

    let (index, legacy) = read_frontend_metadata(&root, "s").unwrap();
    assert_eq!(legacy.get("gb").unwrap(), xml);
    let output = publication(&legacy, &index);
    let gamelist = String::from_utf8(output.gamelists()["gb/gamelist.xml"].clone()).unwrap();
    assert!(gamelist.contains("<favorite>true</favorite>"), "{gamelist}");
    publish_frontend_metadata(&root, "s", output.index(), &BTreeMap::new(), || Ok(())).unwrap();
    let index_path = root.join(".rommfs-metadata/index.json");
    assert!(index_path.is_file());
    read_frontend_metadata(&root, "s").unwrap();

    fs::write(index_path, b"invalid JSON").unwrap();
    assert!(read_frontend_metadata(&root, "s").is_err());
}

#[cfg(unix)]
#[test]
fn publication_refuses_redirected_local_files_without_touching_target() {
    let (tmp, root) = claimed_root();
    let output = publication(&BTreeMap::new(), &MetadataIndex::default());
    let source = tmp.path().join("cached.png");
    fs::write(&source, b"cached artwork").unwrap();
    let artwork = BTreeMap::from([("gb/.rommfs-artwork/cover.png".into(), source)]);
    publish_frontend_metadata(&root, "s", output.index(), &artwork, || Ok(())).unwrap();
    let outside = tmp.path().join("outside.png");
    fs::write(&outside, b"unrelated data").unwrap();
    let target = root.join("gb/.rommfs-artwork/cover.png");
    fs::remove_file(&target).unwrap();
    std::os::unix::fs::symlink(&outside, &target).unwrap();
    assert!(publish_frontend_metadata(&root, "s", output.index(), &artwork, || Ok(())).is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"unrelated data");
}

#[test]
fn failed_gamelist_publication_retries_swapped_paths_without_reassigning_history() {
    use rommfs_core::retrobat::publish_gamelists;
    let (tmp, root) = claimed_root();
    let installation = tmp.path().join("RetroBat");
    // gb cannot be written; nes can. Publication must still attempt both.
    fs::create_dir_all(installation.join("roms/nes")).unwrap();
    let systems = vec!["gb".to_string(), "nes".to_string()];
    let platforms: Vec<PlatformDto> = serde_json::from_str(&contract::platforms(&[
        (1, "gb", "gb", "Game Boy"),
        (2, "nes", "nes", "NES"),
    ]))
    .unwrap();
    let mut roms: Vec<RomDto> = vec![
        serde_json::from_value(contract::rom(1, "gb", "A.gb", 3, "a")).unwrap(),
        serde_json::from_value(contract::rom(2, "gb", "B.gb", 3, "b")).unwrap(),
        serde_json::from_value(contract::rom(3, "nes", "Game.nes", 3, "c")).unwrap(),
    ];
    let catalogue = build_catalogue("s", &platforms, &roms, |_| {}).unwrap();
    let initial = generate_gamelists(
        &catalogue,
        &roms,
        "s",
        7,
        &MetadataIndex::default(),
        &FrontendGamelists::default(),
        &BTreeMap::new(),
        &HashMap::new(),
    )
    .unwrap();
    let old = FrontendGamelists::new(BTreeMap::from([("gb".into(), "<gameList><game><path>./RomM/A.gb</path><playcount>11</playcount></game><game><path>./RomM/B.gb</path><playcount>22</playcount></game></gameList>".into())]));
    roms[0].files[0].file_name = "B.gb".into();
    roms[1].files[0].file_name = "A.gb".into();
    let catalogue = build_catalogue("s", &platforms, &roms, |_| {}).unwrap();
    let prepared = generate_gamelists(
        &catalogue,
        &roms,
        "s",
        7,
        initial.index(),
        &old,
        &BTreeMap::new(),
        &HashMap::new(),
    )
    .unwrap();
    let result = publish_frontend_metadata(&root, "s", prepared.index(), &BTreeMap::new(), || {
        let failures = publish_gamelists(&installation, &systems, &prepared);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].0, "gb");
        anyhow::ensure!(failures.is_empty(), "gamelist write failed");
        Ok(())
    });
    assert!(result.is_err());
    assert!(
        fs::read_to_string(installation.join("roms/nes/gamelist.xml"))
            .unwrap()
            .contains("./RomM/Game.nes")
    );
    let (staged, _) = read_frontend_metadata(&root, "s").unwrap();
    // A retry can encounter an unreadable gamelist before access is fixed.
    // Publishing other systems must not end recovery for that old XML.
    let unreadable = FrontendGamelists::new(BTreeMap::from([("gb".into(), "<broken/>".into())]));
    let skipped = generate_gamelists(
        &catalogue,
        &roms,
        "s",
        7,
        &staged,
        &unreadable,
        &BTreeMap::new(),
        &HashMap::new(),
    )
    .unwrap();
    publish_frontend_metadata(&root, "s", skipped.index(), &BTreeMap::new(), || {
        anyhow::ensure!(
            publish_gamelists(&installation, &systems, &skipped).is_empty(),
            "retry failed"
        );
        Ok(())
    })
    .unwrap();
    let (staged, _) = read_frontend_metadata(&root, "s").unwrap();
    let recovered = generate_gamelists(
        &catalogue,
        &roms,
        "s",
        7,
        &staged,
        &old,
        &BTreeMap::new(),
        &HashMap::new(),
    )
    .unwrap();
    assert_eq!(
        prepared.gamelists()["gb/gamelist.xml"],
        recovered.gamelists()["gb/gamelist.xml"]
    );
    let xml = std::str::from_utf8(&recovered.gamelists()["gb/gamelist.xml"]).unwrap();
    let a = xml
        .split("</game>")
        .find(|game| game.contains("./RomM/A.gb"))
        .unwrap();
    assert!(a.contains("<playcount>22</playcount>"), "{xml}");
    fs::create_dir(installation.join("roms/gb")).unwrap();
    publish_frontend_metadata(&root, "s", recovered.index(), &BTreeMap::new(), || {
        anyhow::ensure!(
            publish_gamelists(&installation, &systems, &recovered).is_empty(),
            "retry failed"
        );
        Ok(())
    })
    .unwrap();
    let (committed, _) = read_frontend_metadata(&root, "s").unwrap();
    let frontend = rommfs_core::retrobat::read_gamelists(&installation, &systems);
    let unchanged = generate_gamelists(
        &catalogue,
        &roms,
        "s",
        7,
        &committed,
        &frontend,
        &BTreeMap::new(),
        &HashMap::new(),
    )
    .unwrap();
    assert!(unchanged.gamelists().is_empty());
}
