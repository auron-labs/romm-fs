use anyhow::Context;
use rommfs_core::save_sync::validate_windows_path_component;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum RootCheck {
    EmptyReady,
    RecognizedOwned,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct OwnedEntry {
    pub path: String,
    pub identity: Vec<u8>,
    pub directory: bool,
    pub size: u64,
    #[serde(default)]
    pub kind: OwnedKind,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) enum OwnedKind {
    #[default]
    Placeholder,
    ManagedDirectory,
    Gamelist,
    Artwork,
    FrontendState,
}

impl OwnedEntry {
    pub(crate) fn is_local(&self) -> bool {
        self.kind != OwnedKind::Placeholder
    }
}

#[derive(Default, Deserialize, Serialize)]
pub(crate) struct Manifest {
    pub server_id: String,
    pub entries: Vec<OwnedEntry>,
    #[serde(default)]
    pub root_security: String,
}

pub(crate) fn sibling(root: &Path, suffix: &str) -> anyhow::Result<PathBuf> {
    let mut name = root
        .file_name()
        .context("choose a directory below a volume root")?
        .to_os_string();
    name.push(suffix);
    Ok(root.with_file_name(name))
}

/// Resolve the root's canonical path so a claim made through one spelling
/// (an 8.3 alias, a junction) is found by callers using another; the mount
/// backend always canonicalizes before reading.
fn canonical_root(root: &Path) -> PathBuf {
    std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())
}

/// Per-user sidecar directory for roots whose parent rejects new files
/// (e.g. `C:\`, where standard users may create directories but not files).
/// Keyed by the canonical mount path so distinct roots never share entries.
fn state_dir(root: &Path) -> anyhow::Result<PathBuf> {
    let canonical = canonical_root(root);
    let mut key = canonical.to_string_lossy().replace('/', "\\");
    key.make_ascii_lowercase();
    let hash = Sha256::digest(key.as_bytes());
    let mut tag = String::with_capacity(16);
    for byte in &hash[..8] {
        tag.push_str(&format!("{byte:02x}"));
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(PathBuf::from))
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
        })
        .unwrap_or_else(std::env::temp_dir);
    Ok(base.join("rommfs").join("mounts").join(tag))
}

fn fallback_sidecar(root: &Path, suffix: &str) -> anyhow::Result<PathBuf> {
    let mut name = root
        .file_name()
        .context("choose a directory below a volume root")?
        .to_os_string();
    name.push(suffix);
    Ok(state_dir(root)?.join(name))
}

/// Sidecars live beside the mount root when its parent accepts new files and
/// under the per-user state store otherwise. An existing file wins either way
/// so a claim survives the parent's permissions changing between mounts. The
/// canonical root is used throughout so the input path's spelling — an 8.3
/// short name or a junction — cannot make claim and mount disagree.
pub(crate) fn sidecar_path(root: &Path, suffix: &str) -> anyhow::Result<PathBuf> {
    let root = canonical_root(root);
    let primary = sibling(&root, suffix)?;
    if primary.try_exists()? {
        return Ok(primary);
    }
    let fallback = fallback_sidecar(&root, suffix)?;
    if fallback.try_exists()? {
        return Ok(fallback);
    }
    let parent = primary.parent().context("missing parent")?;
    match tempfile::NamedTempFile::new_in(parent) {
        Ok(_) => Ok(primary),
        Err(_) => Ok(fallback),
    }
}

/// Parent directory of the marker may not exist yet for fallback paths.
fn ensure_parent(path: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(path.parent().context("missing parent")?)?;
    Ok(())
}

