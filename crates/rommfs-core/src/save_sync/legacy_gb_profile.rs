//! Compatibility verification for the older Game Boy/Gambatte save profile.
//!
//! The application uses this verifier while previewing a selected RetroBat
//! installation and again before transfers that contain Game Boy profile
//! mappings. The ordinary-file save-sync flow validates its launcher save root
//! directly and does not need Game Boy profile rules. Keeping this verifier
//! preserves compatibility with previously consented `gb/*.srm` mappings and
//! their journal identity; it does not migrate or reinterpret those mappings.

use super::installation::{find_emulationstation_config, validate_installation};
use super::retrobat_paths::{
    configured_install_path, lookup, read_key_values, read_limited_text, resolve_launcher_path,
};
use super::settings::windows_path_key;
use crate::error::{Error, Result};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_OVERRIDE_FILES: usize = 512;
const MAX_SYSTEM_CONFIG_FILES: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetroBatGbProfile {
    pub version: String,
    pub effective_saves_root: PathBuf,
}

/// Resolve the source-verified RetroBat Game Boy/Gambatte layout without
/// creating directories or changing any emulator configuration.
///
/// This compatibility verifier runs during save-sync preview and before
/// transfers using the persisted legacy `gb/*.srm` mapping. Ordinary file
/// mappings take the separate launcher-root validation path.
pub fn resolve_retrobat_gb_profile(
    installation_root: &Path,
    visible_gb_rom_names: &[String],
) -> Result<RetroBatGbProfile> {
    super::path::ensure_no_reparse_components(installation_root)?;
    let install = validate_installation(installation_root).map_err(|problem| {
        Error::Unsupported(format!("RetroBat install is not valid: {problem:?}"))
    })?;
    let launcher_dir = installation_root.join("emulationstation");
    let launcher_config = read_key_values(&launcher_dir.join("emulatorLauncher.cfg"))?;
    let local_path = configured_install_path(&launcher_config, installation_root, &launcher_dir);
    let retrobat_root = resolve_launcher_path(
        lookup(&launcher_config, "retrobat"),
        &local_path,
        installation_root,
        true,
    )?;
    if windows_path_key(&retrobat_root) != windows_path_key(installation_root) {
        return Err(Error::Unsupported(
            "RetroBat launcher root differs from the selected installation".into(),
        ));
    }
    let version_path = retrobat_root.join("system/version.info");
    let version = read_limited_text(&version_path)?.trim().to_owned();

    let home_default = local_path.join(".emulationstation");
    let home = resolve_launcher_path(
        lookup(&launcher_config, "home"),
        &local_path,
        &home_default,
        false,
    )?;
    let settings_file = home.join("es_settings.cfg");
    let system_settings = read_xml_settings(&settings_file)?;
    let saves_default = local_path.parent().unwrap_or(&local_path).join("saves");
    let effective_saves_root = resolve_launcher_path(
        lookup(&launcher_config, "saves"),
        &local_path,
        &saves_default,
        false,
    )?;
    if !effective_saves_root.is_absolute() {
        return Err(Error::Unsupported(
            "RetroBat effective save root is not an absolute path".into(),
        ));
    }
    super::path::ensure_no_reparse_components(&effective_saves_root)?;

    let systems_file = find_emulationstation_config(&installation_root.join("emulationstation"))
        .map_err(|problem| {
            Error::Unsupported(format!("EmulationStation systems config: {problem:?}"))
        })?;
    let systems_files = system_config_files(&systems_file)?;
    let mut configured_systems = HashSet::new();
    for config in &systems_files {
        configured_systems.extend(verify_gameboy_system(config)?);
    }
    verify_system_settings(&system_settings, &effective_saves_root)?;
    verify_retroarch_config(
        &install.retroarch_config,
        &effective_saves_root,
        &configured_systems,
    )?;
    verify_gambatte_overrides(
        &install.retroarch_config,
        visible_gb_rom_names,
        &effective_saves_root,
    )?;

    Ok(RetroBatGbProfile {
        version,
        effective_saves_root,
    })
}

