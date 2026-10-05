use super::mapping::MappingReport;
use super::path::{
    ensure_no_reparse_components, hold_save_directory_chain, is_reparse_point,
    validate_relative_save_path, MAX_SAVE_BYTES,
};
use super::settings::windows_path_key;
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::Path;

pub const MAX_EXISTING_SAVE_SCAN_DEPTH: usize = 8;
pub const MAX_EXISTING_SAVE_SCAN_ENTRIES: usize = 4096;
const MAX_SCAN_DIAGNOSTICS: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExistingSaveScanStatus {
    Complete,
    Partial,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExistingSaveSkipReason {
    RtcRelatedFile,
    UnmappedSram,
    InvalidSramSize,
    OtherFile,
}

impl ExistingSaveSkipReason {
    pub const fn label(self) -> &'static str {
        match self {
            Self::RtcRelatedFile => "RTC-related files",
            Self::UnmappedSram => "unmapped or unsupported .srm files",
            Self::InvalidSramSize => "empty or oversized mapped SRAM files",
            Self::OtherFile => "other files",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExistingSaveSkipCount {
    pub reason: ExistingSaveSkipReason,
    pub files: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExistingSavePreview {
    /// Plain regular `.srm` files at currently mapped targets, counted from
    /// filesystem metadata only.
    pub supported_files: usize,
    /// Ordinary files outside the currently mapped SRAM targets.
    pub skipped_files: usize,
    pub skipped_reasons: Vec<ExistingSaveSkipCount>,
    pub status: ExistingSaveScanStatus,
    /// Bounded explanation of missing roots, skipped redirected paths, errors,
    /// or the scan limit. Partial results must not be presented as complete.
    pub diagnostics: Vec<String>,
}

impl ExistingSavePreview {
    fn new(status: ExistingSaveScanStatus) -> Self {
        Self {
            supported_files: 0,
            skipped_files: 0,
            skipped_reasons: Vec::new(),
            status,
            diagnostics: Vec::new(),
        }
    }

    fn skip_file(&mut self, reason: ExistingSaveSkipReason) {
        self.skipped_files += 1;
        if let Some(count) = self
            .skipped_reasons
            .iter_mut()
            .find(|count| count.reason == reason)
        {
            count.files += 1;
        } else {
            self.skipped_reasons
                .push(ExistingSaveSkipCount { reason, files: 1 });
        }
    }

    fn partial(&mut self, diagnostic: impl Into<String>) {
        if self.status == ExistingSaveScanStatus::Complete {
            self.status = ExistingSaveScanStatus::Partial;
        }
        self.add_diagnostic(diagnostic);
    }

    fn add_diagnostic(&mut self, diagnostic: impl Into<String>) {
        if self.diagnostics.len() < MAX_SCAN_DIAGNOSTICS {
            self.diagnostics.push(diagnostic.into());
        } else if self.diagnostics.len() == MAX_SCAN_DIAGNOSTICS {
            self.diagnostics
                .push("Additional scan issues were omitted.".into());
        }
    }
}

/// Count only files that match the current catalogue mapping. The walk is
/// metadata-only, bounded, and never follows a symlink/reparse point or creates
/// a missing save directory. The caller supplies a profile-verified root.
pub fn preview_existing_saves(root: &Path, mappings: &MappingReport) -> ExistingSavePreview {
    let mapped_targets = mappings
        .mappings
        .iter()
        .filter(|mapping| validate_relative_save_path(&mapping.relative_path).is_ok())
        .map(|mapping| windows_path_key(&mapping.relative_path))
        .collect::<HashSet<_>>();

    let mut preview = ExistingSavePreview::new(ExistingSaveScanStatus::Complete);
    if let Err(error) = ensure_no_reparse_components(root) {
        return unavailable(error.to_string());
    }

    let root_metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            preview.add_diagnostic(
                "The effective saves folder does not exist yet; no files were scanned.",
            );
            return preview;
        }
        Err(error) => return unavailable(format!("Could not inspect the saves folder: {error}")),
    };
    if is_reparse_point(&root_metadata) {
        return unavailable("The effective saves folder is a symbolic link or reparse point.");
    }
    if !root_metadata.is_dir() {
        return unavailable("The effective saves path is not a directory.");
    }

    let mut pending = VecDeque::from([(root.to_path_buf(), 0usize)]);
    let mut inspected_entries = 0usize;
    'walk: while let Some((directory, depth)) = pending.pop_front() {
        let _directory_guard = match hold_save_directory_chain(&directory) {
            Ok(guard) => guard,
            Err(error) => {
                preview.partial(format!(
                    "Could not safely inspect directory {}: {error}",
                    directory.display()
                ));
                continue;
            }
        };
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if directory == root => {
                return unavailable(format!("Could not read the saves folder: {error}"));
            }
            Err(error) => {
                preview.partial(format!(
                    "Could not read directory {}: {error}",
                    directory.display()
                ));
                continue;
            }
        };

        for entry in entries {
            if inspected_entries == MAX_EXISTING_SAVE_SCAN_ENTRIES {
                preview.partial(format!(
                    "The scan reached its {MAX_EXISTING_SAVE_SCAN_ENTRIES}-entry limit."
                ));
                break 'walk;
            }
            inspected_entries += 1;
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    preview.partial(format!("Could not enumerate a saves-folder entry: {error}"));
                    continue;
                }
            };
            let path = entry.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    preview.partial(format!("Could not inspect {}: {error}", path.display()));
                    continue;
                }
            };
            if is_reparse_point(&metadata) {
                preview.partial(format!(
                    "Skipped symbolic link or reparse point: {}",
                    path.display()
                ));
                continue;
            }
            if metadata.is_dir() {
                if path.strip_prefix(root).ok().is_some_and(|relative| {
                    validate_relative_save_path(relative).is_ok()
                        && mapped_targets.contains(&windows_path_key(relative))
                }) {
                    preview.partial(format!(
                        "Mapped save target is a directory, not a regular file: {}",
                        path.display()
                    ));
                }
                if depth == MAX_EXISTING_SAVE_SCAN_DEPTH {
                    preview.partial(format!(
                        "The scan reached its {MAX_EXISTING_SAVE_SCAN_DEPTH}-directory-level limit at {}.",
                        path.display()
                    ));
                } else {
                    pending.push_back((path, depth + 1));
                }
                continue;
            }
            if !metadata.is_file() {
                preview.partial(format!(
                    "Skipped non-regular filesystem entry: {}",
                    path.display()
                ));
                continue;
            }

            let relative = match path.strip_prefix(root) {
                Ok(relative) => relative,
                Err(error) => {
                    preview.partial(format!(
                        "Could not scope {} to the saves folder: {error}",
                        path.display()
                    ));
                    continue;
                }
            };
            let is_mapped_target = validate_relative_save_path(relative).is_ok()
                && mapped_targets.contains(&windows_path_key(relative));
            if is_mapped_target {
                if (1..=MAX_SAVE_BYTES).contains(&metadata.len()) {
                    preview.supported_files += 1;
                } else {
                    preview.skip_file(ExistingSaveSkipReason::InvalidSramSize);
                }
            } else {
                preview.skip_file(skip_reason(&path));
            }
        }
    }
    preview.skipped_reasons.sort_by_key(|count| count.reason);
    preview
}

