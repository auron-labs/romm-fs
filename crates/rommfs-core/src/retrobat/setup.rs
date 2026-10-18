use crate::layout::RetroBatSystems;
use crate::save_sync::{
    ensure_no_reparse_components, hold_save_directory_chain, path_is_reparse_point,
};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const RECORD: &str = ".rommfs-setup.json";
/// Where an earlier RomMFS build moved existing system folders. Setup moves
/// them back; nothing is moved there any more.
const ORIGINAL_ROMS: &str = ".rommfs-original-roms";
const LINK_PROBE: &str = ".rommfs-link-probe";
const SCHEMA: u32 = 3;

/// The folder RomMFS adds inside each RetroBat system folder. It links to the
/// managed library, so RomM games sit alongside the user's own ROMs.
pub const ROMM_FOLDER: &str = "RomM";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[non_exhaustive]
pub struct SetupRecord {
    pub schema: u32,
    pub server_id: String,
    pub managed_root: PathBuf,
    pub version: Option<String>,
    /// Systems RomMFS linked. Schema 3: `roms/<name>/RomM`; schema 2:
    /// `roms/<name>` itself; schema 1: empty, `roms` itself was the link.
    #[serde(default)]
    pub links: Vec<String>,
    /// System folders RomMFS created because RetroBat had none.
    #[serde(default)]
    pub created_dirs: Vec<String>,
    /// Systems the last run could not link, with the reason.
    #[serde(default)]
    pub skipped: Vec<SkippedSystem>,
    /// Folders the last run moved back from an earlier build's backup.
    #[serde(default)]
    pub restored: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SkippedSystem {
    pub system: String,
    pub reason: String,
}

/// What a setup run did. One system failing never stops the others.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct SetupOutcome {
    pub record: SetupRecord,
    /// Systems whose `RomM` folder links to the managed library.
    pub linked: Vec<String>,
    /// Systems left without a `RomM` folder, and why.
    pub skipped: Vec<SkippedSystem>,
    /// System folders moved back from `.rommfs-original-roms`.
    pub restored: Vec<String>,
}

impl SetupOutcome {
    fn from_record(record: SetupRecord) -> Self {
        Self {
            linked: record.links.clone(),
            skipped: record.skipped.clone(),
            restored: record.restored.clone(),
            record,
        }
    }

    pub fn summary(&self) -> String {
        let mut text = format!("{} systems linked", self.linked.len());
        if !self.restored.is_empty() {
            text.push_str(&format!(
                "; {} folders restored from an earlier setup: {}",
                self.restored.len(),
                self.restored.join(", ")
            ));
        }
        if !self.skipped.is_empty() {
            let systems: Vec<_> = self.skipped.iter().map(|s| s.system.as_str()).collect();
            text.push_str(&format!(
                "; {} skipped: {}",
                self.skipped.len(),
                systems.join(", ")
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
                (1..=SCHEMA).contains(&record.schema),
                "unsupported RetroBat setup record schema"
            );
            Ok(Some(record))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn save_record(installation: &Path, record: &SetupRecord) -> anyhow::Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(installation)?;
    serde_json::to_writer(file.as_file_mut(), record)?;
    file.as_file().sync_all()?;
    file.persist(installation.join(RECORD))
        .map_err(|error| error.error)?;
    Ok(())
}

fn expected_record(
    installation: &Path,
    managed: &Path,
    server: &str,
) -> anyhow::Result<SetupRecord> {
    expected_record_with_root_check(installation, managed, server, |root| {
        ensure_no_reparse_components(root).map_err(anyhow::Error::from)
    })
}

fn expected_record_with_root_check(
    installation: &Path,
    managed: &Path,
    server: &str,
    check_root: impl FnOnce(&Path) -> anyhow::Result<()>,
) -> anyhow::Result<SetupRecord> {
    inspect_installation(installation)?;
    check_root(managed)?;
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
        schema: SCHEMA,
        server_id: server.into(),
        managed_root: managed,
        version,
        links: Vec::new(),
        created_dirs: Vec::new(),
        skipped: Vec::new(),
        restored: Vec::new(),
    })
}

/// Compare link targets tolerating Windows verbatim (`\\?\`) prefixes.
fn same_target(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let text = p.to_string_lossy().replace('\\', "/");
        text.strip_prefix("//?/").unwrap_or(&text).to_lowercase()
    };
    a == b || norm(a) == norm(b)
}

