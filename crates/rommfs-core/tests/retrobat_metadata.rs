use rommfs_core::catalogue::{build_catalogue, Catalogue};
use rommfs_core::retrobat::{
    generate_gamelists, publish_gamelists, read_gamelists, FrontendGamelists, MetadataIndex,
    MetadataPublication, PlayPush, PlaySync,
};
use rommfs_core::romm::{PlatformDto, RomDto};
use rommfs_fixture::contract;
use serde_json::json;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;

fn sources(name: &str) -> (Vec<PlatformDto>, Vec<RomDto>) {
    let platforms =
        serde_json::from_str(&contract::platforms(&[(1, "gb", "gb", "Game Boy")])).unwrap();
    let mut rom = contract::rom(1, "gb", name, 3, "hash");
    rom["name"] = json!("A & B <game>");
    rom["summary"] = json!("Description & details");
    rom["metadatum"] = json!({"genres":["Adventure"],"developers":["Dev"],"publishers":["Pub"],"player_count":"1-2","first_release_date":946684800,"average_rating":85.0});
    (platforms, vec![serde_json::from_value(rom).unwrap()])
}

/// Reconciliation context with play-data sync off, as most tests run.
macro_rules! play_off {
    () => {
        &mut PlaySync {
            enabled: false,
            remote: &HashMap::new(),
            push: &mut |_: &PlayPush| Ok(()),
            now: 0,
        }
    };
}

fn frontend(system: &str, xml: &str) -> FrontendGamelists {
    FrontendGamelists::new(BTreeMap::from([(system.into(), xml.into())]))
}

fn generate(
    cat: &Catalogue,
    roms: &[RomDto],
    index: &MetadataIndex,
    frontend: &FrontendGamelists,
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
        &HashSet::new(),
        play_off!(),
    )
    .unwrap()
}

fn gamelist(publication: &MetadataPublication, system: &str) -> String {
    String::from_utf8(publication.gamelists()[&format!("{system}/gamelist.xml")].clone()).unwrap()
}

/// An index round-tripped through JSON, as it is persisted.
fn reload(index: &MetadataIndex) -> MetadataIndex {
    serde_json::from_slice(&serde_json::to_vec(index).unwrap()).unwrap()
}

#[test]
fn generated_entries_live_under_the_romm_folder_and_are_xml_safe() {
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
        &FrontendGamelists::default(),
        &BTreeMap::new(),
        &art,
        &HashSet::new(),
        play_off!(),
    )
    .unwrap();
    let xml = gamelist(&output, "gb");
    assert!(xml.contains("<path>./RomM/A&amp;B.gb</path>"), "{xml}");
    assert!(xml.contains("A &amp; B &lt;game&gt;"));
    assert!(xml.contains("<releasedate>20000101T000000</releasedate>"));
    assert!(xml.contains("<rating>0.85</rating>"));
    assert!(xml.contains("<image>./RomM/.rommfs-artwork/cover.png</image>"));
    assert_eq!(xml.matches("<game>").count(), 1);
}

#[test]
fn stock_gamelist_entries_are_kept_and_republication_is_a_no_op() {
    let (platforms, roms) = sources("Game.gb");
    let cat = build_catalogue("s", &platforms, &roms, |_| {}).unwrap();
    let stock = "<?xml version=\"1.0\"?>\n<gameList><folder><path>./Homebrew</path><name>Homebrew</name></folder><game><path>./Local.gb</path><name>My game</name><favorite>true</favorite><custom attr=\"keep\">A &amp; B</custom></game></gameList>";
    let output = generate(
        &cat,
        &roms,
        &MetadataIndex::default(),
        &frontend("gb", stock),
    );
    let xml = gamelist(&output, "gb");
    for fragment in [
        "<folder><path>./Homebrew</path><name>Homebrew</name></folder>",
        "<game><path>./Local.gb</path><name>My game</name><favorite>true</favorite><custom attr=\"keep\">A &amp; B</custom></game>",
        "<path>./RomM/Game.gb</path>",
    ] {
        assert!(xml.contains(fragment), "missing {fragment}: {xml}");
    }
    assert!(output.skipped().is_empty());
    // Unchanged content is not rewritten.
    let again = generate(&cat, &roms, &reload(output.index()), &frontend("gb", &xml));
    assert!(again.gamelists().is_empty());
    // A system without RomM games and without our entries is left alone.
    let empty = build_catalogue("s", &platforms, &[], |_| {}).unwrap();
    assert!(generate(
        &empty,
        &[],
        &MetadataIndex::default(),
        &frontend("gb", stock)
    )
    .gamelists()
    .is_empty());
}

