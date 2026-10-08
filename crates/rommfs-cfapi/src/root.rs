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
    crate::imp::check_root_directory(root)
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
/// is explicitly claimed by path/role. Unowned user content is always refused.
pub fn check_mount_root(root: &Path, server_id: &str) -> anyhow::Result<RootCheck> {
    check_directory(root)?;
    let manifest = match read_manifest(root) {
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
    #[cfg(windows)]
    check_tree(root, &manifest, crate::imp::verify_owned)?;
    #[cfg(not(windows))]
    check_tree(root, &manifest, |path, entry| {
        if entry.is_local() {
            verify_local(path, entry)
        } else {
            anyhow::bail!("persistent placeholder validation requires Windows")
        }
    })?;
    Ok(RootCheck::RecognizedOwned)
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

/// Read only previously claimed generated files; no filename-based acquisition
/// of unrelated gamelists or state. Missing local files can be regenerated.
pub fn read_frontend_metadata(
    root: &Path,
    server: &str,
) -> anyhow::Result<(
    rommfs_core::retrobat::MetadataIndex,
    std::collections::BTreeMap<String, String>,
)> {
    check_mount_root(root, server)?;
    let manifest = read_manifest(root)?;
    let mut index = rommfs_core::retrobat::MetadataIndex::default();
    let mut index_found = false;
    let mut gamelists = std::collections::BTreeMap::new();
    for entry in manifest
        .entries
        .iter()
        .filter(|e| matches!(e.kind, OwnedKind::Gamelist | OwnedKind::FrontendState))
    {
        let path = root.join(&entry.path);
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
            index = serde_json::from_slice(&bytes).context("invalid frontend identity index")?;
            index_found = true;
        }
    }
    anyhow::ensure!(
        index_found || gamelists.is_empty(),
        "frontend identity index is missing; existing gamelists were preserved"
    );
    Ok((index, gamelists))
}

/// Publish ordinary gamelists/artwork while the caller holds the stopped
/// frontend guard. Persist captured state first, XML next, then commit its path
/// index. Native ROM cleanup never removes these files.
pub fn publish_frontend_metadata(
    root: &Path,
    server: &str,
    publication: &rommfs_core::retrobat::MetadataPublication,
    artwork: &std::collections::BTreeMap<String, PathBuf>,
) -> anyhow::Result<()> {
    check_mount_root(root, server)?;
    let mut manifest = read_manifest(root)?;
    let index_path = ".rommfs-metadata/index.json";
    publish_local(
        root,
        &mut manifest,
        index_path,
        OwnedKind::FrontendState,
        &serde_json::to_vec(&publication.index().staged())?,
    )?;
    for (path, source) in artwork {
        rommfs_core::save_sync::ensure_no_reparse_components(source)?;
        anyhow::ensure!(
            fs::metadata(source)?.len() <= 32 * 1024 * 1024,
            "oversized cached artwork"
        );
        publish_local(
            root,
            &mut manifest,
            path,
            OwnedKind::Artwork,
            &fs::read(source)?,
        )?;
    }
    for (path, bytes) in publication.gamelists() {
        publish_local(root, &mut manifest, path, OwnedKind::Gamelist, bytes)?;
    }
    publish_local(
        root,
        &mut manifest,
        index_path,
        OwnedKind::FrontendState,
        &serde_json::to_vec(publication.index())?,
    )?;
    Ok(())
}

fn publish_local(
    root: &Path,
    manifest: &mut Manifest,
    relative: &str,
    kind: OwnedKind,
    bytes: &[u8],
) -> anyhow::Result<()> {
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
        crate::imp::make_directory_writable(&target)?;
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
        anyhow::ensure!(
            fs::symlink_metadata(&target).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
            "unowned metadata file was preserved: {}",
            target.display()
        );
        manifest.entries.push(local_entry(relative, kind, false));
        write_manifest(root, manifest)?;
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
    staged.write_all(bytes)?;
    staged.sync_all()?;
    drop(staged);
    #[cfg(windows)]
    crate::imp::make_local_writable(&staging)?;
    if let Ok(metadata) = fs::symlink_metadata(&target) {
        anyhow::ensure!(
            metadata.is_file() && !rommfs_core::save_sync::path_is_reparse_point(&metadata),
            "redirected metadata file was preserved"
        );
    }
    fs::rename(staging, &target)?;
    #[cfg(windows)]
    crate::imp::make_local_writable(&target)?;
    Ok(())
}

fn hold_metadata_directories(root: &Path, directory: &Path) -> anyhow::Result<Vec<fs::File>> {
    #[cfg(windows)]
    {
        crate::imp::hold_local_directory_chain(root, directory)
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

pub(crate) fn check_tree(
    root: &Path,
    manifest: &Manifest,
    verify: impl Fn(&Path, &OwnedEntry) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let owned: HashMap<_, _> = manifest
        .entries
        .iter()
        .map(|e| (e.path.to_lowercase(), e))
        .collect();
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            let relative = path
                .strip_prefix(root)?
                .to_str()
                .context("invalid path encoding")?
                .replace('\\', "/");
            let expected = owned.get(&relative.to_lowercase()).with_context(|| {
                format!(
                    "unowned file {}; preserve it and choose a fresh root",
                    path.display()
                )
            })?;
            verify(&path, expected)?;
            if expected.directory {
                directories.push(path);
            }
        }
    }
    Ok(())
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
