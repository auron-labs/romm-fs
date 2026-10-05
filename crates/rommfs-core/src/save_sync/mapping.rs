use crate::catalog::{Catalogue, RomEntry, RomKey};
use crate::error::{Error, Result};
use crate::sanitize::sanitize_component;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Stable app-owned slot identity. It deliberately does not encode a RomM
/// route or imply that an effective save root was resolved at runtime.
pub const RETROBAT_GB_SRM_PROFILE: &str = "rommfs-retrobat-gb-srm-v1";

const GAMEBOY_PLATFORM: &str = "gb";
const GAMEBOY_ROM_EXTENSION: &str = ".gb";
const GAMEBOY_SAVE_EXTENSION: &str = ".srm";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveProfile {
    pub id: &'static str,
    pub system_dir: &'static str,
    pub rom_extension: &'static str,
    pub save_extension: &'static str,
}

impl SaveProfile {
    pub(super) fn gameboy() -> Self {
        Self {
            id: RETROBAT_GB_SRM_PROFILE,
            system_dir: GAMEBOY_PLATFORM,
            rom_extension: GAMEBOY_ROM_EXTENSION,
            save_extension: GAMEBOY_SAVE_EXTENSION,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveMapping {
    pub rom_key: RomKey,
    pub profile: SaveProfile,
    /// Relative to the currently selected effective RetroBat saves root.
    pub relative_path: PathBuf,
    /// Exact target for preview. It is not created by mapping.
    pub target_path: PathBuf,
    pub visible_rom_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MappingIssue {
    UnsupportedPlatform,
    UnsupportedRomExtension,
    UnsafeTargetName,
    UnsafeTargetPath,
    AmbiguousAlias,
    UnsupportedCompanionSave,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnmappedEntry {
    pub rom_key: RomKey,
    pub visible_rom_name: String,
    pub issue: MappingIssue,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MappingReport {
    pub mappings: Vec<SaveMapping>,
    pub unmapped: Vec<UnmappedEntry>,
    /// Catalogue entries omitted before mapping (e.g. multi-file ROMs).
    pub catalogue_skipped: usize,
}

impl MappingReport {
    pub fn supported_count(&self) -> usize {
        self.mappings.len()
    }

    pub fn unmapped_count(&self) -> usize {
        self.unmapped.len() + self.catalogue_skipped
    }
}

/// Map only already-visible catalogue entries. The source-backed profile is
/// intentionally narrow: `gb/*.gb` maps to `gb/<visible stem>.srm`; paired
/// Gambatte RTC sidecars and `.gbc` content are excluded from this increment.
pub fn map_catalogue(catalogue: &Catalogue, effective_saves_root: &Path) -> Result<MappingReport> {
    let profile = SaveProfile::gameboy();
    let mut report = MappingReport {
        catalogue_skipped: catalogue.skipped_unsupported,
        ..MappingReport::default()
    };
    let mut candidates = Vec::new();

    for entry in &catalogue.entries {
        if !entry.platform_dir.eq_ignore_ascii_case(profile.system_dir) {
            report
                .unmapped
                .push(unmapped(entry, MappingIssue::UnsupportedPlatform));
            continue;
        }
        let Some(stem) = strip_extension(&entry.file_name, profile.rom_extension) else {
            report
                .unmapped
                .push(unmapped(entry, MappingIssue::UnsupportedRomExtension));
            continue;
        };
        let file_name = format!("{stem}{}", profile.save_extension);
        let relative_path = PathBuf::from(profile.system_dir).join(file_name);
        if super::path::validate_relative_save_path(&relative_path).is_err() {
            report
                .unmapped
                .push(unmapped(entry, MappingIssue::UnsafeTargetName));
            continue;
        }
        let target_path =
            match super::path::resolve_save_target(effective_saves_root, &relative_path) {
                Ok(path) => path,
                Err(_) => {
                    report
                        .unmapped
                        .push(unmapped(entry, MappingIssue::UnsafeTargetPath));
                    continue;
                }
            };
        let has_rtc_companion =
            match super::path::has_rtc_companion(effective_saves_root, &relative_path) {
                Ok(has_companion) => has_companion,
                Err(_) => {
                    report
                        .unmapped
                        .push(unmapped(entry, MappingIssue::UnsafeTargetPath));
                    continue;
                }
            };
        candidates.push((
            entry,
            relative_path,
            target_path,
            alias_stems(entry),
            has_rtc_companion,
        ));
    }

    let mut alias_owners: HashMap<String, HashSet<usize>> = HashMap::new();
    for (index, (entry, _, _, aliases, _)) in candidates.iter().enumerate() {
        for alias in aliases {
            alias_owners.entry(alias.clone()).or_default().insert(index);
        }
        // Visible stems always participate, even if remote source names were
        // malformed and therefore cannot be normalized as aliases.
        if let Some(stem) = strip_extension(&entry.file_name, profile.rom_extension) {
            alias_owners
                .entry(stem.to_lowercase())
                .or_default()
                .insert(index);
        }
    }
    let ambiguous: HashSet<usize> = alias_owners
        .values()
        .filter(|owners| owners.len() > 1)
        .flat_map(|owners| owners.iter().copied())
        .collect();

    for (index, (entry, relative_path, target_path, _, has_rtc_companion)) in
        candidates.into_iter().enumerate()
    {
        if ambiguous.contains(&index) {
            report
                .unmapped
                .push(unmapped(entry, MappingIssue::AmbiguousAlias));
            continue;
        }
        if has_rtc_companion {
            report
                .unmapped
                .push(unmapped(entry, MappingIssue::UnsupportedCompanionSave));
            continue;
        }
        report.mappings.push(SaveMapping {
            rom_key: entry.key.clone(),
            profile: profile.clone(),
            target_path,
            relative_path,
            visible_rom_name: entry.file_name.clone(),
        });
    }
    report
        .mappings
        .sort_by_key(|mapping| mapping.rom_key.rom_id);
    report.unmapped.sort_by_key(|entry| entry.rom_key.rom_id);
    Ok(report)
}

fn unmapped(entry: &RomEntry, issue: MappingIssue) -> UnmappedEntry {
    UnmappedEntry {
        rom_key: entry.key.clone(),
        visible_rom_name: entry.file_name.clone(),
        issue,
    }
}

fn strip_extension<'a>(name: &'a str, extension: &str) -> Option<&'a str> {
    let (stem, found) = name.rsplit_once('.')?;
    (found.eq_ignore_ascii_case(extension.trim_start_matches('.')) && !stem.is_empty())
        .then_some(stem)
}

fn alias_stems(entry: &RomEntry) -> HashSet<String> {
    let mut aliases = HashSet::new();
    if let Some(clean) = sanitize_component(&entry.content_name) {
        if let Some(stem) = strip_extension(clean.name(), GAMEBOY_ROM_EXTENSION) {
            aliases.insert(stem.to_lowercase());
        }
    }
    aliases
}

/// Validate a user-visible mapping selection against a currently planned
/// target. `SaveSyncJournal` uses this before persisting any relative path.
pub(crate) fn validate_mapping_path(path: &Path) -> Result<()> {
    super::path::validate_relative_save_path(path).map_err(Error::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::build_catalogue;
    use crate::romm::{PlatformDto, RomDto, RomFileDto};

    fn platform(slug: &str) -> PlatformDto {
        PlatformDto {
            id: 1,
            slug: slug.into(),
            fs_slug: slug.into(),
            name: slug.into(),
            custom_name: None,
            rom_count: 1,
        }
    }

    fn rom(id: i64, platform: &str, file_name: &str) -> RomDto {
        RomDto {
            id,
            platform_fs_slug: platform.into(),
            platform_slug: platform.into(),
            fs_name: file_name.into(),
            fs_size_bytes: 16,
            has_simple_single_file: true,
            has_nested_single_file: false,
            has_multiple_files: false,
            missing_from_fs: false,
            is_physical: false,
            updated_at: String::new(),
            files: vec![RomFileDto {
                id: id + 100,
                file_name: file_name.into(),
                file_size_bytes: 16,
                last_modified: None,
                crc_hash: None,
                md5_hash: None,
                sha1_hash: None,
                is_top_level: true,
            }],
        }
    }

    fn catalogue(roms: Vec<RomDto>) -> Catalogue {
        build_catalogue("server", &[platform("gb"), platform("snes")], &roms, |_| {}).unwrap()
    }

    #[test]
    fn maps_only_supported_visible_entries_and_keeps_catalogue_suffixes() {
        let catalogue = catalogue(vec![
            rom(1, "gb", "Zelda.gb"),
            rom(2, "gb", "zelda (2).GB"),
            rom(3, "snes", "Mario.sfc"),
            rom(4, "gb", "Other.gbc"),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("not-created-saves-root");

        let report = map_catalogue(&catalogue, &root).unwrap();

        assert_eq!(report.supported_count(), 2);
        assert_eq!(report.unmapped_count(), 2);
        let first = report
            .mappings
            .iter()
            .find(|mapping| mapping.rom_key.rom_id == 1)
            .unwrap();
        assert_eq!(first.visible_rom_name, "Zelda.gb");
        assert_eq!(first.relative_path, PathBuf::from("gb/Zelda.srm"));
        assert_eq!(first.target_path, root.join("gb/Zelda.srm"));
        assert!(!root.exists());
        let suffixed = report
            .mappings
            .iter()
            .find(|mapping| mapping.rom_key.rom_id == 2)
            .unwrap();
        assert_eq!(suffixed.visible_rom_name, "zelda (2).GB");
        assert_eq!(suffixed.relative_path, PathBuf::from("gb/zelda (2).srm"));
        assert_eq!(
            report
                .unmapped
                .iter()
                .find(|entry| entry.rom_key.rom_id == 4)
                .unwrap()
                .issue,
            MappingIssue::UnsupportedRomExtension
        );
    }

    #[test]
    fn duplicate_case_insensitive_and_sanitized_source_aliases_are_ambiguous() {
        let catalogue = catalogue(vec![
            rom(1, "gb", "Same.gb"),
            rom(2, "gb", "same.gb"),
            rom(3, "gb", "Alias?.gb"),
            rom(4, "gb", "Alias*.gb"),
        ]);

        let report = map_catalogue(&catalogue, Path::new("saves")).unwrap();

        assert!(report.mappings.is_empty());
        assert_eq!(report.unmapped_count(), 4);
        assert!(report
            .unmapped
            .iter()
            .all(|entry| entry.issue == MappingIssue::AmbiguousAlias));
    }

    #[test]
    fn sanitized_filename_mappings_keep_visible_not_remote_stem() {
        let catalogue = catalogue(vec![rom(1, "gb", "A:B.gb")]);

        let report = map_catalogue(&catalogue, Path::new("saves")).unwrap();

        assert_eq!(report.mappings[0].visible_rom_name, "A_B.gb");
        assert_eq!(
            report.mappings[0].relative_path,
            PathBuf::from("gb/A_B.srm")
        );
    }

    #[test]
    fn gambatte_rtc_sidecar_makes_the_sram_mapping_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let saves = dir.path().join("saves");
        std::fs::create_dir_all(saves.join("gb")).unwrap();
        std::fs::write(saves.join("gb/Game.rtc"), b"RTC data").unwrap();
        let catalogue = catalogue(vec![rom(1, "gb", "Game.gb")]);

        let report = map_catalogue(&catalogue, &saves).unwrap();

        assert!(report.mappings.is_empty());
        assert_eq!(report.unmapped.len(), 1);
        assert_eq!(
            report.unmapped[0].issue,
            MappingIssue::UnsupportedCompanionSave
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_system_directory_is_not_previewed_as_a_save_target() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let saves = dir.path().join("saves");
        let external = dir.path().join("external");
        std::fs::create_dir_all(&saves).unwrap();
        std::fs::create_dir_all(&external).unwrap();
        symlink(&external, saves.join("gb")).unwrap();
        let catalogue = catalogue(vec![rom(1, "gb", "Game.gb")]);

        let report = map_catalogue(&catalogue, &saves).unwrap();

        assert!(report.mappings.is_empty());
        assert_eq!(report.unmapped[0].issue, MappingIssue::UnsafeTargetPath);
    }
}
