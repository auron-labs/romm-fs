use super::settings::windows_path_key;
use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

const RETROBAT_EXECUTABLE: &str = "RetroBat.exe";
const EMULATIONSTATION_DIRECTORY: &str = "emulationstation";
const EMULATIONSTATION_SYSTEMS: &str = "es_systems.cfg";
const RETROARCH_DIRECTORY: &str = "emulators/retroarch";
const RETROARCH_CONFIG: &str = "emulators/retroarch/retroarch.cfg";
const DEFAULT_SAVES_DIRECTORY: &str = "saves";
const MAX_CONFIG_SEARCH_DEPTH: usize = 2;
const MAX_CONFIG_SEARCH_ENTRIES: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum InstallationSource {
    FixedDrive,
    RemovableDrive,
    RetroBatProcess,
    EmulationStationProcess,
}

#[derive(Clone, Debug, Default)]
pub struct DiscoveryInput {
    /// Fixed/removable-drive candidates already restricted to `X:\RetroBat`
    /// by the platform helper.
    pub drive_roots: Vec<(PathBuf, InstallationSource)>,
    /// Full image paths returned by a read-only process query.
    pub process_images: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct InstallationCandidate {
    pub info: InstallationInfo,
    pub sources: Vec<InstallationSource>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallationInfo {
    pub install_root: PathBuf,
    /// RetroBat's documented top-level save directory. This is a read-only
    /// discovery result, not a claim that every emulator writes here.
    pub documented_saves_root: PathBuf,
    pub retroarch_config: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallationProblem {
    Missing,
    Inaccessible,
    Invalid(&'static str),
}

#[derive(Clone, Debug, Default)]
pub struct DiscoveryReport {
    /// Valid installs sorted by Windows path identity. Invalid/inaccessible
    /// paths are deliberately misses and contribute only to `skipped`.
    pub candidates: Vec<InstallationCandidate>,
    pub skipped: usize,
}

/// Validate only by reading documented install markers; never create, alter,
/// or clean an installation directory.
pub fn validate_installation(
    root: impl AsRef<Path>,
) -> Result<InstallationInfo, InstallationProblem> {
    let root = root.as_ref();
    let root_meta = fs::metadata(root).map_err(classify_io)?;
    if !root_meta.is_dir() {
        return Err(InstallationProblem::Invalid(
            "selected path is not a directory",
        ));
    }

    require_file(&root.join(RETROBAT_EXECUTABLE), "RetroBat.exe is missing")?;
    find_emulationstation_config(&root.join(EMULATIONSTATION_DIRECTORY))?;

    let retroarch_directory = root.join(RETROARCH_DIRECTORY);
    let retroarch_meta = fs::metadata(&retroarch_directory).map_err(classify_io)?;
    if !retroarch_meta.is_dir() {
        return Err(InstallationProblem::Invalid(
            "RetroArch installation directory is missing",
        ));
    }
    let retroarch_config = root.join(RETROARCH_CONFIG);
    let _config = fs::read(&retroarch_config).map_err(classify_io)?;

    let documented_saves_root = root.join(DEFAULT_SAVES_DIRECTORY);
    match fs::metadata(&documented_saves_root) {
        Ok(metadata) if !metadata.is_dir() => {
            return Err(InstallationProblem::Invalid(
                "RetroBat saves path is not a directory",
            ));
        }
        Ok(_) => {}
        // A fresh installation may not have created its saves folder yet.
        // Keep the documented path as a candidate without creating it.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(classify_io(error)),
    }

    Ok(InstallationInfo {
        install_root: root.to_path_buf(),
        documented_saves_root,
        retroarch_config,
    })
}

/// Search only a small bounded area beneath `emulationstation` until an exact
/// config location is verified. Never descend through symlinks or reparse
/// points, and cap both depth and total entries to avoid unbounded traversal.
pub(super) fn find_emulationstation_config(root: &Path) -> Result<PathBuf, InstallationProblem> {
    let metadata = fs::symlink_metadata(root).map_err(classify_io)?;
    if !metadata.is_dir() || is_reparse_point(&metadata) {
        return Err(InstallationProblem::Invalid(
            "EmulationStation folder is missing or redirected",
        ));
    }
    let mut pending = VecDeque::from([(root.to_path_buf(), 0usize)]);
    let mut scanned_entries = 0usize;
    while let Some((directory, depth)) = pending.pop_front() {
        let entries = fs::read_dir(directory).map_err(classify_io)?;
        for entry in entries {
            if scanned_entries == MAX_CONFIG_SEARCH_ENTRIES {
                return Err(InstallationProblem::Invalid(
                    "EmulationStation config search exceeded its safety bound",
                ));
            }
            scanned_entries += 1;
            let entry = entry.map_err(classify_io)?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(classify_io)?;
            if is_reparse_point(&metadata) {
                continue;
            }
            if metadata.is_dir() {
                if depth < MAX_CONFIG_SEARCH_DEPTH {
                    pending.push_back((path, depth + 1));
                }
            } else if metadata.is_file()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(EMULATIONSTATION_SYSTEMS)
            {
                let contents = fs::read_to_string(&path).map_err(classify_io)?;
                if contents.contains("<systemList") {
                    return Ok(path);
                }
            }
        }
    }
    Err(InstallationProblem::Invalid(
        "EmulationStation es_systems.cfg was not found",
    ))
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

/// Convert a running RetroBat executable path to its documented install root.
/// EmulationStation's executables reside under the documented `emulationstation`
/// directory; unexpected layouts are ignored instead of walked speculatively.
pub fn installation_root_from_process_image(image: &Path) -> Option<(PathBuf, InstallationSource)> {
    let image_text = image.to_string_lossy().replace('\\', "/");
    let segments: Vec<&str> = image_text
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let executable = segments.last()?;
    let source = if executable.eq_ignore_ascii_case("retrobat.exe") {
        InstallationSource::RetroBatProcess
    } else if executable.eq_ignore_ascii_case("emulationstation.exe") {
        InstallationSource::EmulationStationProcess
    } else {
        return None;
    };

    let mut root_segments = segments[..segments.len() - 1].to_vec();
    if source == InstallationSource::EmulationStationProcess {
        let emulationstation_dir = root_segments
            .iter()
            .rposition(|directory| directory.eq_ignore_ascii_case("emulationstation"))?;
        root_segments.truncate(emulationstation_dir);
    }
    if root_segments.is_empty() {
        return None;
    }
    let separator = if image.to_string_lossy().contains('\\') {
        "\\"
    } else {
        "/"
    };
    let prefix = if image_text.starts_with("//") {
        "//"
    } else if image_text.starts_with('/') {
        "/"
    } else {
        ""
    };
    Some((
        PathBuf::from(format!("{prefix}{}", root_segments.join(separator))),
        source,
    ))
}

/// Deterministic portable candidate pipeline. Drive paths and process image
/// paths are supplied by a narrow platform helper, so the rules are testable
/// without enumerating host processes or volumes.
pub fn discover_installations(input: DiscoveryInput) -> DiscoveryReport {
    let mut unique: BTreeMap<String, (PathBuf, InstallationSource, Vec<InstallationSource>)> =
        BTreeMap::new();
    let mut skipped = 0;

    for (root, source) in input.drive_roots {
        if !matches!(
            source,
            InstallationSource::FixedDrive | InstallationSource::RemovableDrive
        ) {
            skipped += 1;
            continue;
        }
        add_candidate(&mut unique, root, source);
    }
    for image in input.process_images {
        match installation_root_from_process_image(&image) {
            Some((root, source)) => add_candidate(&mut unique, root, source),
            None => skipped += 1,
        }
    }

    let mut candidates = Vec::new();
    for (_, (root, _, mut sources)) in unique {
        sources.sort();
        sources.dedup();
        match validate_installation(&root) {
            Ok(info) => candidates.push(InstallationCandidate { info, sources }),
            Err(_) => skipped += 1,
        }
    }
    DiscoveryReport {
        candidates,
        skipped,
    }
}

fn add_candidate(
    unique: &mut BTreeMap<String, (PathBuf, InstallationSource, Vec<InstallationSource>)>,
    root: PathBuf,
    source: InstallationSource,
) {
    let key = windows_path_key(&root);
    let entry = unique
        .entry(key)
        .or_insert_with(|| (root.clone(), source, Vec::new()));
    let candidate_path = root.to_string_lossy().into_owned();
    let selected_path = entry.0.to_string_lossy().into_owned();
    if source < entry.1 || (source == entry.1 && candidate_path < selected_path) {
        entry.0 = root;
        entry.1 = source;
    }
    entry.2.push(source);
}

fn require_file(path: &Path, reason: &'static str) -> Result<(), InstallationProblem> {
    let metadata = fs::metadata(path).map_err(classify_io)?;
    if metadata.is_file() {
        Ok(())
    } else {
        Err(InstallationProblem::Invalid(reason))
    }
}

fn classify_io(error: std::io::Error) -> InstallationProblem {
    match error.kind() {
        std::io::ErrorKind::NotFound => InstallationProblem::Missing,
        _ => InstallationProblem::Inaccessible,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn valid_install(root: &Path) {
        fs::create_dir_all(root.join("emulationstation/documented-subfolder")).unwrap();
        fs::create_dir_all(root.join(RETROARCH_DIRECTORY)).unwrap();
        fs::create_dir(root.join(DEFAULT_SAVES_DIRECTORY)).unwrap();
        fs::write(root.join(RETROBAT_EXECUTABLE), b"exe marker").unwrap();
        fs::write(
            root.join("emulationstation/documented-subfolder/es_systems.cfg"),
            "<systemList><system/></systemList>",
        )
        .unwrap();
        fs::write(root.join(RETROARCH_CONFIG), b"# existing config\n").unwrap();
    }

    #[test]
    fn candidate_sources_are_deduplicated_windows_aware_and_sorted() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("RetroBat");
        valid_install(&root);
        let retrobat = root.join("RetroBat.exe");
        let emulationstation = root.join("emulationstation/emulationstation.exe");
        fs::write(&emulationstation, b"running image marker").unwrap();
        let report = discover_installations(DiscoveryInput {
            drive_roots: vec![
                (root.clone(), InstallationSource::FixedDrive),
                (
                    PathBuf::from(root.to_string_lossy().to_uppercase()),
                    InstallationSource::RemovableDrive,
                ),
            ],
            process_images: vec![retrobat, emulationstation],
        });
        assert_eq!(report.candidates.len(), 1);
        assert_eq!(report.skipped, 0);
        assert_eq!(
            report.candidates[0].sources,
            vec![
                InstallationSource::FixedDrive,
                InstallationSource::RemovableDrive,
                InstallationSource::RetroBatProcess,
                InstallationSource::EmulationStationProcess,
            ]
        );
    }

    #[test]
    fn selected_install_validation_rejects_bad_markers_without_mutating_the_install() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("RetroBat");
        valid_install(&root);
        let before = fs::read(root.join(RETROARCH_CONFIG)).unwrap();
        let validated = validate_installation(&root).unwrap();
        assert_eq!(validated.documented_saves_root, root.join("saves"));
        assert_eq!(fs::read(root.join(RETROARCH_CONFIG)).unwrap(), before);
        assert!(!root.join(".rommfs").exists());

        fs::write(
            root.join("emulationstation/documented-subfolder/es_systems.cfg"),
            "<wrongRoot></wrongRoot>",
        )
        .unwrap();
        assert!(matches!(
            validate_installation(&root),
            Err(InstallationProblem::Invalid(_))
        ));
    }

    #[test]
    fn missing_saves_directory_is_a_candidate_and_is_not_created() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("RetroBat");
        valid_install(&root);
        fs::remove_dir(root.join(DEFAULT_SAVES_DIRECTORY)).unwrap();

        let info = validate_installation(&root).unwrap();
        assert_eq!(
            info.documented_saves_root,
            root.join(DEFAULT_SAVES_DIRECTORY)
        );
        assert!(!info.documented_saves_root.exists());

        let report = discover_installations(DiscoveryInput {
            drive_roots: vec![(root.clone(), InstallationSource::FixedDrive)],
            process_images: Vec::new(),
        });
        assert_eq!(report.candidates.len(), 1);
        assert_eq!(report.skipped, 0);
        assert_eq!(report.candidates[0].info.install_root, root);
    }

    #[test]
    fn emulationstation_config_search_stops_at_its_depth_bound() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("RetroBat");
        valid_install(&root);
        let config = root.join("emulationstation/documented-subfolder/es_systems.cfg");
        fs::remove_file(config).unwrap();
        let too_deep = root.join("emulationstation/one/two/three/es_systems.cfg");
        fs::create_dir_all(too_deep.parent().unwrap()).unwrap();
        fs::write(too_deep, "<systemList/>").unwrap();

        assert!(matches!(
            validate_installation(&root),
            Err(InstallationProblem::Invalid(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn emulationstation_config_search_does_not_follow_symlink_directories() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("RetroBat");
        valid_install(&root);
        fs::remove_file(root.join("emulationstation/documented-subfolder/es_systems.cfg")).unwrap();
        let external = dir.path().join("external-config");
        fs::create_dir(&external).unwrap();
        fs::write(external.join(EMULATIONSTATION_SYSTEMS), "<systemList/>").unwrap();
        std::os::unix::fs::symlink(&external, root.join("emulationstation/linked-config")).unwrap();

        assert!(matches!(
            validate_installation(&root),
            Err(InstallationProblem::Invalid(_))
        ));
    }

    #[test]
    fn invalid_candidates_are_misses_and_multiple_valid_candidates_stay_manual() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        valid_install(&first);
        valid_install(&second);
        let report = discover_installations(DiscoveryInput {
            drive_roots: vec![
                (first.clone(), InstallationSource::FixedDrive),
                (second.clone(), InstallationSource::RemovableDrive),
                (
                    dir.path().join("not-an-install"),
                    InstallationSource::FixedDrive,
                ),
            ],
            process_images: vec![dir.path().join("other.exe")],
        });
        assert_eq!(report.candidates.len(), 2);
        assert_eq!(report.skipped, 2);
        assert_ne!(
            windows_path_key(&report.candidates[0].info.install_root),
            windows_path_key(&report.candidates[1].info.install_root)
        );
    }

    #[test]
    fn process_image_paths_must_match_supported_executable_locations() {
        let root = PathBuf::from(r"D:\RetroBat");
        assert_eq!(
            installation_root_from_process_image(&root.join("RetroBat.exe")),
            Some((root.clone(), InstallationSource::RetroBatProcess))
        );
        assert_eq!(
            installation_root_from_process_image(
                &root.join("emulationstation/bin/emulationstation.exe")
            ),
            Some((root.clone(), InstallationSource::EmulationStationProcess))
        );
        assert!(
            installation_root_from_process_image(&root.join("nested/emulationstation.exe"))
                .is_none()
        );
        assert!(installation_root_from_process_image(&root.join("unrelated.exe")).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn inaccessible_installation_is_a_discovery_miss() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("RetroBat");
        valid_install(&root);
        let emulationstation = root.join(EMULATIONSTATION_DIRECTORY);
        fs::set_permissions(&emulationstation, fs::Permissions::from_mode(0o0)).unwrap();
        let denied = matches!(fs::read_dir(&emulationstation), Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied);

        if !denied {
            fs::set_permissions(&emulationstation, fs::Permissions::from_mode(0o755)).unwrap();
            return; // The test host bypasses directory permissions (e.g. root).
        }
        let report = discover_installations(DiscoveryInput {
            drive_roots: vec![(root.clone(), InstallationSource::FixedDrive)],
            process_images: Vec::new(),
        });
        let validation = validate_installation(&root);
        fs::set_permissions(&emulationstation, fs::Permissions::from_mode(0o755)).unwrap();

        assert!(report.candidates.is_empty());
        assert_eq!(report.skipped, 1);
        assert_eq!(validation, Err(InstallationProblem::Inaccessible));
    }
}