pub(crate) fn read_manifest(root: &Path) -> anyhow::Result<Manifest> {
    let marker = sidecar_path(root, ".rommfs-root")?;
    let bytes = fs::read(&marker)
        .with_context(|| format!("read ownership manifest {}", marker.display()))?;
    // Old WinFsp markers identify a server, but authorize no persistent files.
    let manifest: Manifest = if bytes.first() == Some(&b'{') {
        serde_json::from_slice(&bytes).context("invalid RomMFS ownership manifest")?
    } else {
        Manifest {
            server_id: String::from_utf8(bytes)?,
            ..Manifest::default()
        }
    };
    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn validate_manifest(manifest: &Manifest) -> anyhow::Result<()> {
    let mut paths = HashMap::new();
    for entry in &manifest.entries {
        let parts: Vec<_> = entry.path.split('/').collect();
        anyhow::ensure!(
            !parts.is_empty() && (entry.directory || parts.len() >= 2),
            "invalid owned path"
        );
        for part in parts {
            validate_windows_path_component(part).map_err(anyhow::Error::msg)?;
        }
        anyhow::ensure!(entry.identity.len() == 32, "invalid placeholder identity");
        anyhow::ensure!(
            (entry.kind == OwnedKind::ManagedDirectory) == (entry.is_local() && entry.directory),
            "invalid ordinary entry kind"
        );
        anyhow::ensure!(
            paths
                .insert(entry.path.to_lowercase(), entry.directory)
                .is_none(),
            "duplicate owned path"
        );
    }
    for entry in &manifest.entries {
        let Some((parent, _)) = entry.path.rsplit_once('/') else {
            continue;
        };
        anyhow::ensure!(
            paths.get(&parent.to_lowercase()) == Some(&true),
            "missing owned directory"
        );
    }
    Ok(())
}

pub(crate) fn write_manifest(root: &Path, manifest: &Manifest) -> anyhow::Result<()> {
    validate_manifest(manifest)?;
    let marker = sidecar_path(root, ".rommfs-root")?;
    ensure_parent(&marker)?;
    let mut file = tempfile::NamedTempFile::new_in(marker.parent().context("missing parent")?)?;
    serde_json::to_writer(file.as_file_mut(), manifest)?;
    file.as_file().sync_all()?;
    file.persist(marker)?;
    Ok(())
}

pub(crate) fn check_directory(root: &Path) -> anyhow::Result<()> {
    let parent = root
        .parent()
        .context("choose a directory below a volume root")?;
    let _guard = rommfs_core::save_sync::hold_save_directory_chain(parent)
        .with_context(|| format!("protect mount parent {}", parent.display()))?;
    #[cfg(windows)]
    crate::windows::check_root_directory(root)
        .with_context(|| format!("inspect mount directory {}", root.display()))?;
    #[cfg(not(windows))]
    {
        let meta = fs::symlink_metadata(root)?;
        anyhow::ensure!(
            meta.is_dir() && !meta.file_type().is_symlink(),
            "mount root must be an ordinary directory"
        );
    }
    Ok(())
}

/// Placeholders require native identity verification; writable frontend content
/// is explicitly claimed by path/role. Files a frontend legitimately writes
/// between mounts (its gamelist) are adopted; everything else foreign is
/// reported as `UnownedEntries` for targeted cleanup, never silently claimed.
pub fn check_mount_root(root: &Path, server_id: &str) -> anyhow::Result<RootCheck> {
    check_directory(root)?;
    let mut manifest = match read_manifest(root) {
        Ok(manifest) => manifest,
        Err(e)
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            anyhow::ensure!(
                fs::read_dir(root)?.next().is_none(),
                "root is not empty; preserve existing files and choose a fresh directory"
            );
            return Ok(RootCheck::EmptyReady);
        }
        Err(e) => return Err(e),
    };
    anyhow::ensure!(
        manifest.server_id == server_id,
        "mount root belongs to a different server"
    );
    let scan = scan_tree(root, &manifest, verify_entry)?;
    let unowned = adopt_entries(root, &mut manifest, scan.unowned)?;
    if !unowned.is_empty() {
        let mut error = anyhow::Error::new(UnownedEntries(unowned));
        if !scan.failures.is_empty() {
            error = error.context(format!(
                "{} owned entries also failed verification",
                scan.failures.len()
            ));
        }
        return Err(error);
    }
    anyhow::ensure!(
        scan.failures.is_empty(),
        "owned entries failed verification: {}",
        scan.failures.join("; ")
    );
    Ok(RootCheck::RecognizedOwned)
}

/// Validate ownership and hold the backend-managed root for RetroBat setup.
pub fn hold_mount_root(root: &Path, server_id: &str) -> anyhow::Result<Vec<fs::File>> {
    let guards = hold_metadata_directories(root, root)?;
    check_mount_root(root, server_id)?;
    Ok(guards)
}

/// Writable ordinary files are owned by path/role, not by a content digest:
/// frontend edits are expected. Links and object-type substitution are not.
pub(crate) fn verify_local(path: &Path, entry: &OwnedEntry) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        !rommfs_core::save_sync::path_is_reparse_point(&metadata)
            && metadata.is_dir() == entry.directory
            && (entry.directory || metadata.is_file()),
        "ordinary frontend entry changed type or is redirected: {}",
        path.display()
    );
    Ok(())
}

fn verify_entry(path: &Path, entry: &OwnedEntry) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        crate::windows::verify_owned(path, entry)
    }
    #[cfg(not(windows))]
    {
        anyhow::ensure!(
            entry.is_local(),
            "persistent placeholder validation requires Windows"
        );
        verify_local(path, entry)
    }
}

/// Read the identity index and any gamelists an earlier build published in the
/// managed folder (only read now, for frontend fields to carry over). Only
/// previously claimed files are read; a missing index starts a default one.
pub fn read_frontend_metadata(
    root: &Path,
    server: &str,
) -> anyhow::Result<(
    rommfs_core::retrobat::MetadataIndex,
    std::collections::BTreeMap<String, String>,
)> {
    FrontendMetadata::begin(root, server)?.read()
}

/// One metadata pass over a backend-validated mount root. Its manifest is
/// scoped to the pass; each target is still checked immediately before access.
pub struct FrontendMetadata {
    root: PathBuf,
    server: String,
    manifest: Manifest,
}

