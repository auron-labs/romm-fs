use rommfs_core::retrobat::{configure_switch_saves, setup_roms_link, switch_save_setting};
#[cfg(unix)]
use rommfs_core::retrobat::{verify_setup_link, ROMM_FOLDER};
use std::fs;
use std::path::{Path, PathBuf};

fn installation(parent: &Path) -> (PathBuf, PathBuf) {
    let install = parent.join("RetroBat");
    let managed = parent.join("managed");
    fs::create_dir_all(install.join("emulationstation/.emulationstation")).unwrap();
    fs::create_dir_all(install.join("system")).unwrap();
    fs::create_dir_all(install.join("roms/gb/empty")).unwrap();
    fs::create_dir(&managed).unwrap();
    fs::write(install.join("retrobat.exe"), b"fixture").unwrap();
    fs::write(install.join("system/version.info"), b"9.0.1-compatible").unwrap();
    fs::write(
        install.join("emulationstation/emulatorLauncher.cfg"),
        "home=.\\.emulationstation\nsaves=..\\saves\n",
    )
    .unwrap();
    fs::write(install.join("emulationstation/.emulationstation/es_systems.cfg"), "<systemList><system><name>gb</name><path>~/../roms/gb</path><extension>.gb</extension></system></systemList>").unwrap();
    fs::write(
        install.join("emulationstation/.emulationstation/es_settings.cfg"),
        "<config><string name=\"unrelated\" value=\"keep &amp; preserve\"/></config>",
    )
    .unwrap();
    (install, managed)
}

const STOCK_SYSTEMS: &str = "<systemList>\
<system><name>gb</name><path>~/../roms/gb</path><extension>.gb</extension></system>\
<system><name>snes</name><path>~/../roms/snes</path><extension>.sfc</extension></system>\
<system><name>singe</name><path>~/../roms/singe</path><extension>.singe</extension></system>\
<system><name>ports</name><path>~/../roms/ports</path><extension>.libretro</extension></system>\
<system><name>halflife</name><path>~/../roms/halflife</path><extension>.pak</extension></system>\
</systemList>";

/// Compact subset of a pristine RetroBat 8.3.0 `roms` tree (see the
/// clean-machine file listing): singe, ports and halflife ship content.
fn write_stock_roms(install: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fs::write(
        install.join("emulationstation/.emulationstation/es_systems.cfg"),
        STOCK_SYSTEMS,
    )
    .unwrap();
    let files = [
        "roms/singe/ldp.singe",
        "roms/singe/Dragon's Lair/dl.singe",
        "roms/singe/Dragon's Lair/dl.txt",
        "roms/ports/2048.libretro",
        "roms/ports/gamelist.xml",
        "roms/ports/images/2048.png",
        "roms/ports/xrick/data.zip",
        "roms/halflife/extras.pak",
        "roms/halflife/valve/config.cfg",
        "roms/halflife/valve/extras.pk3",
        "roms/quake/_info.txt",
        "roms/user-dump/game.nsp",
    ];
    files
        .iter()
        .map(|rel| {
            let path = install.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let bytes = format!("stock bytes of {rel}").into_bytes();
            fs::write(&path, &bytes).unwrap();
            (path, bytes)
        })
        .collect()
}

/// Every stock file, byte for byte, plus the folders that held them.
fn assert_untouched(existing: &[(PathBuf, Vec<u8>)]) {
    for (path, bytes) in existing {
        assert_eq!(&fs::read(path).unwrap(), bytes, "{}", path.display());
    }
}

#[cfg(unix)]
#[test]
fn romm_games_are_linked_alongside_stock_roms_without_touching_them() {
    let dir = tempfile::tempdir().unwrap();
    let (install, managed) = installation(dir.path());
    let existing = write_stock_roms(&install);
    let outcome = setup_roms_link(&install, &managed, "s").unwrap();
    assert!(outcome.skipped.is_empty(), "{:?}", outcome.skipped);
    assert_eq!(outcome.linked, ["gb", "halflife", "ports", "singe", "snes"]);
    let canonical = fs::canonicalize(&managed).unwrap();
    for system in &outcome.linked {
        let folder = install.join("roms").join(system);
        assert!(!path_is_link(&folder), "{system} must stay a real folder");
        assert_eq!(
            fs::read_link(folder.join(ROMM_FOLDER)).unwrap(),
            canonical.join(system)
        );
    }
    assert_untouched(&existing);
    assert!(!install.join(".rommfs-original-roms").exists());
    // snes had no folder: RomMFS created it and recorded that.
    assert!(outcome.record.created_dirs.contains(&"snes".to_string()));
    let record = fs::read_to_string(install.join(".rommfs-setup.json")).unwrap();
    assert!(record.contains("\"schema\":3"));
    verify_setup_link(&install, &managed, "s").unwrap();

    // Repeatable, and a removed link is restored on the next run.
    fs::remove_file(install.join("roms/ports").join(ROMM_FOLDER)).unwrap();
    let again = setup_roms_link(&install, &managed, "s").unwrap();
    assert_eq!(again.linked, outcome.linked);
    assert!(path_is_link(&install.join("roms/ports").join(ROMM_FOLDER)));
    assert_untouched(&existing);
}