#[test]
fn frontend_state_is_carried_by_path_when_the_identity_index_is_missing() {
    let (platforms, roms) = sources("Game.gb");
    let cat = build_catalogue("s", &platforms, &roms, |_| {}).unwrap();
    let existing = "<gameList><game><path>./RomM/Game.gb</path><name>Old</name><favorite>true</favorite><playcount>12</playcount><lastplayed>20261007T120000</lastplayed></game></gameList>";
    let output = generate(
        &cat,
        &roms,
        &MetadataIndex::default(),
        &frontend("gb", existing),
    );
    let xml = gamelist(&output, "gb");
    for fragment in [
        "<favorite>true</favorite>",
        "<playcount>12</playcount>",
        "<lastplayed>20261007T120000</lastplayed>",
    ] {
        assert!(xml.contains(fragment), "missing {fragment}: {xml}");
    }
    assert!(!xml.contains("<name>Old</name>"));
}

#[test]
fn existing_artwork_is_kept_until_fresh_artwork_is_available() {
    let (platforms, roms) = sources("Game.gb");
    let cat = build_catalogue("s", &platforms, &roms, |_| {}).unwrap();
    let existing = "<gameList><game><path>./RomM/Game.gb</path><image>./RomM/.rommfs-artwork/old.png</image><thumbnail>./elsewhere.png</thumbnail></game></gameList>";
    let output = generate(
        &cat,
        &roms,
        &MetadataIndex::default(),
        &frontend("gb", existing),
    );
    let xml = gamelist(&output, "gb");
    assert!(
        xml.contains("<image>./RomM/.rommfs-artwork/old.png</image>"),
        "{xml}"
    );
    // Artwork RomMFS doesn't own is a generated field and is replaced.
    assert!(!xml.contains("elsewhere.png"));
    let art = HashMap::from([((1, "image"), ".rommfs-artwork/new.png".into())]);
    let fresh = generate_gamelists(
        &cat,
        &roms,
        "s",
        7,
        output.index(),
        &frontend("gb", &xml),
        &BTreeMap::new(),
        &art,
        &HashSet::new(),
        play_off!(),
    )
    .unwrap();
    let xml = gamelist(&fresh, "gb");
    assert!(xml.contains("./RomM/.rommfs-artwork/new.png") && !xml.contains("old.png"));
}

#[test]
fn an_unreadable_gamelist_skips_only_its_own_system() {
    let platforms: Vec<PlatformDto> = serde_json::from_str(&contract::platforms(&[
        (1, "gb", "gb", "Game Boy"),
        (2, "nes", "nes", "NES"),
    ]))
    .unwrap();
    let roms: Vec<RomDto> = vec![
        serde_json::from_value(contract::rom(1, "gb", "Game.gb", 3, "a")).unwrap(),
        serde_json::from_value(contract::rom(2, "nes", "Game.nes", 3, "b")).unwrap(),
    ];
    let cat = build_catalogue("s", &platforms, &roms, |_| {}).unwrap();
    for broken in [
        "<gameList>",
        "<other/>",
        "<!DOCTYPE gameList><gameList/>",
        "<gameList><game><path>./x</path><custom><nested></wrong></custom></game></gameList>",
    ] {
        let output = generate(
            &cat,
            &roms,
            &MetadataIndex::default(),
            &frontend("gb", broken),
        );
        assert_eq!(output.skipped().len(), 1, "{broken}");
        assert_eq!(output.skipped()[0].0, "gb");
        assert!(!output.gamelists().contains_key("gb/gamelist.xml"));
        assert!(gamelist(&output, "nes").contains("./RomM/Game.nes"));
    }
    // Unusual but readable entries (duplicates, odd paths) are kept, not errors.
    let odd = "<gameList><game><path>../save</path></game><game><path>./RomM/Game.gb</path></game><game><path>./RomM/Game.gb</path><playcount>3</playcount></game></gameList>";
    let output = generate(&cat, &roms, &MetadataIndex::default(), &frontend("gb", odd));
    assert!(output.skipped().is_empty());
    let xml = gamelist(&output, "gb");
    assert!(xml.contains("<path>../save</path>"));
    assert_eq!(xml.matches("./RomM/Game.gb").count(), 1);
}

