#[cfg(unix)]
use rommfs_core::retrobat::verify_setup_link;
use rommfs_core::retrobat::{configure_switch_saves, setup_roms_link, switch_save_setting};
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

#[cfg(unix)]
#[test]
fn stock_retrobat_roms_are_preserved_and_managed_gamelists_are_visible() {
    let dir = tempfile::tempdir().unwrap();
    let (install, managed) = installation(dir.path());
    let existing = write_stock_roms(&install);
    let outcome = setup_roms_link(&install, &managed, "s").unwrap();
    assert!(outcome.skipped.is_empty());
    let backup = outcome.original_roms.as_ref().unwrap();
    assert!(outcome.summary().contains(&backup.display().to_string()));
    assert_eq!(outcome.linked, ["gb", "halflife", "ports", "singe", "snes"]);
    for system in &outcome.linked {
        assert!(path_is_link(&install.join("roms").join(system)));
        assert_eq!(
            fs::read_link(install.join("roms").join(system)).unwrap(),
            fs::canonicalize(&managed).unwrap().join(system)
        );
    }
    assert!(!path_is_link(&install.join("roms")));
    for (path, bytes) in &existing {
        let relative = path.strip_prefix(install.join("roms")).unwrap();
        let system = relative.components().next().unwrap().as_os_str();
        let preserved = if outcome.linked.iter().any(|name| name.as_str() == system) {
            backup.join(relative)
        } else {
            path.clone()
        };
        assert_eq!(
            &fs::read(&preserved).unwrap(),
            bytes,
            "{}",
            preserved.display()
        );
    }
    fs::create_dir(managed.join("ports")).unwrap();
    fs::write(managed.join("ports/gamelist.xml"), b"managed gamelist").unwrap();
    assert_eq!(
        fs::read(install.join("roms/ports/gamelist.xml")).unwrap(),
        b"managed gamelist"
    );
    let record = fs::read_to_string(install.join(".rommfs-setup.json")).unwrap();
    assert!(record.contains("\"schema\":2"));
    // An interrupted link step is recoverable without overwriting the backup.
    fs::remove_file(install.join("roms/ports")).unwrap();
    let again = setup_roms_link(&install, &managed, "s").unwrap();
    assert_eq!(again.linked, outcome.linked);
    assert!(again.skipped.is_empty());
    assert_eq!(again.original_roms, outcome.original_roms);
    assert_eq!(
        fs::read(install.join("roms/ports/gamelist.xml")).unwrap(),
        b"managed gamelist"
    );
    assert_eq!(
        fs::read(backup.join("ports/gamelist.xml")).unwrap(),
        b"stock bytes of roms/ports/gamelist.xml"
    );
    verify_setup_link(&install, &managed, "s").unwrap();
}

#[cfg(unix)]
#[test]
fn original_rom_backups_are_never_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let (install, managed) = installation(dir.path());
    let backup = install.join(".rommfs-original-roms/gb");
    fs::create_dir_all(&backup).unwrap();
    fs::write(backup.join("original.gb"), b"original game").unwrap();
    fs::write(install.join("roms/gb/new.gb"), b"new game").unwrap();
    assert!(setup_roms_link(&install, &managed, "s").is_err());
    assert_eq!(
        fs::read(backup.join("original.gb")).unwrap(),
        b"original game"
    );
    assert_eq!(
        fs::read(install.join("roms/gb/new.gb")).unwrap(),
        b"new game"
    );
    assert!(!path_is_link(&install.join("roms/gb")));
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
    assert!(path_is_link(&install.join("roms/snes")));
}

#[cfg(unix)]
#[test]
fn empty_system_directories_are_replaced_and_foreign_links_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let (install, managed) = installation(dir.path());
    fs::write(
        install.join("emulationstation/.emulationstation/es_systems.cfg"),
        STOCK_SYSTEMS,
    )
    .unwrap();
    let elsewhere = dir.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, install.join("roms/snes")).unwrap();
    let outcome = setup_roms_link(&install, &managed, "s").unwrap();
    // roms/gb/empty was an empty directory tree: replaced by the link.
    assert!(outcome.linked.contains(&"gb".to_string()));
    assert!(path_is_link(&install.join("roms/gb")));
    assert!(outcome.skipped.contains(&"snes".to_string()));
    assert_eq!(fs::read_link(install.join("roms/snes")).unwrap(), elsewhere);
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
fn setup_accepts_compatible_versions_is_repeatable_and_rejects_wrong_ownership() {
    let dir = tempfile::tempdir().unwrap();
    let (install, managed) = installation(dir.path());
    let outcome = setup_roms_link(&install, &managed, "s").unwrap();
    assert_eq!(outcome.record.version.as_deref(), Some("9.0.1-compatible"));
    assert_eq!(
        fs::read_link(install.join("roms/gb")).unwrap(),
        fs::canonicalize(&managed).unwrap().join("gb")
    );
    setup_roms_link(&install, &managed, "s").unwrap();
    assert!(setup_roms_link(&install, &managed, "other").is_err());
    let other = dir.path().join("other");
    fs::create_dir(&other).unwrap();
    assert!(setup_roms_link(&install, &other, "s").is_err());
    // A recorded link retargeted elsewhere is rejected and left untouched.
    fs::remove_file(install.join("roms/gb")).unwrap();
    std::os::unix::fs::symlink(&other, install.join("roms/gb")).unwrap();
    assert!(verify_setup_link(&install, &managed, "s").is_err());
    assert_eq!(fs::read_link(install.join("roms/gb")).unwrap(), other);
}

#[cfg(unix)]
#[test]
fn legacy_whole_roms_link_is_still_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let (install, managed) = installation(dir.path());
    fs::remove_dir_all(install.join("roms")).unwrap();
    let canonical = fs::canonicalize(&managed).unwrap();
    std::os::unix::fs::symlink(&canonical, install.join("roms")).unwrap();
    let version = "9.0.1-compatible";
    fs::write(
        install.join(".rommfs-setup.json"),
        format!(
            "{{\"schema\":1,\"server_id\":\"s\",\"managed_root\":{:?},\"version\":\"{version}\"}}",
            canonical.to_str().unwrap()
        ),
    )
    .unwrap();
    verify_setup_link(&install, &managed, "s").unwrap();
    let outcome = setup_roms_link(&install, &managed, "s").unwrap();
    assert!(outcome.legacy_whole_link && path_is_link(&install.join("roms")));
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