#[cfg(unix)]
#[test]
fn one_system_that_cannot_be_linked_does_not_stop_the_others() {
    let dir = tempfile::tempdir().unwrap();
    let (install, managed) = installation(dir.path());
    fs::write(
        install.join("emulationstation/.emulationstation/es_systems.cfg"),
        STOCK_SYSTEMS,
    )
    .unwrap();
    let elsewhere = dir.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    // A user's own link, a file where a folder should be, and an existing RomM entry.
    std::os::unix::fs::symlink(&elsewhere, install.join("roms/snes")).unwrap();
    fs::write(install.join("roms/singe"), b"not a folder").unwrap();
    fs::create_dir_all(install.join("roms/ports/RomM")).unwrap();
    fs::write(install.join("roms/ports/RomM/mine.txt"), b"user file").unwrap();
    let outcome = setup_roms_link(&install, &managed, "s").unwrap();
    let skipped: Vec<_> = outcome.skipped.iter().map(|s| s.system.as_str()).collect();
    assert_eq!(skipped, ["ports", "singe", "snes"]);
    assert!(outcome.skipped.iter().all(|s| !s.reason.is_empty()));
    assert_eq!(outcome.linked, ["gb", "halflife"]);
    assert!(outcome.summary().contains("3 skipped"));
    assert_eq!(fs::read_link(install.join("roms/snes")).unwrap(), elsewhere);
    assert_eq!(
        fs::read(install.join("roms/singe")).unwrap(),
        b"not a folder"
    );
    assert_eq!(
        fs::read(install.join("roms/ports/RomM/mine.txt")).unwrap(),
        b"user file"
    );
    assert!(!path_is_link(&install.join("roms/ports/RomM")));
}

#[cfg(unix)]
#[test]
fn rerun_adds_links_for_systems_that_become_available() {
    let dir = tempfile::tempdir().unwrap();
    let (install, managed) = installation(dir.path());
    setup_roms_link(&install, &managed, "s").unwrap();
    assert!(!install.join("roms/snes").exists());
    fs::write(
        install.join("emulationstation/.emulationstation/es_systems.cfg"),
        STOCK_SYSTEMS,
    )
    .unwrap();
    let outcome = setup_roms_link(&install, &managed, "s").unwrap();
    assert!(outcome.linked.contains(&"snes".to_string()));
    assert!(path_is_link(&install.join("roms/snes").join(ROMM_FOLDER)));
}

#[cfg(unix)]
#[test]
fn earlier_layouts_are_converted_and_moved_folders_come_back() {
    let dir = tempfile::tempdir().unwrap();
    let (install, managed) = installation(dir.path());
    fs::write(
        install.join("emulationstation/.emulationstation/es_systems.cfg"),
        STOCK_SYSTEMS,
    )
    .unwrap();
    let canonical = fs::canonicalize(&managed).unwrap();
    // Schema 2: whole-system links, with an original folder moved aside.
    fs::remove_dir_all(install.join("roms/gb")).unwrap();
    std::os::unix::fs::symlink(canonical.join("gb"), install.join("roms/gb")).unwrap();
    let backup = install.join(".rommfs-original-roms/ports");
    fs::create_dir_all(&backup).unwrap();
    fs::write(backup.join("2048.libretro"), b"stock game").unwrap();
    std::os::unix::fs::symlink(canonical.join("ports"), install.join("roms/ports")).unwrap();
    fs::write(
        install.join(".rommfs-setup.json"),
        format!(
            "{{\"schema\":2,\"server_id\":\"s\",\"managed_root\":{:?},\"version\":null,\"links\":[\"gb\",\"ports\"]}}",
            canonical.to_str().unwrap()
        ),
    )
    .unwrap();
    assert!(verify_setup_link(&install, &managed, "s").is_err());
    let outcome = setup_roms_link(&install, &managed, "s").unwrap();
    assert_eq!(outcome.restored, ["ports"]);
    assert!(outcome.skipped.is_empty(), "{:?}", outcome.skipped);
    for system in ["gb", "ports"] {
        assert!(!path_is_link(&install.join("roms").join(system)));
        assert!(path_is_link(
            &install.join("roms").join(system).join(ROMM_FOLDER)
        ));
    }
    assert_eq!(
        fs::read(install.join("roms/ports/2048.libretro")).unwrap(),
        b"stock game"
    );
    assert!(!install.join(".rommfs-original-roms").exists());
    verify_setup_link(&install, &managed, "s").unwrap();
}