#[test]
fn frontend_state_survives_renames_removal_readdition_and_clean_index_reload() {
    let (platforms, roms) = sources("Original.gb");
    let cat = build_catalogue("s", &platforms, &roms, |_| {}).unwrap();
    let initial = generate(
        &cat,
        &roms,
        &MetadataIndex::default(),
        &FrontendGamelists::default(),
    );
    let index = reload(initial.index());
    let existing = frontend("gb", "<gameList><game><path>./RomM/Original.gb</path><name>Old</name><favorite>true</favorite><playcount>12</playcount><custom attr=\"keep\">A &amp; B</custom></game></gameList>");
    let mut renamed = roms.clone();
    renamed[0].files[0].file_name = "Renamed.gb".into();
    let new_cat = build_catalogue("s", &platforms, &renamed, |_| {}).unwrap();
    let output = generate(&new_cat, &renamed, &index, &existing);
    let xml = gamelist(&output, "gb");
    for fragment in [
        "<path>./RomM/Renamed.gb</path>",
        "<favorite>true</favorite>",
        "<playcount>12</playcount>",
        "<custom attr=\"keep\">A &amp; B</custom>",
    ] {
        assert!(xml.contains(fragment), "missing {fragment}: {xml}");
    }
    let empty_cat = build_catalogue("s", &platforms, &[], |_| {}).unwrap();
    let empty = generate(&empty_cat, &[], output.index(), &frontend("gb", &xml));
    assert!(!gamelist(&empty, "gb").contains("<game>"));
    let restored = generate(
        &new_cat,
        &renamed,
        &reload(empty.index()),
        &FrontendGamelists::default(),
    );
    assert!(gamelist(&restored, "gb").contains("<playcount>12</playcount>"));
}

#[test]
fn an_index_for_another_account_starts_fresh_instead_of_refusing() {
    let (platforms, roms) = sources("Game.gb");
    let cat = build_catalogue("s", &platforms, &roms, |_| {}).unwrap();
    let other = generate_gamelists(
        &cat,
        &roms,
        "s",
        8,
        &MetadataIndex::default(),
        &FrontendGamelists::default(),
        &BTreeMap::new(),
        &HashMap::new(),
        &HashSet::new(),
        play_off!(),
    )
    .unwrap();
    let output = generate(&cat, &roms, other.index(), &FrontendGamelists::default());
    assert!(gamelist(&output, "gb").contains("./RomM/Game.gb"));
}

#[test]
fn legacy_managed_gamelists_contribute_frontend_state() {
    let (platforms, roms) = sources("Game.gb");
    let cat = build_catalogue("s", &platforms, &roms, |_| {}).unwrap();
    let legacy = BTreeMap::from([(
        "gb".into(),
        "<gameList><game><path>./Game.gb</path><favorite>true</favorite><image>./.rommfs-artwork/a.png</image></game></gameList>".into(),
    )]);
    let output = generate_gamelists(
        &cat,
        &roms,
        "s",
        7,
        &MetadataIndex::default(),
        &FrontendGamelists::default(),
        &legacy,
        &HashMap::new(),
        &HashSet::new(),
        play_off!(),
    )
    .unwrap();
    let xml = gamelist(&output, "gb");
    assert!(xml.contains("<favorite>true</favorite>"), "{xml}");
    // Legacy artwork paths don't resolve from RetroBat's folder.
    assert!(!xml.contains("a.png"));
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
    let initial = generate(
        &cat,
        &roms,
        &MetadataIndex::default(),
        &FrontendGamelists::default(),
    );
    let existing = frontend("gb", "<gameList><game><path>./RomM/A.gb</path><playcount>11</playcount></game><game><path>./RomM/B.gb</path><playcount>22</playcount></game></gameList>");
    let mut swapped = roms.clone();
    swapped[0].files[0].file_name = "B.gb".into();
    swapped[1].files[0].file_name = "A.gb".into();
    let next_cat = build_catalogue("s", &platforms, &swapped, |_| {}).unwrap();
    let prepared = generate(&next_cat, &swapped, initial.index(), &existing);
    // The new index is durable but XML is still old when the process dies.
    let recovered = generate(&next_cat, &swapped, &prepared.index().staged(), &existing);
    assert_eq!(prepared.gamelists(), recovered.gamelists());
    let xml = gamelist(&recovered, "gb");
    let first = xml.split("</game>").next().unwrap();
    assert!(
        first.contains("./RomM/A.gb") && first.contains("<playcount>22</playcount>"),
        "{xml}"
    );
}

