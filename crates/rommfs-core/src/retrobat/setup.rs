use crate::layout::RetroBatSystems;
use crate::save_sync::{
    ensure_no_reparse_components, hold_save_directory_chain, path_is_reparse_point,
};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const RECORD: &str = ".rommfs-setup.json";
const ORIGINAL_ROMS: &str = ".rommfs-original-roms";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[non_exhaustive]
pub struct SetupRecord {
    pub schema: u32,
    pub server_id: String,
    pub managed_root: PathBuf,
    pub version: Option<String>,
    /// Per-system links (`roms/<name>`) RomMFS created. Empty for schema 1
    /// records, whose whole `roms` directory was a single link.
    #[serde(default)]
    pub links: Vec<String>,
}

/// What a setup run did, so the caller can report skipped systems.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct SetupOutcome {
    pub record: SetupRecord,
    /// Systems now linked to the managed library (new or already present).
    pub linked: Vec<String>,
    /// Systems whose paths are foreign links or files and were left as-is.
    pub skipped: Vec<String>,
    /// Where original system directories were preserved, if present.
    pub original_roms: Option<PathBuf>,
    /// Setup predates per-system links: `roms` itself is the managed link.
    pub legacy_whole_link: bool,
}

impl SetupOutcome {
    pub fn summary(&self) -> String {
        if self.legacy_whole_link {
            return "RetroBat's roms folder is already linked to the managed folder".into();
        }
        let mut text = format!("{} system folder(s) linked", self.linked.len());
        if let Some(original_roms) = &self.original_roms {
            text.push_str(&format!(
                "; original system folders preserved in {}",
                original_roms.display()
            ));
        }
        if !self.skipped.is_empty() {
            text.push_str(&format!(
                "; {} system(s) have foreign links or files and were left as-is: {}",
                self.skipped.len(),
                self.skipped.join(", ")
            ));
        }
        text
    }
}

fn inspect_installation(installation: &Path) -> anyhow::Result<()> {
    ensure_no_reparse_components(installation)?;
    anyhow::ensure!(
        installation.is_absolute(),
        "select an absolute RetroBat installation path"
    );
    let executable = installation.join("retrobat.exe");
    ensure_no_reparse_components(&executable)?;
    anyhow::ensure!(
        executable.is_file(),
        "RetroBat launcher is missing: {}",
        executable.display()
    );
    RetroBatSystems::from_installation(installation)?;
    Ok(())
}

