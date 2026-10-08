use crate::layout::RetroBatSystems;
use crate::save_sync::{
    ensure_no_reparse_components, hold_save_directory_chain, path_is_reparse_point,
    path_is_symbolic_link, remove_symbolic_link,
};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const RECORD: &str = ".rommfs-setup.json";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[non_exhaustive]
pub struct SetupRecord {
    pub schema: u32,
    pub server_id: String,
    pub managed_root: PathBuf,
    pub version: Option<String>,
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
                record.schema == 1,
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
        schema: 1,
        server_id: server.into(),
        managed_root: managed,
        version,
    })
}

fn check_record(record: &SetupRecord, expected: &SetupRecord) -> anyhow::Result<()> {
    anyhow::ensure!(record.server_id == expected.server_id && record.managed_root == expected.managed_root,
        "RetroBat setup belongs to another server or managed root; existing files and links were preserved");
    Ok(())
}

/// Verify the link target AND its setup ownership, never just the path spelling.
pub fn verify_setup_link(installation: &Path, managed: &Path, server: &str) -> anyhow::Result<()> {
    let expected = expected_record(installation, managed, server)?;
    let record = load_record(installation)?
        .context("run RetroBat setup before mounting this integration")?;
    check_record(&record, &expected)?;
    let roms = installation.join("roms");
    anyhow::ensure!(
        path_is_reparse_point(&fs::symlink_metadata(&roms)?),
        "RetroBat roms directory is not the setup-owned link"
    );
    anyhow::ensure!(
        fs::canonicalize(&roms)? == expected.managed_root,
        "RetroBat roms link points to a different directory; it was not changed"
    );
    Ok(())
}

/// What stands between a real `roms/` directory and the managed link,
/// classified so a caller can offer one explicit cleanup for the entries that
/// are never user data while still refusing to touch the rest.
#[derive(Debug, Default)]
pub struct RomsConflicts {
    /// RetroBat stock info/read-me files, OS metadata noise, and
    /// symlinks/junctions. Removing a link deletes only the link, never its
    /// target — these are safe to delete only after explicit user consent.
    pub removable: Vec<PathBuf>,
    /// Anything else — possible user data. Setup refuses until the user moves
    /// these out themselves; RomMFS never moves, backs up, or deletes them.
    pub blocking: Vec<PathBuf>,
}

impl RomsConflicts {
    pub fn is_empty(&self) -> bool {
        self.removable.is_empty() && self.blocking.is_empty()
    }
}