#[test]
fn removed_games_leave_user_entries_and_history_is_not_given_to_a_new_game() {
    let (platforms, roms) = sources("Game.gb");
    let cat = build_catalogue("s", &platforms, &roms, |_| {}).unwrap();
    let initial = generate(
        &cat,
        &roms,
        &MetadataIndex::default(),
        &FrontendGamelists::default(),
    );
    let existing = "<gameList><game><path>./Local.gb</path></game><game><path>./RomM/Game.gb</path><playcount>11</playcount></game></gameList>";
    let empty_cat = build_catalogue("s", &[], &[], |_| {}).unwrap();
    let empty = generate(&empty_cat, &[], initial.index(), &frontend("gb", existing));
    let xml = gamelist(&empty, "gb");
    assert!(
        xml.contains("./Local.gb") && !xml.contains("./RomM/"),
        "{xml}"
    );

    let mut replacement = roms.clone();
    replacement[0].id = 2;
    replacement[0].files[0].id = 2;
    let replacement_cat = build_catalogue("s", &platforms, &replacement, |_| {}).unwrap();
    let replaced = generate(
        &replacement_cat,
        &replacement,
        empty.index(),
        &FrontendGamelists::default(),
    );
    let replacement_xml = frontend(
        "gb",
        "<gameList><game><path>./RomM/Game.gb</path><playcount>99</playcount></game></gameList>",
    );
    let restored = generate(&cat, &roms, replaced.index(), &replacement_xml);
    let xml = gamelist(&restored, "gb");
    assert!(xml.contains("<playcount>11</playcount>"), "{xml}");
    assert!(!xml.contains("<playcount>99</playcount>"));
}

#[test]
fn gamelists_are_read_and_written_only_for_linked_systems() {
    let dir = tempfile::tempdir().unwrap();
    let install = dir.path();
    for system in ["gb", "nes"] {
        fs::create_dir_all(install.join("roms").join(system)).unwrap();
        fs::write(
            install.join("roms").join(system).join("gamelist.xml"),
            "\u{feff}<gameList><game><path>./Local.rom</path></game></gameList>",
        )
        .unwrap();
    }
    fs::write(install.join("roms/nes/gamelist.xml"), [0xff, 0xfe, 0x00]).unwrap();
    let linked = vec!["gb".to_string(), "nes".to_string(), "snes".to_string()];
    let read = read_gamelists(install, &linked);
    let (platforms, roms) = sources("Game.gb");
    let cat = build_catalogue("s", &platforms, &roms, |_| {}).unwrap();
    let output = generate(&cat, &roms, &MetadataIndex::default(), &read);
    // The non-UTF-8 nes gamelist is reported and never rewritten.
    assert!(output.skipped().iter().any(|(system, _)| system == "nes"));
    assert!(publish_gamelists(install, &["gb".into()], &output).is_empty());
    let written = fs::read_to_string(install.join("roms/gb/gamelist.xml")).unwrap();
    assert!(written.contains("./Local.rom") && written.contains("./RomM/Game.gb"));
    assert_eq!(
        fs::read(install.join("roms/nes/gamelist.xml")).unwrap(),
        [0xff, 0xfe, 0x00]
    );
    // Systems that aren't linked are never written.
    let only_nes = publish_gamelists(install, &["nes".into()], &output);
    assert!(only_nes.is_empty());
    assert_eq!(
        fs::read_to_string(install.join("roms/gb/gamelist.xml")).unwrap(),
        written
    );
}

#[test]
fn skipped_system_recaptures_history_after_its_gamelist_becomes_readable() {
    let (platforms, roms) = sources("Game.gb");
    let cat = build_catalogue("s", &platforms, &roms, |_| {}).unwrap();
    let initial = generate(
        &cat,
        &roms,
        &MetadataIndex::default(),
        &FrontendGamelists::default(),
    );
    let xml = "<gameList><game><path>./RomM/Game.gb</path><favorite>true</favorite><playcount>12</playcount></game></gameList>";
    let skipped = generate(&cat, &roms, initial.index(), &frontend("gb", "<broken/>"));
    assert!(!skipped.gamelists().contains_key("gb/gamelist.xml"));
    let recovered = generate(&cat, &roms, &reload(skipped.index()), &frontend("gb", xml));
    let recovered_xml = gamelist(&recovered, "gb");
    assert!(
        recovered_xml.contains("<playcount>12</playcount>"),
        "{recovered_xml}"
    );
    assert!(
        recovered_xml.contains("<favorite>true</favorite>"),
        "{recovered_xml}"
    );

    // I/O/encoding failures take the same preservation path as invalid XML.
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("roms/gb")).unwrap();
    fs::write(dir.path().join("roms/gb/gamelist.xml"), [0xff]).unwrap();
    let unreadable = read_gamelists(dir.path(), &["gb".into()]);
    let skipped = generate(&cat, &roms, initial.index(), &unreadable);
    let recovered = generate(&cat, &roms, &reload(skipped.index()), &frontend("gb", xml));
    assert!(gamelist(&recovered, "gb").contains("<playcount>12</playcount>"));
}