fn create_dir_link(target: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    return std::os::windows::fs::symlink_dir(target, link);
    #[cfg(unix)]
    return std::os::unix::fs::symlink(target, link);
}

fn remove_dir_link(link: &Path) -> std::io::Result<()> {
    // Windows directory links are removed as directories; their target is untouched.
    #[cfg(windows)]
    return fs::remove_dir(link);
    #[cfg(unix)]
    return fs::remove_file(link);
}

/// Confirm Windows lets this process create directory links before anything
/// changes, so a permission failure (and a declined UAC prompt) leaves the
/// installation exactly as it was.
fn probe_link_permission(installation: &Path, target: &Path) -> anyhow::Result<()> {
    let probe = installation.join(LINK_PROBE);
    if let Ok(metadata) = fs::symlink_metadata(&probe) {
        anyhow::ensure!(
            path_is_reparse_point(&metadata),
            "{} exists and is not a RomMFS link probe",
            probe.display()
        );
        remove_dir_link(&probe)?;
    }
    create_dir_link(target, &probe).context("create RetroBat ROM directory link")?;
    remove_dir_link(&probe)?;
    Ok(())
}

/// Verify setup ran for this server and managed folder with the current link
/// layout. Individual links are not checked: a system whose link was removed
/// or changed is simply no longer linked.
pub fn verify_setup_link(installation: &Path, managed: &Path, server: &str) -> anyhow::Result<()> {
    verify_setup_with_root_check(installation, managed, server, |root| {
        ensure_no_reparse_components(root).map_err(anyhow::Error::from)
    })
}

fn verify_setup_with_root_check(
    installation: &Path,
    managed: &Path,
    server: &str,
    check_root: impl FnOnce(&Path) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let expected = expected_record_with_root_check(installation, managed, server, check_root)?;
    let record = load_record(installation)?
        .context("run RetroBat setup before mounting this integration")?;
    anyhow::ensure!(
        record.schema == SCHEMA,
        "run RetroBat setup again so RomM games appear alongside your own ROMs"
    );
    anyhow::ensure!(
        record.server_id == expected.server_id && record.managed_root == expected.managed_root,
        "RetroBat was set up for another server or games folder; run RetroBat setup again"
    );
    Ok(())
}

/// Whether an earlier build's setup record needs migrating before this
/// integration can mount: running setup converts old links and moves
/// backed-up folders back. False when setup never ran (nothing to migrate) or
/// the record already matches this build.
pub fn setup_needs_migration(installation: &Path) -> bool {
    matches!(load_record(installation), Ok(Some(record)) if record.schema != SCHEMA)
}

/// Read the last setup result without changing anything. Used by the
/// unelevated app after the Windows setup helper exits.
pub fn inspect_setup(
    installation: &Path,
    managed: &Path,
    server: &str,
) -> anyhow::Result<SetupOutcome> {
    inspect_setup_with_root_check(installation, managed, server, |root| {
        ensure_no_reparse_components(root).map_err(anyhow::Error::from)
    })
}

