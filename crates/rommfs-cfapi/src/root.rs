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
            parts.len() == if entry.directory { 1 } else { 2 },
            "invalid owned path"
        );
        for part in parts {
            validate_windows_path_component(part).map_err(anyhow::Error::msg)?;
        }
        anyhow::ensure!(entry.identity.len() == 32, "invalid placeholder identity");
        anyhow::ensure!(
            paths
                .insert(entry.path.to_lowercase(), entry.directory)
                .is_none(),
            "duplicate owned path"
        );
    }
    for entry in manifest.entries.iter().filter(|e| !e.directory) {
        let parent = entry.path.split('/').next().context("missing parent")?;
        anyhow::ensure!(
            paths.get(&parent.to_lowercase()) == Some(&true),
            "missing owned directory"
        );
    }
    Ok(())
}

#[cfg(windows)]
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

/// A manifest alone never authorizes replacing ordinary user files.
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
    check_tree(root, &manifest, |_, _| {
        anyhow::bail!("persistent placeholder validation requires Windows")
    })?;
    Ok(RootCheck::RecognizedOwned)
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
