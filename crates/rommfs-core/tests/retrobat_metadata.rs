use rommfs_core::catalogue::build_catalogue;
use rommfs_core::retrobat::{generate_gamelists, MetadataIndex};
use rommfs_core::romm::{PlatformDto, RomDto};
use rommfs_fixture::contract;
use serde_json::json;
use std::collections::{BTreeMap, HashMap};

fn sources(name: &str) -> (Vec<PlatformDto>, Vec<RomDto>) {
    let platforms =
        serde_json::from_str(&contract::platforms(&[(1, "gb", "gb", "Game Boy")])).unwrap();
    let mut rom = contract::rom(1, "gb", name, 3, "hash");
    rom["name"] = json!("A & B <game>");
    rom["summary"] = json!("Description & details");
    rom["metadatum"] = json!({"genres":["Adventure"],"developers":["Dev"],"publishers":["Pub"],"player_count":"1-2","first_release_date":946684800,"average_rating":85.0});
    (platforms, vec![serde_json::from_value(rom).unwrap()])
}

#[test]
fn generated_paths_and_metadata_are_xml_safe_and_auxiliary_files_are_not_games() {
    let (platforms, mut roms) = sources("A&B.gb");
    let mut auxiliary = roms[0].clone();
    auxiliary.id = 2;
    auxiliary.files[0].id += 1;
    auxiliary.files[0].category = Some("update".into());
    roms.push(auxiliary);
    let cat = build_catalogue("s", &platforms, &roms, |_| {}).unwrap();
    let art = HashMap::from([((1, "image"), ".rommfs-artwork/cover.png".into())]);
    let output = generate_gamelists(
        &cat,
        &roms,
        "s",
        7,
        &MetadataIndex::default(),
        &BTreeMap::new(),
        &art,
    )
    .unwrap();
    let xml = std::str::from_utf8(&output.gamelists()["gb/gamelist.xml"]).unwrap();
    assert!(xml.contains("<path>./A&amp;B.gb</path>"), "{xml}");
    assert!(xml.contains("A &amp; B &lt;game&gt;"));
    assert!(xml.contains("<releasedate>20000101T000000</releasedate>"));
    assert!(xml.contains("<rating>0.85</rating>"));
    assert!(xml.contains("<image>./.rommfs-artwork/cover.png</image>"));
    assert_eq!(xml.matches("<game>").count(), 1);
}

#[test]
fn frontend_state_survives_renames_removal_readdition_and_clean_index_reload() {
    let (platforms, roms) = sources("Original.gb");
    let cat = build_catalogue("s", &platforms, &roms, |_| {}).unwrap();
    let initial = generate_gamelists(
        &cat,
        &roms,
        "s",
        7,
        &MetadataIndex::default(),
        &BTreeMap::new(),
        &HashMap::new(),
    )
    .unwrap();
    let index: MetadataIndex =
        serde_json::from_slice(&serde_json::to_vec(initial.index()).unwrap()).unwrap();
    let existing = BTreeMap::from([("gb".into(), "<gameList><folder><path>./Folder</path><favorite>true</favorite></folder><game><path>./Original.gb</path><name>Old</name><favorite>true</favorite><playcount>12</playcount><lastplayed>20261007T120000</lastplayed><custom attr=\"keep\">A &amp; B</custom></game></gameList>".into())]);
    let mut renamed = roms.clone();
    renamed[0].files[0].file_name = "Renamed.gb".into();
    let new_cat = build_catalogue("s", &platforms, &renamed, |_| {}).unwrap();
    let output = generate_gamelists(
        &new_cat,
        &renamed,
        "s",
        7,
        &index,
        &existing,
        &HashMap::new(),
    )
    .unwrap();
    let xml = std::str::from_utf8(&output.gamelists()["gb/gamelist.xml"]).unwrap();
    for fragment in [
        "<path>./Renamed.gb</path>",
        "<favorite>true</favorite>",
        "<playcount>12</playcount>",
        "<lastplayed>20261007T120000</lastplayed>",
        "<custom attr=\"keep\">A &amp; B</custom>",
        "<folder>",
    ] {
        assert!(xml.contains(fragment), "missing {fragment}: {xml}");
    }
    let empty_cat = build_catalogue("s", &platforms, &[], |_| {}).unwrap();
    let empty = generate_gamelists(
        &empty_cat,
        &[],
        "s",
        7,
        output.index(),
        &BTreeMap::from([("gb".into(), xml.into())]),
        &HashMap::new(),
    )
    .unwrap();
    let restored = generate_gamelists(
        &new_cat,
        &renamed,
        "s",
        7,
        empty.index(),
        &BTreeMap::new(),
        &HashMap::new(),
    )
    .unwrap();
    assert!(
        std::str::from_utf8(&restored.gamelists()["gb/gamelist.xml"])
            .unwrap()
            .contains("<playcount>12</playcount>")
    );
    assert!(generate_gamelists(&cat, &roms, "s", 8, &index, &existing, &HashMap::new()).is_err());
}

