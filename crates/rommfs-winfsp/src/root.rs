use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Whether this empty mount directory already belongs to this server.
#[derive(Debug)]
pub enum RootCheck {
    EmptyReady,
    RecognizedOwned,
}

fn marker_path(root: &Path) -> anyhow::Result<PathBuf> {
    let mut name = root
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("choose a mount directory below a volume root"))?
        .to_os_string();
    name.push(".rommfs-root");
    Ok(root.with_file_name(name))
}

/// Refuse links and nonempty directories, including legacy hydrated roots.
/// The ownership marker lives beside the directory so the mount stays empty.
pub fn check_mount_root(root: &Path, server_id: &str) -> anyhow::Result<RootCheck> {
    check_empty_root(root)?;
    let marker = marker_path(root)?;
    match fs::read_to_string(&marker) {
        Ok(owner) => {
            anyhow::ensure!(
                owner == server_id,
                "mount root belongs to a different server"
            );
            Ok(RootCheck::RecognizedOwned)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(RootCheck::EmptyReady),
        Err(e) => Err(e.into()),
    }
}

pub(crate) fn check_empty_root(root: &Path) -> anyhow::Result<()> {
    let meta = fs::symlink_metadata(root)?;
    anyhow::ensure!(
        meta.is_dir() && !meta.file_type().is_symlink(),
        "mount root {} must be an ordinary directory",
        root.display()
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        anyhow::ensure!(
            meta.file_attributes() & 0x400 == 0,
            "mount root must not be a reparse point"
        );
    }
    anyhow::ensure!(
        fs::read_dir(root)?.next().is_none(),
        "mount root {} is not empty; choose a fresh empty directory and preserve existing files",
        root.display()
    );
    Ok(())
}

/// Claim an empty directory without replacing an existing ownership marker.
pub fn claim_mount_root(root: &Path, server_id: &str) -> anyhow::Result<()> {
    if matches!(
        check_mount_root(root, server_id)?,
        RootCheck::RecognizedOwned
    ) {
        return Ok(());
    }
    let mut marker = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(marker_path(root)?)?;
    marker.write_all(server_id.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ownership_never_authorizes_hiding_or_removing_local_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("roms");
        fs::create_dir(&root).unwrap();
        claim_mount_root(&root, "server-a").unwrap();
        assert!(matches!(
            check_mount_root(&root, "server-a").unwrap(),
            RootCheck::RecognizedOwned
        ));
        assert!(check_mount_root(&root, "server-b").is_err());
        assert!(claim_mount_root(&root, "server-b").is_err());
        fs::write(root.join("save.srm"), b"user data").unwrap();
        assert!(check_mount_root(&root, "server-a").is_err());
        assert_eq!(fs::read(root.join("save.srm")).unwrap(), b"user data");
    }
    #[test]
    fn legacy_roots_are_preserved_and_refused() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join(".rommfs-root"), "server-a").unwrap();
        fs::write(tmp.path().join("local.srm"), b"save").unwrap();
        assert!(claim_mount_root(tmp.path(), "server-a").is_err());
        assert_eq!(fs::read(tmp.path().join("local.srm")).unwrap(), b"save");
    }

    #[cfg(unix)]
    #[test]
    fn linked_roots_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        let link = tmp.path().join("link");
        fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(check_mount_root(&link, "server-a").is_err());
        assert!(claim_mount_root(&link, "server-a").is_err());
        assert_eq!(fs::read_dir(target).unwrap().count(), 0);
    }
}