fn read_xml_settings(path: &Path) -> Result<Vec<(String, String)>> {
    let contents = read_limited_text(path)?;
    let mut reader = Reader::from_str(&contents);
    reader.config_mut().trim_text(true);
    let mut settings = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(element) | Event::Empty(element)) => {
                let mut name = None;
                let mut value = None;
                for attribute in element.attributes() {
                    let attribute = attribute.map_err(|error| {
                        Error::Unsupported(format!(
                            "invalid EmulationStation settings XML: {error}"
                        ))
                    })?;
                    let decoded = attribute
                        .normalized_value(quick_xml::XmlVersion::default())
                        .map_err(|error| Error::Unsupported(error.to_string()))?
                        .into_owned();
                    match attribute.key.as_ref() {
                        b"name" => name = Some(decoded),
                        b"value" => value = Some(decoded),
                        _ => {}
                    }
                }
                if let (Some(name), Some(value)) = (name, value) {
                    settings.push((name.to_ascii_lowercase(), value));
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(Error::Unsupported(format!(
                    "invalid EmulationStation settings XML: {error}"
                )))
            }
        }
    }
    Ok(settings)
}

fn system_config_files(base: &Path) -> Result<Vec<PathBuf>> {
    let parent = base
        .parent()
        .ok_or_else(|| Error::Unsupported("EmulationStation config has no parent".into()))?;
    let mut files = vec![base.to_path_buf()];
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name
            .get(.."es_systems_".len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("es_systems_"))
            || !path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("cfg"))
        {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        if super::path::is_reparse_point(&metadata) {
            return Err(Error::Unsupported(format!(
                "EmulationStation systems override is redirected: {}",
                path.display()
            )));
        }
        if metadata.is_file() {
            files.push(path);
            if files.len() > MAX_SYSTEM_CONFIG_FILES {
                return Err(Error::Unsupported(
                    "EmulationStation systems override search exceeded its safety bound".into(),
                ));
            }
        }
    }
    files[1..].sort();
    Ok(files)
}

fn verify_gameboy_system(path: &Path) -> Result<HashSet<String>> {
    let contents = read_limited_text(path)?;
    let mut reader = Reader::from_str(&contents);
    reader.config_mut().trim_text(true);
    let mut in_system = false;
    let mut name = String::new();
    let mut command = String::new();
    let mut field = None;
    let mut current_emulator = String::new();
    let mut has_gambatte = false;
    let mut found_gameboy = false;
    let mut valid_gb_system = false;
    let mut configured_systems = HashSet::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) if element.name().as_ref() == b"system" => {
                in_system = true;
                name.clear();
                command.clear();
                current_emulator.clear();
                has_gambatte = false;
            }
            Ok(Event::Start(element)) if in_system && element.name().as_ref() == b"emulator" => {
                current_emulator = element
                    .attributes()
                    .flatten()
                    .find(|attribute| attribute.key.as_ref() == b"name")
                    .and_then(|attribute| {
                        attribute
                            .normalized_value(quick_xml::XmlVersion::default())
                            .ok()
                    })
                    .unwrap_or_default()
                    .to_ascii_lowercase();
            }
            Ok(Event::Start(element)) if in_system => match element.name().as_ref() {
                b"name" => field = Some("name"),
                b"command" => field = Some("command"),
                _ => {}
            },
            Ok(Event::Text(text)) if in_system => {
                let text = text.decode().unwrap_or_default();
                match field {
                    Some("name") => name.push_str(text.trim()),
                    Some("command") => command.push_str(&text),
                    _ => {}
                }
                if current_emulator == "libretro" && text.trim().eq_ignore_ascii_case("gambatte") {
                    has_gambatte = true;
                }
            }
            Ok(Event::End(element)) if element.name().as_ref() == b"name" => field = None,
            Ok(Event::End(element)) if element.name().as_ref() == b"command" => field = None,
            Ok(Event::End(element)) if element.name().as_ref() == b"emulator" => {
                current_emulator.clear();
            }
            Ok(Event::End(element)) if element.name().as_ref() == b"system" => {
                if !name.is_empty() {
                    configured_systems.insert(name.to_ascii_lowercase());
                }
                if name.eq_ignore_ascii_case("gb") {
                    found_gameboy = true;
                    if is_stock_gameboy_command(&command) && has_gambatte {
                        valid_gb_system = true;
                    }
                }
                in_system = false;
                field = None;
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(Error::Unsupported(error.to_string())),
        }
    }
    if found_gameboy && valid_gb_system {
        Ok(configured_systems)
    } else if found_gameboy {
        Err(Error::Unsupported(
            "Game Boy is not configured for RetroBat's stock libretro Gambatte launcher".into(),
        ))
    } else {
        Ok(configured_systems)
    }
}