fn load_record(installation: &Path) -> anyhow::Result<Option<SetupRecord>> {
    let path = installation.join(RECORD);
    ensure_no_reparse_components(&path)?;
    match fs::read(&path) {
        Ok(bytes) => {
            let record: SetupRecord =
                serde_json::from_slice(&bytes).context("invalid RetroBat setup record")?;
            anyhow::ensure!(
                matches!(record.schema, 1 | 2),
                "unsupported RetroBat setup record schema"
            );
            Ok(Some(record))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn expected_record(
    installation: &Path,
    managed: &Path,
    server: &str,
) -> anyhow::Result<SetupRecord> {
    inspect_installation(installation)?;
    ensure_no_reparse_components(managed)?;
    anyhow::ensure!(
        managed.is_absolute() && managed.is_dir(),
        "managed ROM root must be an existing absolute directory"
    );
    let managed = fs::canonicalize(managed)?;
    let install = fs::canonicalize(installation)?;
    anyhow::ensure!(
        !install.starts_with(&managed) && !managed.starts_with(&install),
        "RetroBat installation and managed ROM root must be separate directories"
    );
    let version_file = installation.join("system/version.info");
    ensure_no_reparse_components(&version_file)?;
    let version = match fs::read_to_string(version_file) {
        Ok(version) => Some(version.trim().to_owned()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    Ok(SetupRecord {
        schema: 2,
        server_id: server.into(),
        managed_root: managed,
        version,
        links: Vec::new(),
    })
}

fn check_record(record: &SetupRecord, expected: &SetupRecord) -> anyhow::Result<()> {
    anyhow::ensure!(record.server_id == expected.server_id && record.managed_root == expected.managed_root,
        "RetroBat setup belongs to another server or managed root; existing files and links were preserved");
    Ok(())
}

/// Compare link targets tolerating Windows verbatim (`\\?\`) prefixes.
fn same_target(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let text = p.to_string_lossy().replace('\\', "/");
        text.strip_prefix("//?/").unwrap_or(&text).to_lowercase()
    };
    a == b || norm(a) == norm(b)
}

/// Verify setup ownership and every recorded link, never just path spelling.
/// A recorded link that is missing is tolerated (re-running setup restores it);
/// one that now points elsewhere is rejected and left untouched.
pub fn verify_setup_link(installation: &Path, managed: &Path, server: &str) -> anyhow::Result<()> {
    let expected = expected_record(installation, managed, server)?;
    let record = load_record(installation)?
        .context("run RetroBat setup before mounting this integration")?;
    check_record(&record, &expected)?;
    let roms = installation.join("roms");
    // `roms` itself may legitimately be a legacy link; `installation` is already checked.
    let metadata = fs::symlink_metadata(&roms)?;
    if path_is_reparse_point(&metadata) {
        // Schema 1: `roms` itself is the setup-owned link.
        anyhow::ensure!(
            fs::canonicalize(&roms)? == expected.managed_root,
            "RetroBat roms link points to a different directory; it was not changed"
        );
        return Ok(());
    }
    anyhow::ensure!(metadata.is_dir(), "RetroBat roms is not a directory");
    for name in &record.links {
        validate_link_name(name)?;
        let link = roms.join(name);
        match fs::symlink_metadata(&link) {
            Ok(m) if path_is_reparse_point(&m) => {
                let target = fs::read_link(&link)?;
                anyhow::ensure!(
                    same_target(&target, &expected.managed_root.join(name)),
                    "RetroBat roms\\{name} link points to a different directory; it was not changed"
                );
            }
            // Missing or replaced by the user: no longer ours, nothing to verify.
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// Read the current setup result without creating links or changing settings.
/// Used by the unelevated app after the Windows setup helper exits.
pub fn inspect_setup(
    installation: &Path,
    managed: &Path,
    server: &str,
) -> anyhow::Result<SetupOutcome> {
    verify_setup_link(installation, managed, server)?;
    let record = load_record(installation)?.context("missing RetroBat setup record")?;
    let roms = installation.join("roms");
    let legacy_whole_link = path_is_reparse_point(&fs::symlink_metadata(&roms)?);
    let mut linked = Vec::new();
    let mut skipped = Vec::new();
    if !legacy_whole_link {
        for name in RetroBatSystems::from_installation(installation)?.folders() {
            let path = roms.join(name);
            if fs::read_link(&path)
                .is_ok_and(|target| same_target(&target, &record.managed_root.join(name)))
            {
                linked.push(name.to_owned());
            } else {
                skipped.push(name.to_owned());
            }
        }
    }
    Ok(SetupOutcome {
        record,
        linked,
        skipped,
        original_roms: installation
            .join(ORIGINAL_ROMS)
            .try_exists()?
            .then(|| installation.join(ORIGINAL_ROMS)),
        legacy_whole_link,
    })
}

fn validate_link_name(name: &str) -> anyhow::Result<()> {
    crate::save_sync::validate_windows_path_component(name)
        .map_err(|e| anyhow::anyhow!("invalid recorded system name {name:?}: {e}"))
}

/// Link `roms/<system>` to the managed library for each system RetroBat knows.
/// Original directories are moved intact to `.rommfs-original-roms/<system>`;
/// existing backups are never overwritten. Foreign links and files stay untouched.
///
/// The caller must validate native root ownership and establish that RetroBat
/// is stopped. The record (listing intended links) is persisted before the
/// links are created so an interrupted run is recovered by running setup again.
pub fn setup_roms_link(
    installation: &Path,
    managed: &Path,
    server: &str,
) -> anyhow::Result<SetupOutcome> {
    let mut expected = expected_record(installation, managed, server)?;
    let _installation_guard = hold_save_directory_chain(installation)?;
    let _managed_guard = hold_save_directory_chain(&expected.managed_root)?;
    let previous = load_record(installation)?;
    if let Some(record) = &previous {
        check_record(record, &expected)?;
    }
    anyhow::ensure!(
        fs::canonicalize(installation)? != expected.managed_root,
        "installation and managed ROM root must differ"
    );
    let roms = installation.join("roms");
    match fs::symlink_metadata(&roms) {
        Ok(m) if path_is_reparse_point(&m) => {
            verify_setup_link(installation, managed, server)?;
            return Ok(SetupOutcome {
                record: previous.unwrap_or(expected),
                linked: Vec::new(),
                skipped: Vec::new(),
                original_roms: None,
                legacy_whole_link: true,
            });
        }
        Ok(m) => anyhow::ensure!(m.is_dir(), "RetroBat roms exists and is not a directory"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&roms)?,
        Err(e) => return Err(e.into()),
    }
    let systems = RetroBatSystems::from_installation(installation)?;
    let recorded = previous
        .as_ref()
        .map(|r| r.links.clone())
        .unwrap_or_default();

    enum Plan {
        Adopt,
        Create,
        Preserve,
    }
    let original_roms = installation.join(ORIGINAL_ROMS);
    let mut plans = Vec::new();
    let mut skipped = Vec::new();
    for name in systems.folders() {
        let link = roms.join(name);
        match fs::symlink_metadata(&link) {
            Ok(m) if path_is_reparse_point(&m) => {
                let ours = fs::read_link(&link)
                    .is_ok_and(|t| same_target(&t, &expected.managed_root.join(name)));
                if ours {
                    plans.push((name.to_owned(), Plan::Adopt));
                } else {
                    skipped.push(name.to_owned());
                }
            }
            Ok(m) if m.is_dir() => {
                ensure_no_reparse_components(&original_roms)?;
                let backup = original_roms.join(name);
                anyhow::ensure!(
                    fs::symlink_metadata(&backup)
                        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
                    "original ROM backup already exists; both folders were preserved: {}",
                    backup.display()
                );
                plans.push((name.to_owned(), Plan::Preserve));
            }
            Ok(_) => skipped.push(name.to_owned()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                plans.push((name.to_owned(), Plan::Create));
            }
            Err(e) => return Err(e.into()),
        }
    }

    let mut links: Vec<String> = recorded;
    for (name, _) in &plans {
        if !links.contains(name) {
            links.push(name.clone());
        }
    }
    links.sort();
    expected.links = links;
    let record_path = installation.join(RECORD);
    if previous.as_ref().map(|r| (&r.links, r.schema)) != Some((&expected.links, 2)) {
        let mut file = tempfile::NamedTempFile::new_in(installation)?;
        serde_json::to_writer(file.as_file_mut(), &expected)?;
        file.as_file().sync_all()?;
        file.persist(&record_path).map_err(|error| error.error)?;
    }

    let _roms_guard = hold_save_directory_chain(&roms)?;
    let _backup_guard = if plans.iter().any(|(_, plan)| matches!(plan, Plan::Preserve)) {
        if !original_roms.try_exists()? {
            fs::create_dir(&original_roms)?;
        }
        Some(hold_save_directory_chain(&original_roms)?)
    } else {
        None
    };
    let mut linked = Vec::new();
    for (name, plan) in plans {
        let link = roms.join(&name);
        if !matches!(plan, Plan::Adopt) {
            if matches!(plan, Plan::Preserve) {
                ensure_no_reparse_components(&link)?;
                let backup = original_roms.join(&name);
                anyhow::ensure!(
                    fs::symlink_metadata(&backup)
                        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
                    "original ROM backup already exists: {}",
                    backup.display()
                );
                fs::rename(&link, &backup).with_context(|| {
                    format!("preserve original ROM folder in {}", backup.display())
                })?;
            }
            let target = expected.managed_root.join(&name);
            #[cfg(windows)]
            std::os::windows::fs::symlink_dir(&target, &link)
                .context("create RetroBat ROM directory link")?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, &link)?;
        }
        linked.push(name);
    }
    verify_setup_link(installation, managed, server)?;
    Ok(SetupOutcome {
        record: expected,
        linked,
        skipped,
        original_roms: original_roms.try_exists()?.then_some(original_roms),
        legacy_whole_link: false,
    })
}
