//! Source-backed RetroBat folder mappings, checked against installed systems.
use crate::romm::{RomDto, RomFileDto};
use crate::{Error, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Deserialize)]
struct SystemList {
    #[serde(rename = "system", default)]
    systems: Vec<System>,
}
#[derive(Deserialize)]
struct System {
    name: String,
    path: String,
    #[serde(default)]
    platform: String,
    #[serde(default)]
    extension: String,
}

/// A validated snapshot of the frontend's supported folders and extensions.
pub struct RetroBatSystems {
    systems: HashMap<String, System>,
}
impl RetroBatSystems {
    pub fn from_xml(xml: &str) -> Result<Self> {
        let list: SystemList = quick_xml::de::from_str(xml)
            .map_err(|e| Error::Unsupported(format!("RetroBat systems configuration: {e}")))?;
        let mut systems = HashMap::new();
        for system in list.systems {
            crate::save_sync::validate_windows_path_component(&system.name)
                .map_err(Error::Unsupported)?;
            let normalized = system.path.replace('\\', "/");
            // Redirected paths would defeat the managed link.
            if normalized != format!("~/../roms/{}", system.name) {
                continue;
            }
            let name = system.name.to_lowercase();
            if systems.insert(name, system).is_some() {
                return Err(Error::Unsupported("duplicate RetroBat system name".into()));
            }
        }
        if systems.is_empty() {
            return Err(Error::Unsupported(
                "no supported RetroBat roms folders in systems configuration".into(),
            ));
        }
        Ok(Self { systems })
    }

    pub fn from_installation(root: &Path) -> Result<Self> {
        crate::save_sync::ensure_no_reparse_components(root)?;
        let path = root.join("emulationstation/.emulationstation/es_systems.cfg");
        crate::save_sync::ensure_no_reparse_components(&path)?;
        Self::from_xml(&std::fs::read_to_string(&path)?)
    }

    pub fn folder(&self, slug: &str, fs_slug: &str) -> Option<&str> {
        let alias = match slug {
            "gb" | "game-boy" => "gb",
            "gbc" | "game-boy-color" => "gbc",
            "gba" | "game-boy-advance" => "gba",
            "genesis" => "megadrive",
            "segacd" | "sega-cd" => "megacd",
            "ps" | "playstation" => "psx",
            "ngc" | "gc" => "gamecube",
            "sms" => "mastersystem",
            "32x" => "sega32x",
            "tg16" => "pcengine",
            "turbografx-cd" => "pcenginecd",
            _ => slug,
        };
        for key in [alias, fs_slug] {
            if let Some(system) = self.systems.get(&key.to_lowercase()) {
                return Some(&system.name);
            }
        }
        let mut candidates = self
            .systems
            .values()
            .filter(|s| s.platform.split(',').any(|p| p.trim() == slug));
        let candidate = candidates.next()?;
        candidates
            .next()
            .is_none()
            .then_some(candidate.name.as_str())
    }

    pub fn supports_file(&self, folder: &str, filename: &str) -> bool {
        let Some(system) = self.systems.get(&folder.to_lowercase()) else {
            return false;
        };
        let Some((_, extension)) = filename.rsplit_once('.') else {
            return false;
        };
        system
            .extension
            .split_whitespace()
            .any(|e| e.eq_ignore_ascii_case(&format!(".{extension}")))
    }
}

/// file_path is a library-relative directory; fs_path is the platform directory.
/// Old flat catalogues may omit paths. Nested records must provide them.
pub(crate) fn relative_file_path(rom: &RomDto, file: &RomFileDto) -> Option<String> {
    let directory = if file.file_path.is_empty() {
        if !file.is_top_level {
            return None;
        }
        ""
    } else {
        if !valid_source_directory(&file.file_path) || !valid_source_directory(&rom.fs_path) {
            return None;
        }
        if file.file_path == rom.fs_path {
            ""
        } else {
            file.file_path.strip_prefix(&format!("{}/", rom.fs_path))?
        }
    };
    Some(if directory.is_empty() {
        file.file_name.clone()
    } else {
        format!("{directory}/{}", file.file_name)
    })
}
fn valid_source_directory(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && path.split('/').all(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && !p.contains(':')
                && !p.chars().any(char::is_control)
        })
}