fn is_stock_gameboy_command(command: &str) -> bool {
    let mut actual = command
        .split_ascii_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if actual.is_empty() {
        return false;
    }
    let executable = actual.remove(0);
    let Some(executable) = executable
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    else {
        return false;
    };
    let expected = [
        r#"%HOME%\emulatorLauncher.exe"#,
        "-gameinfo",
        "%GAMEINFOXML%",
        "%CONTROLLERSCONFIG%",
        "-system",
        "%SYSTEM%",
        "-emulator",
        "%EMULATOR%",
        "-core",
        "%CORE%",
        "-rom",
        "%ROM%",
    ];
    executable.eq_ignore_ascii_case(expected[0])
        && actual.len() == expected.len() - 1
        && actual
            .iter()
            .zip(&expected[1..])
            .all(|(actual, expected)| actual.eq_ignore_ascii_case(expected))
}

fn verify_system_settings(settings: &[(String, String)], saves_root: &Path) -> Result<()> {
    for (full_key, value) in settings {
        let option = if let Some(option) = full_key.strip_prefix("global.") {
            Some(option)
        } else if let Some(option) = full_key.strip_prefix("gb.") {
            Some(option)
        } else if let Some(per_game) = full_key.strip_prefix("gb[") {
            per_game.split_once("].").map(|(_, option)| option)
        } else if !full_key.contains('.') {
            Some(full_key.as_str())
        } else {
            None
        };
        if let Some(option) = option {
            match option {
                "core" if !value.eq_ignore_ascii_case("gambatte") => {
                    return unsupported_override(full_key);
                }
                "emulator" if !value.eq_ignore_ascii_case("libretro") => {
                    return unsupported_override(full_key);
                }
                "retroarch.savefile_directory" | "savefile_directory"
                    if windows_path_key(Path::new(value))
                        != windows_path_key(&saves_root.join("gb")) =>
                {
                    return unsupported_override(full_key);
                }
                "retroarch.savefiles_in_content_dir" | "savefiles_in_content_dir"
                    if parse_bool(value) != Some(false) =>
                {
                    return unsupported_override(full_key);
                }
                "retroarch.sort_savefiles_enable"
                | "retroarch.sort_savefiles_by_content_enable"
                | "sort_savefiles_enable"
                | "sort_savefiles_by_content_enable"
                    if parse_bool(value) != Some(false) =>
                {
                    return unsupported_override(full_key);
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn unsupported_override<T>(key: &str) -> Result<T> {
    Err(Error::Unsupported(format!(
        "RetroBat override {key:?} changes the verified Game Boy save profile"
    )))
}

fn verify_retroarch_config(
    path: &Path,
    saves_root: &Path,
    configured_systems: &HashSet<String>,
) -> Result<()> {
    let config = read_key_values(path)?;
    verify_retroarch_options(
        &config,
        &saves_root.join("gb"),
        Some((saves_root, configured_systems)),
    )
}

fn verify_gambatte_overrides(
    retroarch_config: &Path,
    visible_rom_names: &[String],
    saves_root: &Path,
) -> Result<()> {
    let retroarch_root = retroarch_config
        .parent()
        .ok_or_else(|| Error::Unsupported("RetroArch config has no parent directory".into()))?;
    let config_dir = find_child_case_insensitive(retroarch_root, "config")?;
    let Some(config_dir) = config_dir else {
        return Ok(());
    };
    let core_dir = find_child_case_insensitive(&config_dir, "gambatte")?;
    let Some(core_dir) = core_dir else {
        return Ok(());
    };
    let mut game_names: HashSet<String> = visible_rom_names
        .iter()
        .filter_map(|name| Path::new(name).file_stem())
        .filter_map(|stem| stem.to_str())
        .map(str::to_ascii_lowercase)
        .collect();
    game_names.insert("gambatte".into());
    game_names.insert("gb".into());
    let mut scanned = 0;
    for entry in fs::read_dir(&core_dir)? {
        let entry = entry?;
        let override_path = entry.path();
        let metadata = fs::symlink_metadata(&override_path)?;
        let Some(extension) = override_path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        let Some(stem) = override_path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if !extension.eq_ignore_ascii_case("cfg")
            || !game_names.contains(&stem.to_ascii_lowercase())
        {
            continue;
        }
        if super::path::is_reparse_point(&metadata) {
            return Err(Error::Unsupported(format!(
                "RetroArch Gambatte override is redirected: {}",
                override_path.display()
            )));
        }
        if !metadata.is_file() {
            continue;
        }
        scanned += 1;
        if scanned > MAX_OVERRIDE_FILES {
            return Err(Error::Unsupported(
                "RetroArch Gambatte override search exceeded its safety bound".into(),
            ));
        }
        let config = read_key_values(&override_path)?;
        verify_retroarch_options(&config, &saves_root.join("gb"), None)?;
    }
    Ok(())
}

fn verify_retroarch_options(
    config: &[(String, String)],
    expected_save_dir: &Path,
    generated_save_root: Option<(&Path, &HashSet<String>)>,
) -> Result<()> {
    for (key, value) in config {
        match key.as_str() {
            "savefile_directory"
                if !value.trim().is_empty()
                    && windows_path_key(Path::new(value))
                        != windows_path_key(expected_save_dir)
                    && !generated_save_root.is_some_and(|(root, systems)| {
                        is_launcher_save_directory(value, root, systems)
                    }) =>
            {
                return unsupported_override(key);
            }
            "savefiles_in_content_dir" if parse_bool(value) != Some(false) => {
                return unsupported_override(key);
            }
            "sort_savefiles_enable" | "sort_savefiles_by_content_enable"
                if parse_bool(value) != Some(false) =>
            {
                return unsupported_override(key);
            }
            "config_directory" if !value.trim().is_empty() && value.trim() != ":" => {
                return unsupported_override(key);
            }
            _ => {}
        }
    }
    Ok(())
}

fn is_launcher_save_directory(value: &str, saves_root: &Path, systems: &HashSet<String>) -> bool {
    if windows_path_key(Path::new(value)) == windows_path_key(Path::new(r":\saves")) {
        return true;
    }
    systems.iter().any(|system| {
        windows_path_key(Path::new(value)) == windows_path_key(&saves_root.join(system))
    })
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

fn find_child_case_insensitive(parent: &Path, name: &str) -> Result<Option<PathBuf>> {
    if !parent.exists() {
        return Ok(None);
    }
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        if entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(name)
        {
            let metadata = fs::symlink_metadata(entry.path())?;
            if super::path::is_reparse_point(&metadata) {
                return Err(Error::Unsupported(format!(
                    "RetroArch override path is redirected: {}",
                    entry.path().display()
                )));
            }
            return Ok(metadata.is_dir().then(|| entry.path()));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn stock_fixture(root: &Path) -> PathBuf {
        let home = root.join("emulationstation/.emulationstation");
        let retroarch = root.join("emulators/retroarch");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&retroarch).unwrap();
        fs::create_dir_all(root.join("system")).unwrap();
        fs::write(root.join("RetroBat.exe"), b"exe").unwrap();
        fs::write(root.join("system/version.info"), "8.2.1-stable-win64\r\n").unwrap();
        fs::write(
            root.join("emulators/retroarch/retroarch.cfg"),
            "savefile_directory = \":\\saves\"\nsavefiles_in_content_dir = \"false\"\nsort_savefiles_enable = \"false\"\n",
        )
        .unwrap();
        fs::write(
            root.join("emulationstation/emulatorLauncher.cfg"),
            "home=.\\.emulationstation\nsaves=.\\..\\saves\n",
        )
        .unwrap();
        fs::write(home.join("es_settings.cfg"), "<config/>\n").unwrap();
        fs::write(
            home.join("es_systems.cfg"),
            format!(
                "<systemList><system><name>gb</name><command>{}</command><emulators><emulator name=\"libretro\"><cores><core>gambatte</core></cores></emulator></emulators></system><system><name>snes</name></system></systemList>",
                stock_gameboy_command()
            ),
        )
        .unwrap();
        fs::write(root.join("emulationstation/es_settings.cfg"), "").unwrap();
        root.to_path_buf()
    }

    fn stock_gameboy_command() -> &'static str {
        r#""%HOME%\emulatorLauncher.exe" -gameinfo %GAMEINFOXML% %CONTROLLERSCONFIG% -system %SYSTEM% -emulator %EMULATOR% -core %CORE% -rom %ROM%"#
    }

    #[test]
    fn verifies_stock_821_gambatte_and_resolves_uncreated_saves_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = stock_fixture(dir.path());
        let profile = resolve_retrobat_gb_profile(&root, &["Tiny Game.gb".into()]).unwrap();
        assert_eq!(profile.version, "8.2.1-stable-win64");
        assert_eq!(profile.effective_saves_root, root.join("saves"));
        assert!(!profile.effective_saves_root.exists());
    }

    #[test]
    fn redirects_and_identity_affecting_overrides_are_resolved_or_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let root = stock_fixture(dir.path());
        fs::create_dir_all(root.join("emulationstation/other-save-root")).unwrap();
        fs::write(
            root.join("emulationstation/emulatorLauncher.cfg"),
            "home=.\\.emulationstation\nsaves=other-save-root\n",
        )
        .unwrap();
        let profile = resolve_retrobat_gb_profile(&root, &[]).unwrap();
        assert_eq!(
            profile.effective_saves_root,
            root.join("emulationstation/other-save-root")
        );

        fs::write(
            root.join("emulationstation/.emulationstation/es_settings.cfg"),
            r#"<config><string name="gb.retroarch.sort_savefiles_enable" value="true"/></config>"#,
        )
        .unwrap();
        assert!(resolve_retrobat_gb_profile(&root, &[]).is_err());
    }

    #[test]
    fn compatible_upgrades_and_unrelated_retroarch_options_remain_supported() {
        let dir = tempfile::tempdir().unwrap();
        let root = stock_fixture(dir.path());
        fs::write(root.join("system/version.info"), "8.3.0").unwrap();
        assert_eq!(
            resolve_retrobat_gb_profile(&root, &[]).unwrap().version,
            "8.3.0"
        );

        fs::write(root.join("system/version.info"), "8.2.1").unwrap();
        fs::write(
            root.join("emulators/retroarch/retroarch.cfg"),
            "video_driver = \"gl\"\nsavefiles_in_content_dir = \"false\"\n",
        )
        .unwrap();
        assert!(resolve_retrobat_gb_profile(&root, &[]).is_ok());
    }

    #[test]
    fn version_markers_are_diagnostics_not_compatibility_gates() {
        let dir = tempfile::tempdir().unwrap();
        let root = stock_fixture(dir.path());
        let version_path = root.join("system/version.info");

        fs::write(&version_path, " \t8.2.1-stable-win64 \r\n").unwrap();
        let shipped = resolve_retrobat_gb_profile(&root, &[]).unwrap();
        assert_eq!(shipped.version, "8.2.1-stable-win64");

        fs::write(&version_path, "8.2.1\r\n").unwrap();
        let legacy = resolve_retrobat_gb_profile(&root, &[]).unwrap();
        assert_eq!(legacy.version, "8.2.1");

        for marker in [
            "8.3.0",
            "8.2.1-beta",
            "8.2.1-stable-linux64",
            "8.2.1-stable-win32",
            "8.2.1-stable-win64-extra",
            "8.2.1-stable-win64-build.1",
            "8.2.1-stable-win64.1",
        ] {
            fs::write(&version_path, marker).unwrap();
            assert!(
                resolve_retrobat_gb_profile(&root, &[]).is_ok(),
                "compatible layout rejected solely for marker {marker:?}"
            );
        }
    }

    #[test]
    fn accepts_retrobat_stock_marker_and_last_system_generated_save_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = stock_fixture(dir.path());
        assert!(resolve_retrobat_gb_profile(&root, &[]).is_ok());

        fs::write(
            root.join("emulators/retroarch/retroarch.cfg"),
            format!(
                "savefile_directory = \"{}\"\nsavefiles_in_content_dir = \"false\"\nsort_savefiles_enable = \"false\"\n",
                root.join("saves/snes").display()
            ),
        )
        .unwrap();
        assert!(resolve_retrobat_gb_profile(&root, &[]).is_ok());
    }

    #[test]
    fn persistent_system_game_and_core_overrides_must_keep_the_gameboy_save_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = stock_fixture(dir.path());
        let settings = root.join("emulationstation/.emulationstation/es_settings.cfg");
        fs::write(
            &settings,
            r#"<config><string name="global.retroarch.savefile_directory" value="D:\foreign"/></config>"#,
        )
        .unwrap();
        assert!(resolve_retrobat_gb_profile(&root, &[]).is_err());

        fs::write(
            &settings,
            r#"<config><string name="gb[&quot;Tiny Game&quot;].retroarch.savefile_directory" value="D:\foreign"/></config>"#,
        )
        .unwrap();
        assert!(resolve_retrobat_gb_profile(&root, &[]).is_err());

        fs::write(&settings, "<config/>\n").unwrap();
        let core_config = root.join("emulators/retroarch/config/Gambatte");
        fs::create_dir_all(&core_config).unwrap();
        fs::write(
            core_config.join("gb.cfg"),
            "savefile_directory = \"D:\\\\foreign\"\n",
        )
        .unwrap();
        assert!(resolve_retrobat_gb_profile(&root, &[]).is_err());
    }

    #[test]
    fn rejects_nonstock_gameboy_executables_and_unaccounted_launcher_flags() {
        let dir = tempfile::tempdir().unwrap();
        let root = stock_fixture(dir.path());
        let systems = root.join("emulationstation/.emulationstation/es_systems.cfg");
        let stock = stock_gameboy_command();
        for command in [
            stock.replace("emulatorLauncher.exe", "evil.exe"),
            stock.replace(" -system", " -saves D:\\other -system"),
            stock.replace(" -system", " -installpath D:\\other -system"),
        ] {
            fs::write(
                &systems,
                format!(
                    "<systemList><system><name>gb</name><command>{command}</command><emulators><emulator name=\"libretro\"><cores><core>gambatte</core></cores></emulator></emulators></system></systemList>"
                ),
            )
            .unwrap();
            assert!(resolve_retrobat_gb_profile(&root, &[]).is_err());
        }
    }

    #[test]
    fn rejects_unresolvable_or_different_retrobat_launcher_roots() {
        let dir = tempfile::tempdir().unwrap();
        let root = stock_fixture(dir.path());
        let launcher_config = root.join("emulationstation/emulatorLauncher.cfg");
        fs::write(
            &launcher_config,
            "home=.\\.emulationstation\nsaves=.\\..\\saves\nretrobat=missing-install\n",
        )
        .unwrap();
        assert!(resolve_retrobat_gb_profile(&root, &[]).is_err());

        let other = dir.path().join("other-retrobat");
        fs::create_dir_all(other.join("system")).unwrap();
        fs::write(other.join("system/version.info"), "8.2.1\n").unwrap();
        fs::write(
            &launcher_config,
            format!(
                "home=.\\.emulationstation\nsaves=.\\..\\saves\nretrobat={}\n",
                other.display()
            ),
        )
        .unwrap();
        assert!(resolve_retrobat_gb_profile(&root, &[]).is_err());
    }

    #[test]
    fn gameboy_command_override_files_are_checked() {
        let dir = tempfile::tempdir().unwrap();
        let root = stock_fixture(dir.path());
        fs::write(
            root.join("emulationstation/.emulationstation/es_systems_custom.cfg"),
            "<systemList><system><name>gb</name><command>\"%HOME%\\emulatorLauncher.exe\" -system %SYSTEM% -emulator %EMULATOR% -core %CORE% -rom %ROM%</command></system></systemList>",
        )
        .unwrap();
        assert!(resolve_retrobat_gb_profile(&root, &[]).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn redirected_gambatte_content_directory_override_is_not_ignored() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let root = stock_fixture(dir.path());
        let config_dir = root.join("emulators/retroarch/config/Gambatte");
        fs::create_dir_all(&config_dir).unwrap();
        let external = dir.path().join("external-gb.cfg");
        fs::write(&external, "savefile_directory = \"D:\\\\foreign\"\n").unwrap();
        symlink(external, config_dir.join("gb.cfg")).unwrap();

        assert!(resolve_retrobat_gb_profile(&root, &[]).is_err());
    }
}