fn inspect_setup_with_root_check(
    installation: &Path,
    managed: &Path,
    server: &str,
    check_root: impl FnOnce(&Path) -> anyhow::Result<()>,
) -> anyhow::Result<SetupOutcome> {
    verify_setup_with_root_check(installation, managed, server, check_root)?;
    let record = load_record(installation)?.context("missing RetroBat setup record")?;
    let mut outcome = SetupOutcome::from_record(record);
    outcome.linked.clear();
    for name in &outcome.record.links {
        validate_system_name(name)?;
        let folder = installation.join("roms").join(name);
        let current = ensure_no_reparse_components(&folder)
            .and_then(|()| Ok(fs::read_link(folder.join(ROMM_FOLDER))?));
        if current.is_ok_and(|target| same_target(&target, &outcome.record.managed_root.join(name)))
        {
            outcome.linked.push(name.clone());
        } else {
            outcome.skipped.push(SkippedSystem {
                system: name.clone(),
                reason: "RomM folder no longer links to the managed system folder".into(),
            });
        }
    }
    Ok(outcome)
}

/// Inspect linked systems using the owning backend's managed-root validation.
/// The check must reject foreign or redirected roots before setup identity is
/// compared. Ordinary save/setup callers continue to reject all reparse points.
pub fn linked_systems_with_root_check(
    installation: &Path,
    managed: &Path,
    server: &str,
    check_root: impl FnOnce(&Path) -> anyhow::Result<()>,
) -> anyhow::Result<Vec<String>> {
    Ok(inspect_setup_with_root_check(installation, managed, server, check_root)?.linked)
}

/// Systems whose `RomM` folder setup linked, for gamelist publication.
pub fn linked_systems(
    installation: &Path,
    managed: &Path,
    server: &str,
) -> anyhow::Result<Vec<String>> {
    Ok(inspect_setup(installation, managed, server)?.linked)
}

fn validate_system_name(name: &str) -> anyhow::Result<()> {
    crate::save_sync::validate_windows_path_component(name)
        .map_err(|e| anyhow::anyhow!("invalid system name {name:?}: {e}"))
}

enum SystemResult {
    Linked { created: bool, restored: bool },
    Skipped(String),
}

/// Add `roms/<system>/RomM` → `<managed>/<system>` for each system RetroBat
/// knows. The user's own files in `roms/<system>` are never moved, replaced or
/// deleted; a system that can't be linked is skipped with a reason and the
/// others still proceed.
///
/// Links left by earlier builds are converted: a `roms/<system>` link RomMFS
/// owns becomes a real folder again, and a folder an earlier build moved to
/// `.rommfs-original-roms` is moved back. A record for another server or
/// managed folder is retargeted, since every link it lists is RomMFS's own.
///
/// The caller must validate native root ownership and establish that RetroBat
/// is stopped.
pub fn setup_roms_link(
    installation: &Path,
    managed: &Path,
    server: &str,
) -> anyhow::Result<SetupOutcome> {
    let mut expected = expected_record(installation, managed, server)?;
    let _installation_guard = hold_save_directory_chain(installation)?;
    let _managed_guard = hold_save_directory_chain(&expected.managed_root)?;
    let previous = load_record(installation)?;
    probe_link_permission(installation, &expected.managed_root)?;

    // Every managed root a record has pointed at: links into these are ours.
    let mut owned_roots = vec![expected.managed_root.clone()];
    if let Some(record) = &previous {
        owned_roots.push(record.managed_root.clone());
        expected.created_dirs = record.created_dirs.clone();
    }

    let roms = installation.join("roms");
    match fs::symlink_metadata(&roms) {
        Ok(m) if path_is_reparse_point(&m) => {
            // Schema 1 linked `roms` itself; turn it back into a real folder.
            let ours = previous.as_ref().is_some_and(|r| r.schema == 1)
                && fs::read_link(&roms)
                    .is_ok_and(|t| owned_roots.iter().any(|root| same_target(&t, root)));
            anyhow::ensure!(
                ours,
                "RetroBat's roms folder is a link to another folder; RomMFS left it unchanged"
            );
            remove_dir_link(&roms)?;
            fs::create_dir(&roms)?;
        }
        Ok(m) => anyhow::ensure!(m.is_dir(), "RetroBat roms exists and is not a folder"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&roms)?,
        Err(e) => return Err(e.into()),
    }
    let _roms_guard = hold_save_directory_chain(&roms)?;
    let backups = installation.join(ORIGINAL_ROMS);
    let has_backups = fs::symlink_metadata(&backups).is_ok_and(|m| m.is_dir());

    let systems = RetroBatSystems::from_installation(installation)?;
    for name in systems.folders() {
        let result = validate_system_name(name).and_then(|()| {
            setup_system(
                &roms,
                name,
                &expected.managed_root,
                &owned_roots,
                has_backups.then_some(backups.as_path()),
            )
        });
        match result {
            Ok(SystemResult::Linked { created, restored }) => {
                expected.links.push(name.to_owned());
                if created && !expected.created_dirs.iter().any(|d| d == name) {
                    expected.created_dirs.push(name.to_owned());
                }
                if restored {
                    expected.restored.push(name.to_owned());
                }
            }
            Ok(SystemResult::Skipped(reason)) => expected.skipped.push(SkippedSystem {
                system: name.to_owned(),
                reason,
            }),
            Err(error) => expected.skipped.push(SkippedSystem {
                system: name.to_owned(),
                reason: format!("{error:#}"),
            }),
        }
    }
    // An earlier build's backup folder goes once every system is moved back.
    if has_backups && fs::read_dir(&backups)?.next().is_none() {
        fs::remove_dir(&backups)?;
    }
    expected.links.sort();
    expected.created_dirs.sort();
    save_record(installation, &expected)?;
    Ok(SetupOutcome::from_record(expected))
}

