use rommfs_core::retrobat::{
    configure_switch_saves, setup_roms_link, switch_save_setting, RomsConflicts,
};
#[cfg(unix)]
use rommfs_core::retrobat::verify_setup_link;
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

#[test]
fn stock_and_unowned_files_are_reported_and_preserved_without_partial_setup() {
    let dir = tempfile::tempdir().unwrap();
    let (install, managed) = installation(dir.path());
    fs::write(install.join("roms/gb/stock.txt"), b"stock").unwrap();
    fs::write(install.join("roms/game.nsp"), b"user data").unwrap();
    let error = setup_roms_link(&install, &managed, "s", false).unwrap_err();
    let conflicts = error
        .downcast_ref::<RomsConflicts>()
        .expect("typed conflict report");
    assert_eq!(conflicts.blocking.len(), 2);
    let error = error.to_string();
    assert!(
        error.contains("stock.txt") && error.contains("game.nsp"),
        "{error}"
    );
    assert_eq!(
        fs::read(install.join("roms/game.nsp")).unwrap(),
        b"user data"
    );
    assert!(!install.join(".rommfs-setup.json").exists());
    assert!(install.join("roms/gb/empty").is_dir());
}

#[test]
fn consented_cleanup_removes_only_stock_and_noise_never_user_files() {
    let dir = tempfile::tempdir().unwrap();
    let (install, managed) = installation(dir.path());
    fs::write(install.join("roms/gb/_info.txt"), b"stock info").unwrap();
    fs::write(install.join("roms/gb/desktop.ini"), b"noise").unwrap();
    fs::write(install.join("roms/Thumbs.db"), b"noise").unwrap();
    // Without consent the removable files still refuse setup and survive.
    let error = setup_roms_link(&install, &managed, "s", false).unwrap_err();
    let conflicts = error
        .downcast_ref::<RomsConflicts>()
        .expect("cleanup UI needs the typed report");
    assert_eq!(conflicts.removable.len(), 3);
    assert!(conflicts.blocking.is_empty());
    let error = error.to_string();
    assert!(error.contains("_info.txt"), "{error}");
    assert!(install.join("roms/gb/_info.txt").exists());
    // With consent the classified files go and setup completes.
    setup_roms_link(&install, &managed, "s", true).unwrap();
    assert_eq!(fs::canonicalize(install.join("roms")).unwrap(), managed);
    assert!(!install.join("roms").join("gb/_info.txt").exists());
    assert!(install.join(".rommfs-setup.json").exists());
}

#[test]
fn user_files_block_setup_even_after_consent_and_are_never_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let (install, managed) = installation(dir.path());
    fs::write(install.join("roms/gb/_info.txt"), b"stock info").unwrap();
    fs::write(install.join("roms/gb/gamelist.xml"), b"<gameList/>").unwrap();
    fs::write(install.join("roms/game.nsp"), b"user data").unwrap();
    let error = setup_roms_link(&install, &managed, "s", true)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("gamelist.xml") && error.contains("game.nsp"),
        "{error}"
    );
    // The consented stock file was removed; user files were not, and no
    // record or link was created.
    assert!(!install.join("roms/gb/_info.txt").exists());
    assert_eq!(
        fs::read(install.join("roms/game.nsp")).unwrap(),
        b"user data"
    );
    assert!(!install.join(".rommfs-setup.json").exists());
    assert!(!path_is_link(&install.join("roms")));
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
    let record = setup_roms_link(&install, &managed, "s", false).unwrap();
    assert_eq!(record.version.as_deref(), Some("9.0.1-compatible"));
    assert_eq!(fs::canonicalize(install.join("roms")).unwrap(), managed);
    setup_roms_link(&install, &managed, "s", false).unwrap();
    assert!(setup_roms_link(&install, &managed, "other", false).is_err());
    let other = dir.path().join("other");
    fs::create_dir(&other).unwrap();
    assert!(setup_roms_link(&install, &other, "s", false).is_err());
    fs::remove_file(install.join("roms")).unwrap();
    std::os::unix::fs::symlink(&other, install.join("roms")).unwrap();
    assert!(verify_setup_link(&install, &managed, "s").is_err());
    assert_eq!(fs::canonicalize(install.join("roms")).unwrap(), other);
}

#[cfg(unix)]
#[test]
fn unowned_links_and_nested_reparse_points_are_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let (install, managed) = installation(dir.path());
    std::os::unix::fs::symlink(&managed, install.join("roms/gb/link")).unwrap();
    // Links are classified removable: still refused without consent, deleted
    // (link only — target intact) with consent.
    assert!(setup_roms_link(&install, &managed, "s", false).is_err());
    assert!(fs::symlink_metadata(install.join("roms/gb/link"))
        .unwrap()
        .file_type()
        .is_symlink());
    setup_roms_link(&install, &managed, "s", true).unwrap();
    assert!(!install.join("roms/gb/link").exists());
    assert!(managed.is_dir());
    assert_eq!(fs::canonicalize(install.join("roms")).unwrap(), managed);
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
