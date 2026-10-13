use crate::layout::RetroBatSystems;
use crate::save_sync::{
    ensure_no_reparse_components, hold_save_directory_chain, path_is_reparse_point,
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
    /// Systems whose `roms/<name>` already has content and was left as-is.
    pub skipped: Vec<String>,
    /// Setup predates per-system links: `roms` itself is the managed link.
    pub legacy_whole_link: bool,
}

impl SetupOutcome {
    pub fn summary(&self) -> String {
        if self.legacy_whole_link {
            return "RetroBat's roms folder is already linked to the managed folder".into();
        }
        let mut text = format!("{} system folder(s) linked", self.linked.len());
        if !self.skipped.is_empty() {
            text.push_str(&format!(
                "; {} system(s) already have content in RetroBat's roms folder and were left as-is: {}",
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

fn validate_link_name(name: &str) -> anyhow::Result<()> {
    crate::save_sync::validate_windows_path_component(name)
        .map_err(|e| anyhow::anyhow!("invalid recorded system name {name:?}: {e}"))
}

/// Returns the directories (parents first) of `root` when it contains nothing
/// but empty ordinary directories, or `None` as soon as any file or link shows
/// up. Only an entirely empty tree may be replaced by a link.
fn empty_directory_tree(root: &Path) -> anyhow::Result<Option<Vec<PathBuf>>> {
    let mut directories = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let _parent_guard =
            hold_save_directory_chain(path.parent().context("ROM entry has no parent")?)?;
        let metadata = fs::symlink_metadata(&path)?;
        if path_is_reparse_point(&metadata) || !metadata.is_dir() {
            return Ok(None);
        }
        directories.push(path.clone());
        for entry in fs::read_dir(path)? {
            pending.push(entry?.path());
        }
    }
    Ok(Some(directories))
}

/// Coexist with whatever is already in RetroBat's `roms` directory: keep it a
/// real directory and add `roms/<system>` links to the managed library for each
/// system RetroBat knows. A system whose folder already has content (stock or
/// user) is left untouched and reported as skipped. Nothing is ever deleted,
/// moved or overwritten; only empty directories are replaced by a link.
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
        Create(Vec<PathBuf>),
    }
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
            Ok(m) if m.is_dir() => match empty_directory_tree(&link)? {
                Some(directories) => plans.push((name.to_owned(), Plan::Create(directories))),
                None => skipped.push(name.to_owned()),
            },
            Ok(_) => skipped.push(name.to_owned()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                plans.push((name.to_owned(), Plan::Create(Vec::new())));
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

    let mut linked = Vec::new();
    for (name, plan) in plans {
        let link = roms.join(&name);
        if let Plan::Create(directories) = plan {
            // Only empty ordinary directories. remove_dir refuses a concurrent
            // new file. No recursive deletion, backups, or migration.
            for directory in directories.into_iter().rev() {
                let _parent_guard = hold_save_directory_chain(
                    directory.parent().context("ROM directory has no parent")?,
                )?;
                ensure_no_reparse_components(&directory)?;
                fs::remove_dir(directory)?;
            }
            let target = expected.managed_root.join(&name);
            #[cfg(windows)]
            std::os::windows::fs::symlink_dir(&target, &link).context(
                "create RetroBat ROM directory link (Windows Developer Mode or link privilege is required)",
            )?;
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
        legacy_whole_link: false,
    })
}