#[cfg(unix)]
#[test]
fn a_backup_that_cannot_go_back_is_left_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let (install, managed) = installation(dir.path());
    let backup = install.join(".rommfs-original-roms/gb");
    fs::create_dir_all(&backup).unwrap();
    fs::write(backup.join("original.gb"), b"original game").unwrap();
    fs::write(install.join("roms/gb/new.gb"), b"new game").unwrap();
    let outcome = setup_roms_link(&install, &managed, "s").unwrap();
    // RetroBat recreated roms/gb meanwhile: both folders are kept as they are.
    assert!(outcome.restored.is_empty());
    assert_eq!(outcome.linked, ["gb"]);
    assert_eq!(
        fs::read(backup.join("original.gb")).unwrap(),
        b"original game"
    );
    assert_eq!(
        fs::read(install.join("roms/gb/new.gb")).unwrap(),
        b"new game"
    );
}

#[test]
fn missing_system_configuration_asks_to_start_retrobat_once() {
    let dir = tempfile::tempdir().unwrap();
    let (install, managed) = installation(dir.path());
    fs::remove_file(install.join("emulationstation/.emulationstation/es_systems.cfg")).unwrap();
    let error = setup_roms_link(&install, &managed, "s")
        .unwrap_err()
        .to_string();
    assert!(error.contains("Start RetroBat once, close it"), "{error}");
}

#[cfg(unix)]
fn path_is_link(path: &Path) -> bool {
    fs::symlink_metadata(path).unwrap().file_type().is_symlink()
}
#[cfg(not(unix))]
fn path_is_link(path: &Path) -> bool {
    rommfs_core::save_sync::path_is_reparse_point(&fs::symlink_metadata(path).unwrap())
}

#[cfg(unix)]
#[test]
fn setup_retargets_its_own_links_when_the_games_folder_or_server_changes() {
    let dir = tempfile::tempdir().unwrap();
    let (install, managed) = installation(dir.path());
    let outcome = setup_roms_link(&install, &managed, "s").unwrap();
    assert_eq!(outcome.record.version.as_deref(), Some("9.0.1-compatible"));
    setup_roms_link(&install, &managed, "s").unwrap();
    assert!(verify_setup_link(&install, &managed, "other").is_err());
    let other = dir.path().join("other");
    fs::create_dir(&other).unwrap();
    setup_roms_link(&install, &other, "other").unwrap();
    assert_eq!(
        fs::read_link(install.join("roms/gb").join(ROMM_FOLDER)).unwrap(),
        fs::canonicalize(&other).unwrap().join("gb")
    );
    verify_setup_link(&install, &other, "other").unwrap();
    assert!(verify_setup_link(&install, &managed, "s").is_err());
}

#[cfg(unix)]
#[test]
fn legacy_whole_roms_link_becomes_a_real_folder() {
    let dir = tempfile::tempdir().unwrap();
    let (install, managed) = installation(dir.path());
    fs::remove_dir_all(install.join("roms")).unwrap();
    let canonical = fs::canonicalize(&managed).unwrap();
    std::os::unix::fs::symlink(&canonical, install.join("roms")).unwrap();
    fs::write(
        install.join(".rommfs-setup.json"),
        format!(
            "{{\"schema\":1,\"server_id\":\"s\",\"managed_root\":{:?},\"version\":\"9.0.1-compatible\"}}",
            canonical.to_str().unwrap()
        ),
    )
    .unwrap();
    let outcome = setup_roms_link(&install, &managed, "s").unwrap();
    assert!(!path_is_link(&install.join("roms")));
    assert_eq!(outcome.linked, ["gb"]);
    assert!(path_is_link(&install.join("roms/gb").join(ROMM_FOLDER)));
    // A roms link RomMFS doesn't own is reported, not replaced.
    let dir = tempfile::tempdir().unwrap();
    let (install, managed) = installation(dir.path());
    let elsewhere = dir.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    fs::remove_dir_all(install.join("roms")).unwrap();
    std::os::unix::fs::symlink(&elsewhere, install.join("roms")).unwrap();
    assert!(setup_roms_link(&install, &managed, "s").is_err());
    assert_eq!(fs::read_link(install.join("roms")).unwrap(), elsewhere);
}