impl FrontendMetadata {
    pub fn begin(root: &Path, server: &str) -> anyhow::Result<Self> {
        let started = std::time::Instant::now();
        check_mount_root(root, server)?;
        let manifest = read_manifest(root)?;
        tracing::info!(
            elapsed_ms = started.elapsed().as_millis() as u64,
            entries = manifest.entries.len(),
            "frontend metadata root validated"
        );
        Ok(Self {
            root: root.to_path_buf(),
            server: server.to_string(),
            manifest,
        })
    }

    pub fn validate_linked_root(&self, root: &Path) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.manifest.server_id == self.server,
            "metadata root ownership changed"
        );
        anyhow::ensure!(
            fs::canonicalize(root)? == fs::canonicalize(&self.root)?,
            "frontend link targets a different managed root"
        );
        Ok(())
    }

    pub fn read(
        &self,
    ) -> anyhow::Result<(
        rommfs_core::retrobat::MetadataIndex,
        std::collections::BTreeMap<String, String>,
    )> {
        let mut index = rommfs_core::retrobat::MetadataIndex::default();
        let mut gamelists = std::collections::BTreeMap::new();
        for entry in self
            .manifest
            .entries
            .iter()
            .filter(|e| matches!(e.kind, OwnedKind::Gamelist | OwnedKind::FrontendState))
        {
            let path = self.root.join(&entry.path);
            if path.exists() {
                verify_local(&path, entry)?;
            }
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            if entry.kind == OwnedKind::Gamelist {
                let platform = entry
                    .path
                    .strip_suffix("/gamelist.xml")
                    .context("invalid gamelist ownership path")?;
                gamelists.insert(platform.into(), String::from_utf8(bytes)?);
            } else if entry.path == ".rommfs-metadata/index.json" {
                index =
                    serde_json::from_slice(&bytes).context("invalid frontend identity index")?;
            }
        }
        Ok((index, gamelists))
    }

    pub fn publish(
        &mut self,
        index: &rommfs_core::retrobat::MetadataIndex,
        artwork: &std::collections::BTreeMap<String, PathBuf>,
        publish_gamelists: impl FnOnce() -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        self.publish_with(index, artwork, || false, publish_gamelists)
    }

    pub fn publish_with(
        &mut self,
        index: &rommfs_core::retrobat::MetadataIndex,
        artwork: &std::collections::BTreeMap<String, PathBuf>,
        mut cancelled: impl FnMut() -> bool,
        publish_gamelists: impl FnOnce() -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        publish_frontend_metadata_validated(
            &self.root,
            &mut self.manifest,
            index,
            artwork,
            &mut cancelled,
            publish_gamelists,
        )
    }
}

/// Publish artwork and the identity index while the caller holds the stopped
/// frontend guard. The staged index is persisted first, then artwork, then
/// `publish_gamelists` writes RetroBat's gamelists, and the final index commits
/// last. Native ROM cleanup never removes these files.
pub fn publish_frontend_metadata(
    root: &Path,
    server: &str,
    index: &rommfs_core::retrobat::MetadataIndex,
    artwork: &std::collections::BTreeMap<String, PathBuf>,
    publish_gamelists: impl FnOnce() -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    check_mount_root(root, server)?;
    let mut manifest = read_manifest(root)?;
    publish_frontend_metadata_validated(
        root,
        &mut manifest,
        index,
        artwork,
        &mut || false,
        publish_gamelists,
    )
}

fn publish_frontend_metadata_validated(
    root: &Path,
    manifest: &mut Manifest,
    index: &rommfs_core::retrobat::MetadataIndex,
    artwork: &std::collections::BTreeMap<String, PathBuf>,
    cancelled: &mut impl FnMut() -> bool,
    publish_gamelists: impl FnOnce() -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let started = std::time::Instant::now();
    let index_path = ".rommfs-metadata/index.json";
    anyhow::ensure!(!cancelled(), "frontend metadata publication cancelled");
    publish_local(
        root,
        manifest,
        index_path,
        OwnedKind::FrontendState,
        &serde_json::to_vec(&index.staged())?,
        false,
        cancelled,
    )?;
    // Claim the whole artwork batch before writing bytes, not one manifest rewrite per image.
    for path in artwork.keys() {
        anyhow::ensure!(!cancelled(), "frontend metadata publication cancelled");
        publish_local(
            root,
            manifest,
            path,
            OwnedKind::Artwork,
            &[],
            true,
            cancelled,
        )?;
    }
    if !artwork.is_empty() {
        anyhow::ensure!(!cancelled(), "frontend metadata publication cancelled");
        write_manifest(root, manifest)?;
    }
    let mut updated = 0;
    for (path, source) in artwork {
        anyhow::ensure!(!cancelled(), "frontend metadata publication cancelled");
        rommfs_core::save_sync::ensure_no_reparse_components(source)?;
        anyhow::ensure!(
            fs::metadata(source)?.len() <= 32 * 1024 * 1024,
            "oversized cached artwork"
        );
        updated += publish_local(
            root,
            manifest,
            path,
            OwnedKind::Artwork,
            &fs::read(source)?,
            false,
            cancelled,
        )? as usize;
    }
    tracing::info!(
        reused = artwork.len() - updated,
        updated,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "frontend artwork publication complete"
    );
    anyhow::ensure!(!cancelled(), "frontend metadata publication cancelled");
    publish_gamelists()?;
    anyhow::ensure!(!cancelled(), "frontend metadata publication cancelled");
    publish_local(
        root,
        manifest,
        index_path,
        OwnedKind::FrontendState,
        &serde_json::to_vec(index)?,
        false,
        cancelled,
    )?;
    Ok(())
}