impl std::fmt::Display for RomsConflicts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.blocking.is_empty() {
            write!(
                f,
                "RetroBat roms contains {} entr{} that may be user data — move them out before setup:\n{}",
                self.blocking.len(),
                if self.blocking.len() == 1 { "y" } else { "ies" },
                self.blocking
                    .iter()
                    .take(12)
                    .map(|p| format!("  {}", p.display()))
                    .collect::<Vec<_>>()
                    .join("\n")
            )?;
            if self.blocking.len() > 12 {
                write!(f, "\n  and {} more", self.blocking.len() - 12)?;
            }
            if !self.removable.is_empty() {
                write!(
                    f,
                    "\n{} known RetroBat stock/system file(s) and link(s) can also be removed",
                    self.removable.len()
                )?;
            }
        } else {
            write!(
                f,
                "RetroBat roms contains only {} known stock/system file(s) and link(s); RomMFS can remove them with your consent:\n{}",
                self.removable.len(),
                self.removable
                    .iter()
                    .take(12)
                    .map(|p| format!("  {}", p.display()))
                    .collect::<Vec<_>>()
                    .join("\n")
            )?;
            if self.removable.len() > 12 {
                write!(f, "\n  and {} more", self.removable.len() - 12)?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for RomsConflicts {}

/// Filenames that are never user data inside a stock RetroBat `roms` tree:
/// the per-system info files a fresh install ships and OS metadata noise.
fn stock_or_noise_name(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let name = name.to_lowercase();
    (name.starts_with("_info") && name.ends_with(".txt"))
        || matches!(
            name.as_str(),
            "readme.txt" | "_readme.txt" | "readme.md" | "desktop.ini" | "thumbs.db" | ".ds_store"
        )
}

/// Remove one entry the classifier marked removable. The type is re-verified
/// at delete time so a file swapped in between scan and consent is never
/// deleted; a link loses only the link object, never its target.
fn remove_known_foreign(path: &Path) -> anyhow::Result<()> {
    let _parent_guard =
        hold_save_directory_chain(path.parent().context("removable entry has no parent")?)?;
    let metadata = fs::symlink_metadata(path)?;
    if path_is_reparse_point(&metadata) {
        remove_symbolic_link(path)?;
        return Ok(());
    }
    anyhow::ensure!(
        metadata.is_file() && stock_or_noise_name(path),
        "not a classified removable entry: {}",
        path.display()
    );
    fs::remove_file(path)?;
    Ok(())
}

/// Link an empty ROM directory to a separately verified managed root. The caller
/// must validate native root ownership and establish that RetroBat is stopped.
/// A persisted intent permits recovery after interruption between record/link.
/// `remove_known` consents to deleting only classified stock/OS files and
/// links; possible user files still refuse and are never moved or deleted.
pub fn setup_roms_link(
    installation: &Path,
    managed: &Path,
    server: &str,
    remove_known: bool,
) -> anyhow::Result<SetupRecord> {
    let expected = expected_record(installation, managed, server)?;
    let _installation_guard = hold_save_directory_chain(installation)?;
    let _managed_guard = hold_save_directory_chain(&expected.managed_root)?;
    let previous = load_record(installation)?;
    if let Some(record) = &previous {
        check_record(record, &expected)?;
    }
    let roms = installation.join("roms");
    let metadata = match fs::symlink_metadata(&roms) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if metadata.as_ref().is_some_and(path_is_reparse_point) {
        verify_setup_link(installation, managed, server)?;
        return Ok(expected);
    }
    anyhow::ensure!(
        fs::canonicalize(installation)? != expected.managed_root,
        "installation and managed ROM root must differ"
    );
    let mut directories = Vec::new();
    let mut conflicts = RomsConflicts::default();
    if roms.try_exists()? {
        scan_roms_contents(&roms, &mut directories, &mut conflicts)?;
    }
    if remove_known {
        // Best-effort per entry: re-verified at delete time, failures are
        // reported as still-removable so a retry (or manual cleanup) sees them.
        let mut kept = Vec::new();
        let mut errors = Vec::new();
        for path in conflicts.removable.drain(..) {
            match remove_known_foreign(&path) {
                Ok(()) => {}
                Err(e) => {
                    kept.push(path.clone());
                    errors.push(format!("{}: {e}", path.display()));
                }
            }
        }
        conflicts.removable = kept;
        if !errors.is_empty() {
            return Err(anyhow::Error::new(conflicts).context(format!(
                "failed to remove {} known entries: {}",
                errors.len(),
                errors.join("; ")
            )));
        }
    }
    if !conflicts.is_empty() {
        return Err(anyhow::Error::new(conflicts));
    }
    let record_path = installation.join(RECORD);
    // Do not replace an existing ownership record; compatible upgrades are
    // diagnostics, not a reason to acquire different ownership.
    if previous.is_none() {
        let mut file = tempfile::NamedTempFile::new_in(installation)?;
        serde_json::to_writer(file.as_file_mut(), &expected)?;
        file.as_file().sync_all()?;
        file.persist_noclobber(&record_path)
            .map_err(|error| error.error)?;
    }
    // Only empty ordinary directories. remove_dir refuses a concurrent new file.
    // No recursive deletion, backups, or migration of the ROM directory.
    for directory in directories.into_iter().rev() {
        let _parent_guard =
            hold_save_directory_chain(directory.parent().context("ROM directory has no parent")?)?;
        ensure_no_reparse_components(&directory)?;
        fs::remove_dir(directory)?;
    }
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&expected.managed_root, &roms).context(
        "create RetroBat ROM directory link (Windows Developer Mode or link privilege is required)",
    )?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(&expected.managed_root, &roms)?;
    verify_setup_link(installation, managed, server)?;
    Ok(expected)
}

/// Collect every ordinary directory (emptied later, deepest last) and classify
/// each non-directory entry: links and known stock/OS files are removable with
/// consent; everything else blocks setup as possible user data.
fn scan_roms_contents(
    root: &Path,
    directories: &mut Vec<PathBuf>,
    conflicts: &mut RomsConflicts,
) -> anyhow::Result<()> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let _parent_guard =
            hold_save_directory_chain(path.parent().context("ROM entry has no parent")?)?;
        let metadata = fs::symlink_metadata(&path)?;
        if path_is_reparse_point(&metadata) {
            if path_is_symbolic_link(&path)? {
                conflicts.removable.push(path);
            } else {
                conflicts.blocking.push(path);
            }
            continue;
        }
        if !metadata.is_dir() {
            if metadata.is_file() && stock_or_noise_name(&path) {
                conflicts.removable.push(path);
            } else {
                conflicts.blocking.push(path);
            }
            continue;
        }
        let _directory_guard = hold_save_directory_chain(&path)?;
        directories.push(path.clone());
        for entry in fs::read_dir(path)? {
            pending.push(entry?.path());
        }
    }
    conflicts.removable.sort();
    conflicts.blocking.sort();
    Ok(())
}
