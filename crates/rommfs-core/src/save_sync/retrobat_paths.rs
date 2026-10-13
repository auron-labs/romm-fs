use super::settings::windows_path_key;
use crate::error::{Error, Result};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub(super) const MAX_PROFILE_CONFIG_BYTES: u64 = 1024 * 1024;

/// Resolve the launcher-owned paths independently of emulator save formats.
pub(crate) fn resolve_retrobat_paths(installation_root: &Path) -> Result<(PathBuf, PathBuf)> {
    super::path::ensure_no_reparse_components(installation_root)?;
    let launcher_dir = installation_root.join("emulationstation");
    let launcher_config = read_key_values(&launcher_dir.join("emulatorLauncher.cfg"))?;
    let local_path = configured_install_path(&launcher_config, installation_root, &launcher_dir);
    let root = resolve_launcher_path(
        lookup(&launcher_config, "retrobat"),
        &local_path,
        installation_root,
        true,
    )?;
    if windows_path_key(&root) != windows_path_key(installation_root) {
        return Err(Error::Unsupported(
            "RetroBat launcher root differs from the selected installation".into(),
        ));
    }
    let home = resolve_launcher_path(
        lookup(&launcher_config, "home"),
        &local_path,
        &local_path.join(".emulationstation"),
        false,
    )?;
    let saves = resolve_launcher_path(
        lookup(&launcher_config, "saves"),
        &local_path,
        &local_path.parent().unwrap_or(&local_path).join("saves"),
        false,
    )?;
    super::path::ensure_no_reparse_components(&home)?;
    super::path::ensure_no_reparse_components(&saves)?;
    Ok((home, saves))
}

pub(crate) fn resolve_auxiliary_root(installation: &Path, key: &str) -> Result<PathBuf> {
    resolve_retrobat_paths(installation)?;
    let launcher = installation.join("emulationstation");
    let config = read_key_values(&launcher.join("emulatorLauncher.cfg"))?;
    let local = configured_install_path(&config, installation, &launcher);
    let path = resolve_launcher_path(
        lookup(&config, key),
        &local,
        &local.parent().unwrap_or(&local).join(key),
        false,
    )?;
    super::path::ensure_no_reparse_components(&path)?;
    Ok(path)
}

pub(super) fn configured_install_path(
    config: &[(String, String)],
    install_root: &Path,
    launcher_dir: &Path,
) -> PathBuf {
    let Some(value) = lookup(config, "installpath") else {
        return launcher_dir.to_path_buf();
    };
    let candidate = PathBuf::from(value);
    let candidate = if candidate.is_absolute() || is_windows_absolute(value) {
        candidate
    } else {
        install_root.join(candidate)
    };
    if candidate.is_dir() {
        candidate
    } else {
        launcher_dir.to_path_buf()
    }
}

/// Mirrors the pinned launcher's relative search order, but resolves the first
/// candidate even when a documented saves directory has not been created yet.
pub(super) fn resolve_launcher_path(
    configured: Option<&String>,
    local_path: &Path,
    default: &Path,
    use_retrobat_default: bool,
) -> Result<PathBuf> {
    let Some(value) = configured.filter(|value| !value.trim().is_empty()) else {
        return Ok(default.to_path_buf());
    };
    let value = value.trim();
    if Path::new(value).is_absolute() || is_windows_absolute(value) {
        return Ok(PathBuf::from(value));
    }
    let relative = PathBuf::from(value.replace('\\', "/"));
    let candidates = [
        local_path.join(&relative),
        local_path
            .parent()
            .unwrap_or(local_path)
            .join("emulators")
            .join(&relative),
        local_path
            .parent()
            .unwrap_or(local_path)
            .join("system/emulators")
            .join(&relative),
        local_path
            .parent()
            .unwrap_or(local_path)
            .join("system")
            .join(&relative),
    ];
    if let Some(existing) = candidates.iter().find(|candidate| candidate.is_dir()) {
        return Ok(lexical_normalize(existing));
    }
    if use_retrobat_default {
        return Err(Error::Unsupported(format!(
            "RetroBat launcher path override {value:?} does not resolve to an existing installation"
        )));
    }
    // RetroBat's stock `saves=..\\saves` is a documented sibling even on a
    // fresh install; preserve the first launcher-local resolution without
    // making it.
    Ok(lexical_normalize(&candidates[0]))
}

pub(super) fn is_windows_absolute(value: &str) -> bool {
    let bytes = value.as_bytes();
    (bytes.len() >= 3 && bytes[1] == b':' && matches!(bytes[2], b'/' | b'\\'))
        || value.starts_with("\\\\")
}

pub(super) fn lexical_normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}

pub(super) fn read_key_values(path: &Path) -> Result<Vec<(String, String)>> {
    let contents = read_limited_text(path)?;
    Ok(contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            Some((key.trim().to_ascii_lowercase(), unquote(value.trim())))
        })
        .collect())
}

pub(super) fn read_limited_text(path: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || super::path::is_reparse_point(&metadata) {
        return Err(Error::Unsupported(format!(
            "RetroBat config is not a regular file: {}",
            path.display()
        )));
    }
    if metadata.len() > MAX_PROFILE_CONFIG_BYTES {
        return Err(Error::Unsupported(format!(
            "RetroBat config is larger than the {MAX_PROFILE_CONFIG_BYTES}-byte inspection limit"
        )));
    }
    fs::read_to_string(path).map_err(Error::Io)
}

pub(super) fn unquote(value: &str) -> String {
    value.trim_matches('"').to_owned()
}

pub(super) fn lookup<'a>(config: &'a [(String, String)], key: &str) -> Option<&'a String> {
    config
        .iter()
        .rev()
        .find_map(|(name, value)| (name == key).then_some(value))
}