fn publish_local(
    root: &Path,
    manifest: &mut Manifest,
    relative: &str,
    kind: OwnedKind,
    bytes: &[u8],
    claim_only: bool,
    cancelled: &mut impl FnMut() -> bool,
) -> anyhow::Result<bool> {
    let _guard = rommfs_core::save_sync::hold_save_directory_chain(
        root.parent().context("missing mount parent")?,
    )?;
    let _root_guard = hold_metadata_directories(root, root)?;
    let parts: Vec<_> = relative.split('/').collect();
    anyhow::ensure!(
        parts.len() >= 2,
        "ordinary content must be below a managed directory"
    );
    for part in &parts {
        validate_windows_path_component(part).map_err(anyhow::Error::msg)?;
    }
    if kind == OwnedKind::Artwork {
        if let Some(entry) = manifest.entries.iter().find(|entry| {
            entry.path.eq_ignore_ascii_case(relative)
                && entry.kind == OwnedKind::Artwork
                && !entry.directory
        }) {
            let target = root.join(relative);
            if target.try_exists()? {
                let _target_guard = hold_metadata_directories(
                    root,
                    target.parent().context("missing artwork parent")?,
                )?;
                verify_local(&target, entry)?;
                if claim_only || fs::read(&target)? == bytes {
                    return Ok(false);
                }
            }
        }
    }
    let mut directory = String::new();
    for part in &parts[..parts.len() - 1] {
        if !directory.is_empty() {
            directory.push('/');
        }
        directory.push_str(part);
        let target = root.join(&directory);
        if let Some(entry) = manifest
            .entries
            .iter()
            .find(|e| e.path.eq_ignore_ascii_case(&directory))
        {
            anyhow::ensure!(entry.directory, "metadata parent collides with a ROM file");
        } else {
            anyhow::ensure!(
                !target.try_exists()? && fs::symlink_metadata(&target).is_err(),
                "unowned metadata directory was preserved: {}",
                target.display()
            );
            manifest
                .entries
                .push(local_entry(&directory, OwnedKind::ManagedDirectory, true));
            anyhow::ensure!(!cancelled(), "frontend metadata publication cancelled");
            write_manifest(root, manifest)?;
        }
        if !target.try_exists()? {
            fs::create_dir(&target)?;
        }
        let entry = manifest
            .entries
            .iter()
            .find(|e| e.path.eq_ignore_ascii_case(&directory))
            .context("missing claimed metadata directory")?;
        if entry.is_local() {
            verify_local(&target, entry)?;
        }
        #[cfg(windows)]
        crate::windows::make_directory_writable(&target)?;
    }
    let target = root.join(relative);
    let _target_guard =
        hold_metadata_directories(root, target.parent().context("missing metadata parent")?)?;
    if let Some(entry) = manifest
        .entries
        .iter()
        .find(|e| e.path.eq_ignore_ascii_case(relative))
    {
        anyhow::ensure!(
            entry.kind == kind && !entry.directory,
            "ordinary content collides with another ownership role"
        );
        if target.try_exists()? {
            verify_local(&target, entry)?;
        }
    } else {
        // A frontend-owned file the frontend created between mounts (its own
        // gamelist.xml) is claimed and overwritten like any owned publication;
        // anything else unowned is preserved and still refuses.
        let adoptable = adoptable_kind(
            manifest,
            &UnownedEntry {
                path: relative.to_string(),
                kind: UnownedKind::File,
            },
        )
        .is_some_and(|claimed| {
            claimed == kind && verify_local(&target, &local_entry(relative, kind, false)).is_ok()
        });
        if !adoptable {
            anyhow::ensure!(
                fs::symlink_metadata(&target)
                    .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
                "unowned metadata file was preserved: {}",
                target.display()
            );
        }
        manifest.entries.push(local_entry(relative, kind, false));
        if !claim_only {
            anyhow::ensure!(!cancelled(), "frontend metadata publication cancelled");
            write_manifest(root, manifest)?;
        }
    }
    if claim_only {
        return Ok(false);
    }
    // A named, claimed staging file makes interruption recoverable even when
    // the volume root refuses sibling files (e.g. a standard user's C:\ROMs).
    // The first publication creates this owned metadata directory above.
    let staging_relative = ".rommfs-metadata/publish.tmp";
    let staging = root.join(staging_relative);
    if let Some(entry) = manifest.entries.iter().find(|e| e.path == staging_relative) {
        anyhow::ensure!(
            entry.kind == OwnedKind::FrontendState && !entry.directory,
            "invalid publication staging role"
        );
        if staging.try_exists()? {
            verify_local(&staging, entry)?;
        }
    } else {
        anyhow::ensure!(
            fs::symlink_metadata(&staging).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
            "unowned staging file was preserved"
        );
        manifest.entries.push(local_entry(
            staging_relative,
            OwnedKind::FrontendState,
            false,
        ));
        anyhow::ensure!(!cancelled(), "frontend metadata publication cancelled");
        write_manifest(root, manifest)?;
    }
    let _staging_guard = hold_metadata_directories(
        root,
        staging.parent().context("missing metadata directory")?,
    )?;
    if let Ok(metadata) = fs::symlink_metadata(&staging) {
        anyhow::ensure!(
            metadata.is_file() && !rommfs_core::save_sync::path_is_reparse_point(&metadata),
            "redirected staging file was preserved"
        );
    }
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    let mut staged = options.open(&staging)?;
    anyhow::ensure!(!cancelled(), "frontend metadata publication cancelled");
    staged.write_all(bytes)?;
    staged.sync_all()?;
    drop(staged);
    #[cfg(windows)]
    crate::windows::make_local_writable(&staging)?;
    if let Ok(metadata) = fs::symlink_metadata(&target) {
        anyhow::ensure!(
            metadata.is_file() && !rommfs_core::save_sync::path_is_reparse_point(&metadata),
            "redirected metadata file was preserved"
        );
    }
    anyhow::ensure!(!cancelled(), "frontend metadata publication cancelled");
    fs::rename(staging, &target)?;
    #[cfg(windows)]
    crate::windows::make_local_writable(&target)?;
    Ok(true)
}

