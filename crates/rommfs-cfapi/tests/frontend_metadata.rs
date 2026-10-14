use rommfs_cfapi::{
    check_mount_root, claim_mount_root, publish_frontend_metadata, read_frontend_metadata,
};
use rommfs_core::catalogue::build_catalogue;
use rommfs_core::retrobat::{generate_gamelists, MetadataIndex};
use rommfs_core::romm::{PlatformDto, RomDto};
use rommfs_fixture::contract;
use std::collections::{BTreeMap, HashMap};
use std::fs;

fn sources() -> (Vec<PlatformDto>, Vec<RomDto>) {
    (
        serde_json::from_str(&contract::platforms(&[(1, "gb", "gb", "Game Boy")])).unwrap(),
        vec![serde_json::from_value(contract::rom(1, "gb", "Game.gb", 3, "hash")).unwrap()],
    )
}

#[test]
fn existing_gamelist_without_identity_index_can_be_published() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    fs::create_dir(&root).unwrap();
    claim_mount_root(&root, "s").unwrap();
    fs::create_dir(root.join("gb")).unwrap();
    check_mount_root(&root, "s").unwrap();
    let path = root.join("gb/gamelist.xml");
    let xml = "<gameList><game><path>./Game.gb</path><name>Old name</name></game></gameList>";
    fs::write(&path, xml).unwrap();

    let (index, existing) = read_frontend_metadata(&root, "s").unwrap();
    assert_eq!(existing.get("gb").unwrap(), xml);
    let (platforms, roms) = sources();
    let catalogue = build_catalogue("s", &platforms, &roms, |_| {}).unwrap();
    let publication = generate_gamelists(
        &catalogue,
        &roms,
        "s",
        7,
        &index,
        &existing,
        &HashMap::new(),
    )
    .unwrap();
    publish_frontend_metadata(&root, "s", &publication, &BTreeMap::new()).unwrap();
    assert_eq!(
        fs::read(&path).unwrap(),
        publication.gamelists()["gb/gamelist.xml"]
    );
    let index_path = root.join(".rommfs-metadata/index.json");
    assert!(index_path.is_file());
    read_frontend_metadata(&root, "s").unwrap();

    fs::write(index_path, b"invalid JSON").unwrap();
    assert!(read_frontend_metadata(&root, "s").is_err());
}

#[test]
fn writable_frontend_state_remains_owned_and_survives_repeat_publication() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    fs::create_dir(&root).unwrap();
    claim_mount_root(&root, "s").unwrap();
    let (platforms, roms) = sources();
    let catalogue = build_catalogue("s", &platforms, &roms, |_| {}).unwrap();
    let initial = generate_gamelists(
        &catalogue,
        &roms,
        "s",
        7,
        &MetadataIndex::default(),
        &BTreeMap::new(),
        &HashMap::new(),
    )
    .unwrap();
    publish_frontend_metadata(&root, "s", &initial, &BTreeMap::new()).unwrap();
    let path = root.join("gb/gamelist.xml");
    let before = fs::read_to_string(&path).unwrap().replace(
        "</game>",
        "<favorite>true</favorite><playcount>6</playcount></game>",
    );
    fs::write(&path, &before).unwrap();
    check_mount_root(&root, "s").unwrap();
    let (index, existing) = read_frontend_metadata(&root, "s").unwrap();
    let next = generate_gamelists(
        &catalogue,
        &roms,
        "s",
        7,
        &index,
        &existing,
        &HashMap::new(),
    )
    .unwrap();
    publish_frontend_metadata(&root, "s", &next, &BTreeMap::new()).unwrap();
    assert!(fs::read_to_string(&path)
        .unwrap()
        .contains("<playcount>6</playcount>"));
    fs::write(root.join("gb/user.sav"), b"save bytes").unwrap();
    assert!(publish_frontend_metadata(&root, "s", &next, &BTreeMap::new()).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    assert_eq!(fs::read(root.join("gb/user.sav")).unwrap(), b"save bytes");
}

#[cfg(unix)]
#[test]
fn publication_refuses_redirected_local_files_without_touching_target() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    fs::create_dir(&root).unwrap();
    claim_mount_root(&root, "s").unwrap();
    let (platforms, roms) = sources();
    let catalogue = build_catalogue("s", &platforms, &roms, |_| {}).unwrap();
    let output = generate_gamelists(
        &catalogue,
        &roms,
        "s",
        7,
        &MetadataIndex::default(),
        &BTreeMap::new(),
        &HashMap::new(),
    )
    .unwrap();
    publish_frontend_metadata(&root, "s", &output, &BTreeMap::new()).unwrap();
    let outside = tmp.path().join("outside.xml");
    fs::write(&outside, b"unrelated data").unwrap();
    fs::remove_file(root.join("gb/gamelist.xml")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("gb/gamelist.xml")).unwrap();
    assert!(publish_frontend_metadata(&root, "s", &output, &BTreeMap::new()).is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"unrelated data");
}