#[test]
fn malformed_or_unsafe_existing_gamelists_do_not_become_an_empty_success() {
    let (platforms, roms) = sources("Game.gb");
    let cat = build_catalogue("s", &platforms, &roms, |_| {}).unwrap();
    for xml in [
        "<gameList>",
        "<other/>",
        "<!DOCTYPE gameList><gameList/>",
        "<gameList><game><path>../save</path></game></gameList>",
        "<gameList><game><path>./Game.gb</path><path>./Other.gb</path></game></gameList>",
        "<gameList><game><path>./Game.gb</path><custom><nested></wrong></custom></game></gameList>",
    ] {
        let existing = BTreeMap::from([("gb".into(), xml.into())]);
        assert!(
            generate_gamelists(
                &cat,
                &roms,
                "s",
                7,
                &MetadataIndex::default(),
                &existing,
                &HashMap::new()
            )
            .is_err(),
            "{xml}"
        );
    }
}

#[test]
fn interruption_does_not_reassign_history_when_projected_paths_swap() {
    let (platforms, mut roms) = sources("A.gb");
    let mut second = roms[0].clone();
    second.id = 2;
    second.files[0].id = 2;
    second.files[0].file_name = "B.gb".into();
    roms.push(second);
    let cat = build_catalogue("s", &platforms, &roms, |_| {}).unwrap();
    let initial = generate_gamelists(
        &cat,
        &roms,
        "s",
        7,
        &MetadataIndex::default(),
        &BTreeMap::new(),
        &HashMap::new(),
    )
    .unwrap();
    let existing = BTreeMap::from([("gb".into(),"<gameList><game><path>./A.gb</path><playcount>11</playcount></game><game><path>./B.gb</path><playcount>22</playcount></game></gameList>".into())]);
    let mut swapped = roms.clone();
    swapped[0].files[0].file_name = "B.gb".into();
    swapped[1].files[0].file_name = "A.gb".into();
    let next_cat = build_catalogue("s", &platforms, &swapped, |_| {}).unwrap();
    let prepared = generate_gamelists(
        &next_cat,
        &swapped,
        "s",
        7,
        initial.index(),
        &existing,
        &HashMap::new(),
    )
    .unwrap();
    // The new index is durable but XML is still old when the process dies.
    let recovered = generate_gamelists(
        &next_cat,
        &swapped,
        "s",
        7,
        &prepared.index().staged(),
        &existing,
        &HashMap::new(),
    )
    .unwrap();
    assert_eq!(prepared.gamelists(), recovered.gamelists());
    let xml = std::str::from_utf8(&recovered.gamelists()["gb/gamelist.xml"]).unwrap();
    let first = xml.split("</game>").next().unwrap();
    assert!(
        first.contains("./A.gb") && first.contains("<playcount>22</playcount>"),
        "{xml}"
    );
}

#[test]
fn removed_platform_metadata_is_emptied_but_archived_history_is_not_overwritten_by_new_game() {
    let (platforms, roms) = sources("Game.gb");
    let cat = build_catalogue("s", &platforms, &roms, |_| {}).unwrap();
    let initial = generate_gamelists(
        &cat,
        &roms,
        "s",
        7,
        &MetadataIndex::default(),
        &BTreeMap::new(),
        &HashMap::new(),
    )
    .unwrap();
    let existing = BTreeMap::from([(
        "gb".into(),
        "<gameList><game><path>./Game.gb</path><playcount>11</playcount></game></gameList>".into(),
    )]);
    let empty_cat = build_catalogue("s", &[], &[], |_| {}).unwrap();
    let empty = generate_gamelists(
        &empty_cat,
        &[],
        "s",
        7,
        initial.index(),
        &existing,
        &HashMap::new(),
    )
    .unwrap();
    assert_eq!(
        std::str::from_utf8(&empty.gamelists()["gb/gamelist.xml"]).unwrap(),
        "<gameList></gameList>"
    );
    let mut replacement = roms.clone();
    replacement[0].id = 2;
    replacement[0].files[0].id = 2;
    let replacement_cat = build_catalogue("s", &platforms, &replacement, |_| {}).unwrap();
    let replacement = generate_gamelists(
        &replacement_cat,
        &replacement,
        "s",
        7,
        empty.index(),
        &BTreeMap::new(),
        &HashMap::new(),
    )
    .unwrap();
    let replacement_xml = BTreeMap::from([(
        "gb".into(),
        "<gameList><game><path>./Game.gb</path><playcount>99</playcount></game></gameList>".into(),
    )]);
    let restored = generate_gamelists(
        &cat,
        &roms,
        "s",
        7,
        replacement.index(),
        &replacement_xml,
        &HashMap::new(),
    )
    .unwrap();
    let xml = std::str::from_utf8(&restored.gamelists()["gb/gamelist.xml"]).unwrap();
    assert!(xml.contains("<playcount>11</playcount>"), "{xml}");
    assert!(!xml.contains("<playcount>99</playcount>"));
}