fn hold_metadata_directories(root: &Path, directory: &Path) -> anyhow::Result<Vec<fs::File>> {
    #[cfg(windows)]
    {
        crate::windows::hold_local_directory_chain(root, directory)
    }
    #[cfg(not(windows))]
    {
        directory.strip_prefix(root)?;
        rommfs_core::save_sync::ensure_no_reparse_components(directory)?;
        Ok(Vec::new())
    }
}

fn local_entry(path: &str, kind: OwnedKind, directory: bool) -> OwnedEntry {
    OwnedEntry {
        path: path.into(),
        identity: Sha256::digest(format!("RomMFS-local:{kind:?}:{path}")).to_vec(),
        directory,
        size: 0,
        kind,
    }
}

/// What the mount walk found on disk that the manifest does not own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnownedKind {
    File,
    Directory,
    /// A symlink or junction. Cloud and other data-bearing reparse points are
    /// files/directories, never links eligible for automatic cleanup.
    Link,
}

/// One foreign on-disk entry under a mount root, root-relative and
/// '/'-separated so it can be shown to the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnownedEntry {
    pub path: String,
    pub kind: UnownedKind,
}

/// Every unmanaged entry under a mount root. Surfaced as the error type so a
/// caller can offer targeted cleanup instead of a bare refusal.
#[derive(Debug)]
pub struct UnownedEntries(pub Vec<UnownedEntry>);

impl UnownedKind {
    fn label(self) -> &'static str {
        match self {
            UnownedKind::File => "file",
            UnownedKind::Directory => "directory",
            UnownedKind::Link => "link",
        }
    }
}

impl std::fmt::Display for UnownedEntries {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let n = self.0.len();
        write!(
            f,
            "{n} unmanaged entr{} in the mount folder",
            if n == 1 { "y" } else { "ies" }
        )?;
        const SHOWN: usize = 8;
        for entry in self.0.iter().take(SHOWN) {
            write!(f, "; {} ({})", entry.path, entry.kind.label())?;
        }
        if n > SHOWN {
            write!(f, "; and {} more", n - SHOWN)?;
        }
        Ok(())
    }
}

impl std::error::Error for UnownedEntries {}

/// Result of walking a mount root against its manifest.
pub(crate) struct TreeScan {
    /// On-disk entries the manifest does not own; foreign directories are
    /// reported but never descended into.
    pub unowned: Vec<UnownedEntry>,
    /// Owned entries whose on-disk state failed verification.
    pub failures: Vec<String>,
}