#[test]
fn targeted_switch_setting_preserves_xml_and_refuses_overrides_or_invalid_documents() {
    let xml = "<?xml version=\"1.0\"?><config><!-- keep --><string name=\"unrelated\" value=\"A &amp; B\"/><bool name=\"switch.yuzu_mutualize\" value=\"false\"/></config>";
    let after = switch_save_setting(xml).unwrap();
    assert!(after.contains("<!-- keep -->") && after.contains("value=\"A &amp; B\""));
    assert!(after.contains("name=\"switch.yuzu_mutualize\" value=\"true\""));
    assert_eq!(switch_save_setting(&after).unwrap(), after);
    assert!(switch_save_setting("<config/>")
        .unwrap()
        .contains("switch.yuzu_mutualize"));
    for xml in ["<config><bool></config>", "<config><bool name=\"switch.yuzu_mutualize\" value=\"true\"/>", "<config/><other/>", "<!DOCTYPE config><config/>", "<not-config/>", "<config><bool name=\"switch[&quot;Game.nsp&quot;].yuzu_mutualize\" value=\"false\"/></config>"] {
        assert!(switch_save_setting(xml).is_err(), "{xml}");
    }
}

#[test]
fn config_backup_is_local_repeatable_and_missing_emulators_are_not_setting_errors() {
    let dir = tempfile::tempdir().unwrap();
    let (install, _) = installation(dir.path());
    let home = install.join("emulationstation/.emulationstation");
    let before = fs::read(home.join("es_settings.cfg")).unwrap();
    let messages = configure_switch_saves(&install).unwrap();
    assert!(messages.iter().any(|m| m.contains("eden is not installed")));
    assert!(messages
        .iter()
        .any(|m| m.contains("citron is not installed")));
    let backups = || {
        fs::read_dir(&home)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|s| s == "bak"))
            .map(|e| e.path())
            .collect::<Vec<_>>()
    };
    assert_eq!(backups().len(), 1);
    assert_eq!(fs::read(&backups()[0]).unwrap(), before);
    configure_switch_saves(&install).unwrap();
    assert_eq!(backups().len(), 1);
}

#[cfg(unix)]
#[test]
fn publication_excludes_missing_replaced_and_redirected_links() {
    use rommfs_core::retrobat::{
        generate_gamelists, inspect_setup, linked_systems, publish_gamelists, read_gamelists,
        MetadataIndex,
    };
    use std::collections::{BTreeMap, HashMap};

    let dir = tempfile::tempdir().unwrap();
    let (install, managed) = installation(dir.path());
    write_stock_roms(&install);
    setup_roms_link(&install, &managed, "s").unwrap();
    let folder = install.join("roms/ports");
    let link = folder.join(ROMM_FOLDER);
    let xml =
        "<gameList><game><path>./RomM/Mine.libretro</path><name>User game</name></game></gameList>";
    fs::write(folder.join("gamelist.xml"), xml).unwrap();
    fs::remove_file(&link).unwrap();
    let elsewhere = dir.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    let empty = rommfs_core::catalogue::build_catalogue("s", &[], &[], |_| {}).unwrap();

    for state in ["missing", "directory", "foreign link", "foreign parent"] {
        match state {
            "directory" => {
                fs::create_dir(&link).unwrap();
                fs::write(link.join("Mine.libretro"), b"user game").unwrap();
            }
            "foreign link" => {
                fs::remove_file(link.join("Mine.libretro")).unwrap();
                fs::remove_dir(&link).unwrap();
                std::os::unix::fs::symlink(&elsewhere, &link).unwrap();
            }
            "foreign parent" => {
                fs::rename(&folder, dir.path().join("original-ports")).unwrap();
                fs::write(elsewhere.join("gamelist.xml"), xml).unwrap();
                std::os::unix::fs::symlink(managed.join("ports"), elsewhere.join(ROMM_FOLDER))
                    .unwrap();
                std::os::unix::fs::symlink(&elsewhere, &folder).unwrap();
            }
            _ => {}
        }
        let linked = linked_systems(&install, &managed, "s").unwrap();
        assert!(!linked.iter().any(|s| s == "ports"), "{state}");
        // Targets need not exist yet: setup precedes mounting the library.
        assert!(linked.iter().any(|s| s == "gb"), "{state}");
        assert!(inspect_setup(&install, &managed, "s")
            .unwrap()
            .skipped
            .iter()
            .any(|s| s.system == "ports"));
        let frontend = read_gamelists(&install, &linked);
        let output = generate_gamelists(
            &empty,
            &[],
            "s",
            7,
            &MetadataIndex::default(),
            &frontend,
            &BTreeMap::new(),
            &HashMap::new(),
        )
        .unwrap();
        assert!(publish_gamelists(&install, &linked, &output).is_empty());
        assert_eq!(
            fs::read_to_string(folder.join("gamelist.xml")).unwrap(),
            xml,
            "{state}"
        );
    }
}