fn setup_system(
    roms: &Path,
    name: &str,
    managed_root: &Path,
    owned_roots: &[PathBuf],
    backups: Option<&Path>,
) -> anyhow::Result<SystemResult> {
    let folder = roms.join(name);
    let owned = |target: &Path, suffix: &Path| {
        owned_roots
            .iter()
            .any(|root| same_target(target, &root.join(suffix)))
    };
    match fs::symlink_metadata(&folder) {
        Ok(m) if path_is_reparse_point(&m) => {
            // Schema 2 linked the whole system folder; ours to replace.
            if !fs::read_link(&folder).is_ok_and(|t| owned(&t, Path::new(name))) {
                return Ok(SystemResult::Skipped(format!(
                    "roms\\{name} is a link to another folder"
                )));
            }
            remove_dir_link(&folder)?;
        }
        Ok(m) if m.is_dir() => {}
        Ok(_) => {
            return Ok(SystemResult::Skipped(format!(
                "roms\\{name} is a file, not a folder"
            )))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }

    let mut restored = false;
    if let Some(backup) = backups.map(|b| b.join(name)) {
        let is_folder =
            fs::symlink_metadata(&backup).is_ok_and(|m| m.is_dir() && !path_is_reparse_point(&m));
        if is_folder && fs::symlink_metadata(&folder).is_err() {
            fs::rename(&backup, &folder)
                .with_context(|| format!("move {} back to roms\\{name}", backup.display()))?;
            restored = true;
        }
    }

    let mut created = false;
    if fs::symlink_metadata(&folder).is_err() {
        fs::create_dir(&folder)?;
        created = true;
    }

    let link = folder.join(ROMM_FOLDER);
    let target = managed_root.join(name);
    match fs::symlink_metadata(&link) {
        Ok(m) if path_is_reparse_point(&m) => {
            let current = fs::read_link(&link)?;
            if same_target(&current, &target) {
                return Ok(SystemResult::Linked { created, restored });
            }
            if !owned(&current, Path::new(name)) {
                return Ok(SystemResult::Skipped(format!(
                    "roms\\{name}\\{ROMM_FOLDER} is a link to another folder"
                )));
            }
            remove_dir_link(&link)?;
        }
        Ok(_) => {
            return Ok(SystemResult::Skipped(format!(
                "roms\\{name}\\{ROMM_FOLDER} already exists"
            )))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    if let Err(error) = create_dir_link(&target, &link) {
        if created {
            let _ = fs::remove_dir(&folder);
        }
        return Err(error).context("create RetroBat ROM directory link");
    }
    Ok(SystemResult::Linked { created, restored })
}