/// Walk the whole root instead of stopping at the first problem so one foreign
/// file cannot hide the rest. Links are classified, never followed.
pub(crate) fn scan_tree(
    root: &Path,
    manifest: &Manifest,
    verify: impl Fn(&Path, &OwnedEntry) -> anyhow::Result<()>,
) -> anyhow::Result<TreeScan> {
    let _parent_guard = rommfs_core::save_sync::hold_save_directory_chain(
        root.parent().context("mount root has no parent")?,
    )?;
    check_directory(root)?;
    let _root_guard = hold_metadata_directories(root, root)?;
    let owned: HashMap<_, _> = manifest
        .entries
        .iter()
        .map(|e| (e.path.to_lowercase(), e))
        .collect();
    let mut scan = TreeScan {
        unowned: Vec::new(),
        failures: Vec::new(),
    };
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        let _directory_guard = match hold_metadata_directories(root, &directory) {
            Ok(guard) => guard,
            Err(e) => {
                scan.failures
                    .push(format!("{}: {e:#}", directory.display()));
                continue;
            }
        };
        for entry in fs::read_dir(&directory)? {
            let path = entry?.path();
            let relative = path
                .strip_prefix(root)?
                .to_str()
                .context("invalid path encoding")?
                .replace('\\', "/");
            match owned.get(&relative.to_lowercase()) {
                Some(expected) => {
                    if let Err(e) = verify(&path, expected) {
                        scan.failures.push(format!("{relative}: {e:#}"));
                        continue;
                    }
                    if expected.directory {
                        directories.push(path);
                    }
                }
                None => {
                    let metadata = fs::symlink_metadata(&path)?;
                    let kind = if rommfs_core::save_sync::path_is_symbolic_link(&path)? {
                        UnownedKind::Link
                    } else if metadata.is_dir() {
                        UnownedKind::Directory
                    } else {
                        UnownedKind::File
                    };
                    scan.unowned.push(UnownedEntry {
                        path: relative,
                        kind,
                    });
                }
            }
        }
    }
    scan.unowned.sort_by(|a, b| a.path.cmp(&b.path));
    scan.failures.sort();
    Ok(scan)
}

/// The frontend names a running EmulationStation legitimately creates inside a
/// platform directory between mounts: its `gamelist.xml` plus the `gamelist.*`
/// backups/locks ES writes while saving. Claims stay role-scoped — a link is
/// never adopted, and only empty directories below owned directories join the
/// manifest as managed directories.
fn adoptable_kind(manifest: &Manifest, entry: &UnownedEntry) -> Option<OwnedKind> {
    let (parent, name) = match entry.path.rsplit_once('/') {
        Some((parent, name)) => (Some(parent), name),
        None => (None, entry.path.as_str()),
    };
    match entry.kind {
        UnownedKind::Link => None,
        UnownedKind::Directory => {
            // Only adopt directly below the root or an owned directory; the
            // scan never descends so the caller re-checks emptiness on disk.
            match parent {
                Some(parent) => manifest
                    .entries
                    .iter()
                    .any(|e| e.directory && e.path.eq_ignore_ascii_case(parent))
                    .then_some(OwnedKind::ManagedDirectory),
                None => Some(OwnedKind::ManagedDirectory),
            }
        }
        UnownedKind::File => {
            let owned = parent.is_some_and(|parent| {
                manifest
                    .entries
                    .iter()
                    .any(|e| e.directory && e.path.eq_ignore_ascii_case(parent))
            });
            if !owned {
                return None;
            }
            let name = name.to_lowercase();
            if name == "gamelist.xml" {
                Some(OwnedKind::Gamelist)
            } else {
                name.starts_with("gamelist.xml")
                    .then_some(OwnedKind::FrontendState)
            }
        }
    }
}

/// Move the frontend-owned foreign entries of a scan into the manifest,
/// returning the entries that must stay foreign. Adoption re-verifies the
/// on-disk type at claim time so a file swapped for a link between the scan
/// and the claim is never taken over.
pub(crate) fn adopt_entries(
    root: &Path,
    manifest: &mut Manifest,
    unowned: Vec<UnownedEntry>,
) -> anyhow::Result<Vec<UnownedEntry>> {
    let _parent_guard = rommfs_core::save_sync::hold_save_directory_chain(
        root.parent().context("mount root has no parent")?,
    )?;
    let _root_guard = hold_metadata_directories(root, root)?;
    let mut remaining = Vec::new();
    let before = manifest.entries.len();
    for entry in unowned {
        let directory = entry.kind == UnownedKind::Directory;
        let Some(kind) = adoptable_kind(manifest, &entry) else {
            remaining.push(entry);
            continue;
        };
        let target = root.join(&entry.path);
        let _parent_guard = hold_metadata_directories(
            root,
            target.parent().context("foreign entry has no parent")?,
        )?;
        let owned = local_entry(&entry.path, kind, directory);
        if verify_local(&target, &owned).is_err() {
            remaining.push(entry);
            continue;
        }
        let _directory_guard = if directory {
            Some(hold_metadata_directories(root, &target)?)
        } else {
            None
        };
        // Adopt only directories provably empty; a vanished/unreadable entry
        // stays foreign rather than aborting the remaining claims.
        if directory
            && fs::read_dir(&target)
                .map(|mut d| d.next().is_some())
                .unwrap_or(true)
        {
            remaining.push(entry);
            continue;
        }
        manifest.entries.push(owned);
    }
    if manifest.entries.len() != before {
        write_manifest(root, manifest)?;
    }
    Ok(remaining)
}

/// The foreign entries under a verified `root`, for remediation UI. With no
/// manifest, report the top level; invalid manifests or roots refuse scanning.
pub fn foreign_entries(root: &Path) -> Vec<UnownedEntry> {
    match foreign_tree(root) {
        Ok(scan) => scan.unowned,
        Err(_) => Vec::new(),
    }
}