fn unavailable(diagnostic: impl Into<String>) -> ExistingSavePreview {
    let mut preview = ExistingSavePreview::new(ExistingSaveScanStatus::Unavailable);
    preview.add_diagnostic(diagnostic);
    preview
}

fn skip_reason(path: &Path) -> ExistingSaveSkipReason {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if extension.eq_ignore_ascii_case("rtc") => {
            ExistingSaveSkipReason::RtcRelatedFile
        }
        Some(extension) if extension.eq_ignore_ascii_case("srm") => {
            if fs::symlink_metadata(path.with_extension("rtc"))
                .is_ok_and(|metadata| metadata.is_file() && !is_reparse_point(&metadata))
            {
                ExistingSaveSkipReason::RtcRelatedFile
            } else {
                ExistingSaveSkipReason::UnmappedSram
            }
        }
        _ => ExistingSaveSkipReason::OtherFile,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::RomKey;
    use crate::romm::{PlatformDto, RomDto, RomFileDto};
    use crate::save_sync::{map_catalogue, MappingReport, SaveMapping, SaveProfile};
    use std::fs::File;
    use std::path::PathBuf;

    fn mapping_report(root: &Path) -> MappingReport {
        let platforms = [
            PlatformDto {
                id: 1,
                slug: "gb".into(),
                fs_slug: "gb".into(),
                name: "Game Boy".into(),
                custom_name: None,
                rom_count: 3,
            },
            PlatformDto {
                id: 2,
                slug: "snes".into(),
                fs_slug: "snes".into(),
                name: "Super Nintendo".into(),
                custom_name: None,
                rom_count: 1,
            },
        ];
        let roms = ["Solo.gb", "Empty.gb", "Large.gb", "Bundle.gb"]
            .into_iter()
            .enumerate()
            .map(|(index, name)| RomDto {
                id: index as i64 + 1,
                platform_fs_slug: "gb".into(),
                platform_slug: "gb".into(),
                fs_name: name.into(),
                fs_size_bytes: 1,
                has_simple_single_file: true,
                has_nested_single_file: false,
                has_multiple_files: false,
                missing_from_fs: false,
                is_physical: false,
                updated_at: String::new(),
                files: vec![RomFileDto {
                    id: index as i64 + 10,
                    file_name: name.into(),
                    file_size_bytes: 1,
                    last_modified: None,
                    crc_hash: None,
                    md5_hash: None,
                    sha1_hash: None,
                    is_top_level: true,
                }],
            })
            .collect::<Vec<_>>();
        let mut catalogue =
            crate::catalog::build_catalogue("server", &platforms, &roms, |_| {}).unwrap();
        catalogue.entries.push(crate::catalog::RomEntry {
            key: RomKey {
                server_id: "server".into(),
                rom_id: 10,
                file_id: 20,
            },
            platform_dir: "snes".into(),
            file_name: "Other.sfc".into(),
            size: 1,
            content_name: "Other.sfc".into(),
            version: None,
        });
        map_catalogue(&catalogue, root).unwrap()
    }

    fn mapping_for(root: &Path) -> MappingReport {
        let mapping = SaveMapping {
            rom_key: RomKey {
                server_id: "server".into(),
                rom_id: 1,
                file_id: 10,
            },
            profile: SaveProfile {
                id: super::super::mapping::RETROBAT_GB_SRM_PROFILE,
                system_dir: "gb",
                rom_extension: ".gb",
                save_extension: ".srm",
            },
            relative_path: PathBuf::from("gb/Game.srm"),
            target_path: root.join("gb/Game.srm"),
            visible_rom_name: "Game.gb".into(),
        };
        MappingReport {
            mappings: vec![mapping],
            ..MappingReport::default()
        }
    }

    #[test]
    fn inventory_counts_only_mapped_sram_and_reports_other_file_reasons() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("saves");
        fs::create_dir_all(root.join("gb")).unwrap();
        fs::create_dir_all(root.join("snes")).unwrap();
        fs::write(root.join("gb/Solo.srm"), b"supported save").unwrap();
        fs::write(root.join("gb/orphan.srm"), b"orphan save").unwrap();
        fs::write(root.join("gb/Empty.srm"), b"").unwrap();
        fs::write(
            root.join("gb/Large.srm"),
            vec![b'x'; MAX_SAVE_BYTES as usize + 1],
        )
        .unwrap();
        fs::write(root.join("gb/Bundle.srm"), b"bundle save").unwrap();
        fs::write(root.join("gb/Bundle.rtc"), b"rtc sidecar").unwrap();
        fs::write(root.join("gb/Solo.state"), b"state data").unwrap();
        fs::write(root.join("snes/Other.srm"), b"other platform").unwrap();

        let report = mapping_report(&root);
        assert_eq!(report.supported_count(), 3);
        assert_eq!(
            report.unmapped.len(),
            2,
            "RTC-bundled and SNES games are unmapped"
        );
        let unreadable_save = root.join("gb/Solo.srm");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&unreadable_save, fs::Permissions::from_mode(0o000)).unwrap();
        }
        let unreadable = File::open(&unreadable_save).is_err();

        let preview = preview_existing_saves(&root, &report);

        assert_eq!(preview.status, ExistingSaveScanStatus::Complete);
        assert_eq!(preview.supported_files, 1);
        assert_eq!(preview.skipped_files, 7);
        assert!(preview.skipped_reasons.iter().any(|count| {
            count.reason == ExistingSaveSkipReason::InvalidSramSize && count.files == 2
        }));
        assert!(preview.skipped_reasons.iter().any(|count| {
            count.reason == ExistingSaveSkipReason::RtcRelatedFile && count.files == 2
        }));
        assert!(preview.skipped_reasons.iter().any(|count| {
            count.reason == ExistingSaveSkipReason::UnmappedSram && count.files == 2
        }));
        assert!(preview.skipped_reasons.iter().any(|count| {
            count.reason == ExistingSaveSkipReason::OtherFile && count.files == 1
        }));
        if unreadable {
            assert_eq!(
                preview.supported_files, 1,
                "the preview must not open save contents"
            );
        }
    }

    #[test]
    fn missing_root_is_a_complete_empty_preview_and_is_not_created() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("missing-saves");

        let preview = preview_existing_saves(&root, &mapping_for(&root));

        assert_eq!(preview.status, ExistingSaveScanStatus::Complete);
        assert_eq!(preview.supported_files, 0);
        assert_eq!(preview.skipped_files, 0);
        assert!(!root.exists());
        assert!(preview.diagnostics[0].contains("does not exist yet"));
    }

    #[test]
    fn invalid_root_reports_unavailable_instead_of_a_complete_zero() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("not-a-directory");
        fs::write(&root, b"not a directory").unwrap();

        let preview = preview_existing_saves(&root, &mapping_for(&root));

        assert_eq!(preview.status, ExistingSaveScanStatus::Unavailable);
        assert_eq!(preview.supported_files, 0);
        assert!(preview.diagnostics[0].contains("not a directory"));
    }

    #[test]
    fn directory_depth_limit_marks_the_inventory_partial() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("saves");
        let mut deep = root.clone();
        for index in 0..=MAX_EXISTING_SAVE_SCAN_DEPTH {
            deep.push(format!("level-{index}"));
        }
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("hidden.srm"), b"not scanned").unwrap();

        let preview = preview_existing_saves(&root, &mapping_for(&root));

        assert_eq!(preview.status, ExistingSaveScanStatus::Partial);
        assert!(preview.diagnostics[0].contains("directory-level limit"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_entries_are_not_followed_and_make_the_preview_partial() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("saves");
        let external = temp.path().join("outside");
        fs::create_dir_all(root.join("gb")).unwrap();
        fs::create_dir_all(&external).unwrap();
        fs::write(external.join("hidden.srm"), b"outside save").unwrap();
        symlink(&external, root.join("gb/redirected")).unwrap();
        fs::write(root.join("gb/Game.srm"), b"local save").unwrap();

        let preview = preview_existing_saves(&root, &mapping_for(&root));

        assert_eq!(preview.status, ExistingSaveScanStatus::Partial);
        assert_eq!(preview.supported_files, 1);
        assert_eq!(
            preview.skipped_files, 0,
            "the linked directory is not traversed"
        );
        assert!(!preview.diagnostics.is_empty());

        let linked_root = temp.path().join("linked-saves");
        symlink(&external, &linked_root).unwrap();
        let linked_preview = preview_existing_saves(&linked_root, &mapping_for(&linked_root));
        assert_eq!(linked_preview.status, ExistingSaveScanStatus::Unavailable);
        assert_eq!(linked_preview.supported_files, 0);
        assert!(linked_preview.diagnostics[0].contains("symbolic link or reparse point"));
    }
}