fn foreign_tree(root: &Path) -> anyhow::Result<TreeScan> {
    check_directory(root)?;
    let manifest = match read_manifest(root) {
        Ok(manifest) => manifest,
        Err(e)
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            Manifest::default()
        }
        Err(e) => return Err(e),
    };
    scan_tree(root, &manifest, verify_entry)
}

/// Delete only foreign symlinks/junctions under `root` — removing a link
/// object never deletes its target's data. Real files and directories are
/// never touched. Returns how many links were removed; per-link failures are
/// aggregated so one stubborn link does not stop the rest.
pub fn remove_foreign_links(root: &Path) -> anyhow::Result<usize> {
    let _parent_guard = rommfs_core::save_sync::hold_save_directory_chain(
        root.parent().context("mount root has no parent")?,
    )?;
    check_directory(root)?;
    let _root_guard = hold_metadata_directories(root, root)?;
    let scan = foreign_tree(root)?;
    anyhow::ensure!(
        scan.failures.is_empty(),
        "owned entries failed verification: {}",
        scan.failures.join("; ")
    );
    let mut removed = 0;
    let mut errors = Vec::new();
    for entry in scan.unowned {
        if entry.kind != UnownedKind::Link {
            continue;
        }
        let path = root.join(&entry.path);
        let result = (|| -> anyhow::Result<()> {
            let _directory_guard = hold_metadata_directories(
                root,
                path.parent().context("foreign link has no parent")?,
            )?;
            rommfs_core::save_sync::remove_symbolic_link(&path)?;
            Ok(())
        })();
        match result {
            Ok(()) => removed += 1,
            Err(e) if e.downcast_ref::<rommfs_core::Error>().is_some_and(|e| {
                matches!(e, rommfs_core::Error::Io(e) if e.kind() == std::io::ErrorKind::NotFound)
            }) => {},
            Err(e) => errors.push(format!("{}: {e}", entry.path)),
        }
    }
    if errors.is_empty() {
        Ok(removed)
    } else {
        Err(anyhow::anyhow!(
            "removed {removed} links; failed on {}: {}",
            errors.len(),
            errors.join("; ")
        ))
    }
}

pub fn claim_mount_root(root: &Path, server_id: &str) -> anyhow::Result<()> {
    if matches!(
        check_mount_root(root, server_id)?,
        RootCheck::RecognizedOwned
    ) {
        return Ok(());
    }
    let marker = sidecar_path(root, ".rommfs-root")?;
    ensure_parent(&marker)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker)
        .with_context(|| format!("create ownership manifest {}", marker.display()))?;
    file.write_all(&serde_json::to_vec(&Manifest {
        server_id: server_id.into(),
        ..Manifest::default()
    })?)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn cleanup_refuses_replaced_owned_directories_and_preserves_external_links() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("roms");
        let outside = tmp.path().join("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        claim_mount_root(&root, "server-a").unwrap();
        let mut manifest = read_manifest(&root).unwrap();
        manifest
            .entries
            .push(local_entry("gb", OwnedKind::ManagedDirectory, true));
        write_manifest(&root, &manifest).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("gb")).unwrap();
        std::os::unix::fs::symlink(tmp.path(), outside.join("keep-link")).unwrap();

        assert!(check_mount_root(&root, "server-a").is_err());
        assert!(foreign_entries(&root).is_empty());
        assert!(remove_foreign_links(&root).is_err());
        assert!(fs::symlink_metadata(outside.join("keep-link")).is_ok());
        // Even a caller supplying a permissive verifier cannot traverse links.
        let scan = scan_tree(&root, &manifest, |_, _| Ok(())).unwrap();
        assert!(scan.unowned.is_empty());
        assert_eq!(scan.failures.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_refuses_linked_roots_and_ancestors() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tmp.path().join("outside");
        let root = tmp.path().join("roms");
        fs::create_dir(&outside).unwrap();
        fs::create_dir(outside.join("roms")).unwrap();
        std::os::unix::fs::symlink(&outside, &root).unwrap();
        std::os::unix::fs::symlink(tmp.path(), outside.join("keep-link")).unwrap();
        std::os::unix::fs::symlink(tmp.path(), outside.join("roms/keep-link")).unwrap();
        for redirected in [&root, &root.join("roms")] {
            assert!(foreign_entries(redirected).is_empty());
            assert!(remove_foreign_links(redirected).is_err());
        }
        assert!(fs::symlink_metadata(outside.join("keep-link")).is_ok());
        assert!(fs::symlink_metadata(outside.join("roms/keep-link")).is_ok());
    }

    #[test]
    fn cleanup_refuses_invalid_manifest_instead_of_treating_owned_data_as_foreign() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("roms");
        fs::create_dir(&root).unwrap();
        claim_mount_root(&root, "s").unwrap();
        fs::write(sidecar_path(&root, ".rommfs-root").unwrap(), b"{broken").unwrap();
        assert!(remove_foreign_links(&root).is_err());
    }

    #[test]
    fn ownership_never_authorizes_replacing_user_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("roms");
        fs::create_dir(&root).unwrap();
        claim_mount_root(&root, "server-a").unwrap();
        assert!(matches!(
            check_mount_root(&root, "server-a").unwrap(),
            RootCheck::RecognizedOwned
        ));
        assert!(check_mount_root(&root, "server-b").is_err());
        fs::write(root.join("save.srm"), b"user data").unwrap();
        assert!(check_mount_root(&root, "server-a").is_err());
        assert_eq!(fs::read(root.join("save.srm")).unwrap(), b"user data");
    }

    #[test]
    fn manifest_paths_cannot_escape_or_alias() {
        for path in [
            "../outside",
            "nes/../outside",
            "/outside",
            "nes/game.nes:stream",
            "nes\\game.nes",
        ] {
            let manifest = Manifest {
                server_id: "s".into(),
                entries: vec![OwnedEntry {
                    path: path.into(),
                    identity: vec![0; 32],
                    directory: false,
                    size: 1,
                    kind: OwnedKind::Placeholder,
                }],
                ..Manifest::default()
            };
            assert!(validate_manifest(&manifest).is_err(), "{path}");
        }
    }

    /// Files a frontend legitimately writes between mounts are adopted into the
    /// manifest; everything else is classified and reported, never touched.
    #[test]
    fn frontend_files_are_adopted_and_foreign_entries_classified() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("roms");
        fs::create_dir(&root).unwrap();
        claim_mount_root(&root, "server-a").unwrap();
        // Seed an owned platform directory (as a mount would have left it).
        fs::create_dir(root.join("gb")).unwrap();
        let mut manifest = read_manifest(&root).unwrap();
        manifest
            .entries
            .push(local_entry("gb", OwnedKind::ManagedDirectory, true));
        write_manifest(&root, &manifest).unwrap();

        fs::write(root.join("gb/gamelist.xml"), b"<gameList/>").unwrap();
        fs::write(root.join("stray.srm"), b"user data").unwrap();
        fs::create_dir(root.join("saves")).unwrap();
        fs::write(root.join("saves/keep.srm"), b"user").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&root, root.join("leftover")).unwrap();

        let error = check_mount_root(&root, "server-a").unwrap_err();
        let report = error
            .downcast_ref::<UnownedEntries>()
            .expect("foreign entries must be classified");
        let paths: Vec<_> = report.0.iter().map(|e| (e.path.as_str(), e.kind)).collect();
        assert!(
            paths.contains(&("stray.srm", UnownedKind::File)),
            "{paths:?}"
        );
        assert!(
            paths.contains(&("saves", UnownedKind::Directory)),
            "{paths:?}"
        );
        #[cfg(unix)]
        assert!(
            paths.contains(&("leftover", UnownedKind::Link)),
            "{paths:?}"
        );
        assert!(!paths.iter().any(|(path, _)| path.contains("gamelist")));

        // The gamelist was adopted on disk and the manifest persisted it.
        let manifest = read_manifest(&root).unwrap();
        assert!(manifest
            .entries
            .iter()
            .any(|e| e.path == "gb/gamelist.xml" && e.kind == OwnedKind::Gamelist));
        assert_eq!(fs::read(root.join("stray.srm")).unwrap(), b"user data");
        assert_eq!(fs::read(root.join("saves/keep.srm")).unwrap(), b"user");
    }

    /// Link cleanup deletes only symlinks — the foreign file and the
    /// directory stay, and a missing manifest never blocks reporting.
    #[cfg(unix)]
    #[test]
    fn remove_foreign_links_deletes_only_links() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("roms");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("user.txt"), b"mine").unwrap();
        fs::create_dir(root.join("real")).unwrap();
        std::os::unix::fs::symlink(tmp.path(), root.join("dead-link")).unwrap();
        std::os::unix::fs::symlink(tmp.path().join("missing"), root.join("broken")).unwrap();

        let entries = foreign_entries(&root);
        assert_eq!(
            entries
                .iter()
                .filter(|e| e.kind == UnownedKind::Link)
                .count(),
            2
        );
        assert_eq!(remove_foreign_links(&root).unwrap(), 2);
        assert!(fs::symlink_metadata(root.join("dead-link")).is_err());
        assert!(fs::symlink_metadata(root.join("broken")).is_err());
        assert_eq!(fs::read(root.join("user.txt")).unwrap(), b"mine");
        assert!(root.join("real").is_dir());
        assert!(foreign_entries(&root)
            .iter()
            .all(|e| e.kind != UnownedKind::Link));
    }

    #[cfg(unix)]
    #[test]
    fn linked_roots_and_ancestors_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        let link = tmp.path().join("link");
        fs::create_dir(&target).unwrap();
        fs::create_dir(target.join("roms")).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(claim_mount_root(&link, "s").is_err());
        assert!(claim_mount_root(&link.join("roms"), "s").is_err());
    }
}
