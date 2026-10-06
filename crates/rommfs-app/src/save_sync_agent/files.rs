use rommfs_core::error::{Error, Result};
use rommfs_core::save_sync::{
    ensure_no_reparse_components, hold_save_directory_chain, path_is_reparse_point,
    sha256_content_hash, validate_windows_path_component, SaveSyncScope, MAX_SAVE_BYTES,
};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

/// Check directory create permissions without creating a preview/probe file.
#[cfg(windows)]
pub(crate) fn check_save_root_writable(root: &Path) -> Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ADD_FILE, FILE_ADD_SUBDIRECTORY, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT,
    };

    for target in [root.to_path_buf(), root.join("gb")] {
        ensure_no_reparse_components(&target)?;
        let directory = target
            .ancestors()
            .find(|path| path.exists())
            .ok_or_else(|| Error::Unsupported("save root has no existing ancestor".into()))?;
        let handle = OpenOptions::new()
            .access_mode(FILE_ADD_FILE | FILE_ADD_SUBDIRECTORY)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(directory)
            .map_err(|error| {
                Error::Unsupported(format!(
                    "Save directory is not writable: {}: {error}",
                    directory.display()
                ))
            })?;
        let metadata = handle.metadata()?;
        if !metadata.is_dir() || path_is_reparse_point(&metadata) {
            return Err(Error::Unsupported(
                "save root is not a plain directory".into(),
            ));
        }
    }
    Ok(())
}

/// Copy verified bytes to a flushed sibling and atomically create `target`
/// without ever replacing an emulator-created save.
pub(super) fn publish_missing_save(
    staged: &Path,
    target: &Path,
    expected_hash: &str,
    approved_root: &Path,
    validate_before_publish: impl Fn() -> Result<()>,
) -> Result<()> {
    publish_no_clobber(staged, target, expected_hash, || {
        validate_save_destination(target, approved_root)?;
        validate_before_publish()
    })
}

pub(super) fn export_incoming(
    staged: &Path,
    destination: &Path,
    expected_hash: &str,
    scope: &SaveSyncScope,
    mapped_targets: &[PathBuf],
) -> Result<()> {
    validate_destination_syntax(destination)?;
    publish_no_clobber(staged, destination, expected_hash, || {
        validate_export_destination(destination, scope, mapped_targets)
    })
}

fn publish_no_clobber(
    staged: &Path,
    destination: &Path,
    expected_hash: &str,
    validate: impl Fn() -> Result<()>,
) -> Result<()> {
    validate()?;
    let parent = destination
        .parent()
        .ok_or_else(|| Error::Unsupported("destination has no parent directory".into()))?;
    let _guard = hold_save_directory_chain(parent)?;
    validate()?;
    let sibling = flushed_sibling_copy(staged, destination, expected_hash, &validate)?;

    // Revalidate at the atomic publication boundary, not just before staging.
    if let Err(error) = validate().and_then(|()| ensure_no_reparse_components(parent)) {
        let _ = fs::remove_file(&sibling);
        return Err(error);
    }
    let result = atomic_create_only(&sibling, destination);
    if result.is_err() {
        let _ = fs::remove_file(&sibling);
    }
    result?;
    sync_parent(destination)?;
    Ok(())
}

fn validate_save_destination(destination: &Path, approved_root: &Path) -> Result<()> {
    validate_destination_syntax(destination)?;
    let parent = destination
        .parent()
        .ok_or_else(|| Error::Unsupported("save destination has no parent directory".into()))?;
    ensure_no_reparse_components(approved_root)?;
    ensure_no_reparse_components(parent)?;
    let canonical_root = fs::canonicalize(approved_root)?;
    let canonical_parent = fs::canonicalize(parent)?;
    let target = canonical_parent.join(
        destination
            .file_name()
            .ok_or_else(|| Error::Unsupported("save destination has no filename".into()))?,
    );
    if !path_is_within(&target, &canonical_root) {
        return Err(Error::Unsupported(
            "save publication target is outside the approved RetroBat root".into(),
        ));
    }
    Ok(())
}

fn validate_export_destination(
    destination: &Path,
    scope: &SaveSyncScope,
    mapped_targets: &[PathBuf],
) -> Result<()> {
    validate_destination_syntax(destination)?;
    let parent = destination
        .parent()
        .ok_or_else(|| Error::Unsupported("incoming export has no parent directory".into()))?;
    ensure_no_reparse_components(parent)?;
    let canonical_parent = fs::canonicalize(parent)?;
    let canonical_target =
        canonical_parent.join(destination.file_name().ok_or_else(|| {
            Error::Unsupported("incoming export destination has no filename".into())
        })?);
    ensure_no_reparse_components(&scope.effective_saves_root)?;
    let save_root = fs::canonicalize(&scope.effective_saves_root)?;
    if path_is_within(&canonical_target, &save_root)
        || mapped_targets.iter().any(|mapped| {
            same_path(&canonical_target, mapped)
                || fs::canonicalize(mapped.parent().unwrap_or(mapped))
                    .ok()
                    .is_some_and(|parent| {
                        same_path(
                            &canonical_target,
                            &parent.join(mapped.file_name().unwrap_or_default()),
                        )
                    })
        })
    {
        return Err(Error::Unsupported(
            "incoming export must be separate from the active RetroBat save profile".into(),
        ));
    }
    Ok(())
}

fn validate_destination_syntax(destination: &Path) -> Result<()> {
    if !destination.is_absolute() {
        return Err(Error::Unsupported(
            "destination must be an absolute filesystem path".into(),
        ));
    }
    for component in destination.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {}
            Component::Normal(name) => {
                let name = name.to_str().ok_or_else(|| {
                    Error::Unsupported("destination path is not valid Unicode".into())
                })?;
                validate_windows_path_component(name).map_err(Error::Unsupported)?;
            }
            Component::CurDir | Component::ParentDir => {
                return Err(Error::Unsupported(
                    "destination path cannot contain traversal components".into(),
                ));
            }
        }
    }
    if destination.file_name().is_none() {
        return Err(Error::Unsupported(
            "destination path must name a file".into(),
        ));
    }
    Ok(())
}

fn flushed_sibling_copy(
    source: &Path,
    destination: &Path,
    expected_hash: &str,
    validate: &impl Fn() -> Result<()>,
) -> Result<PathBuf> {
    let parent = destination
        .parent()
        .ok_or_else(|| Error::Unsupported("destination has no parent directory".into()))?;
    validate()?;
    ensure_no_reparse_components(parent)?;

    let source_metadata = fs::symlink_metadata(source)?;
    if !source_metadata.is_file() || path_is_reparse_point(&source_metadata) {
        return Err(Error::Unsupported(
            "private incoming stage is not a plain file".into(),
        ));
    }
    let input = File::open(source)?;
    let metadata = input.metadata()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_SAVE_BYTES {
        return Err(Error::Unsupported(
            "private incoming stage has an invalid size".into(),
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    input.take(MAX_SAVE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.is_empty()
        || bytes.len() as u64 > MAX_SAVE_BYTES
        || sha256_content_hash(&bytes) != expected_hash
    {
        return Err(Error::Unsupported(
            "private incoming stage failed size or SHA-256 verification".into(),
        ));
    }

    // Check the approved root and every existing parent again immediately
    // before creating any sibling file at the user-selected destination.
    validate()?;
    ensure_no_reparse_components(parent)?;
    let name = destination
        .file_name()
        .ok_or_else(|| Error::Unsupported("destination has no filename".into()))?
        .to_string_lossy();
    let sibling = parent.join(format!(".{name}.rommfs-{}.tmp", Uuid::new_v4()));
    let mut file = private_create_new(&sibling)?;
    let write_result = (|| {
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok::<_, Error>(())
    })();
    drop(file);
    if let Err(error) = write_result {
        let _ = fs::remove_file(&sibling);
        return Err(error);
    }
    Ok(sibling)
}

fn private_create_new(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

fn atomic_create_only(source: &Path, destination: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

        let source_wide = source
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let destination_wide = destination
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        // SAFETY: both paths are NUL-terminated UTF-16 buffers alive for the
        // call. Flags are zero: no replace-existing or cross-volume copy.
        let moved = unsafe { MoveFileExW(source_wide.as_ptr(), destination_wide.as_ptr(), 0) };
        if moved == 0 {
            return Err(Error::Io(std::io::Error::last_os_error()));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        // Same-volume hard links provide atomic no-clobber publication.
        fs::hard_link(source, destination)?;
        fs::remove_file(source)?;
        Ok(())
    }
}

fn sync_parent(path: &Path) -> Result<()> {
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    let path = normalize_windows_path(path);
    let root = normalize_windows_path(root);
    path == root || path.starts_with(&(root + "\\"))
}

fn same_path(left: &Path, right: &Path) -> bool {
    normalize_windows_path(left) == normalize_windows_path(right)
}

fn normalize_windows_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    #[cfg(windows)]
    #[test]
    fn windows_save_root_access_check_rejects_denied_create_permissions_without_writing() {
        use std::process::Command;

        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("saves");
        check_save_root_writable(&root).unwrap();
        assert!(!root.exists());
        fs::create_dir_all(root.join("gb")).unwrap();
        check_save_root_writable(&root).unwrap();

        let identity = Command::new("whoami")
            .args(["/user", "/fo", "csv", "/nh"])
            .output()
            .unwrap();
        assert!(identity.status.success());
        let output = String::from_utf8(identity.stdout).unwrap();
        let sid = output.trim().split(',').nth(1).unwrap().trim_matches('"');
        let denied = Command::new("icacls")
            .arg(&root)
            .args(["/deny", &format!("*{sid}:(WD,AD)")])
            .output()
            .unwrap();
        assert!(denied.status.success(), "{denied:?}");
        let result = check_save_root_writable(&root);
        let restored = Command::new("icacls")
            .arg(&root)
            .args(["/remove:d", &format!("*{sid}")])
            .output()
            .unwrap();
        assert!(restored.status.success(), "{restored:?}");
        assert!(result.is_err(), "write-denied directory accepted");
        check_save_root_writable(&root).unwrap();
        assert_eq!(fs::read_dir(root.join("gb")).unwrap().count(), 0);
    }

    fn has_publication_sibling(parent: &Path) -> bool {
        fs::read_dir(parent).unwrap().any(|entry| {
            entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|extension| extension == "tmp")
        })
    }

    #[test]
    fn disabling_before_atomic_publication_keeps_the_stage_and_creates_no_save() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("saves");
        let parent = root.join("gb");
        fs::create_dir_all(&parent).unwrap();
        let staged = directory.path().join("incoming.stage");
        let destination = parent.join("Game.srm");
        fs::write(&staged, b"verified incoming bytes").unwrap();

        let enabled = Arc::new(AtomicBool::new(true));
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let publisher = {
            let enabled = Arc::clone(&enabled);
            let staged = staged.clone();
            let destination = destination.clone();
            let root = root.clone();
            thread::spawn(move || {
                let paused = AtomicBool::new(false);
                publish_missing_save(
                    &staged,
                    &destination,
                    &sha256_content_hash(b"verified incoming bytes"),
                    &root,
                    || {
                        if has_publication_sibling(destination.parent().unwrap())
                            && !paused.swap(true, Ordering::SeqCst)
                        {
                            ready_tx.send(()).unwrap();
                            release_rx.recv().unwrap();
                        }
                        if !enabled.load(Ordering::SeqCst) {
                            return Err(Error::Cancelled);
                        }
                        Ok(())
                    },
                )
            })
        };

        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(!destination.exists());
        enabled.store(false, Ordering::SeqCst);
        release_tx.send(()).unwrap();

        assert!(publisher.join().unwrap().is_err());
        assert!(!destination.exists());
        assert_eq!(fs::read(&staged).unwrap(), b"verified incoming bytes");
        assert!(!has_publication_sibling(&parent));
    }

    #[cfg(windows)]
    #[test]
    fn windows_publication_does_not_replace_a_file_created_at_the_boundary() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("saves");
        let parent = root.join("gb");
        fs::create_dir_all(&parent).unwrap();
        let staged = directory.path().join("incoming.stage");
        let destination = parent.join("Game.srm");
        fs::write(&staged, b"incoming bytes").unwrap();

        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let publisher = {
            let staged = staged.clone();
            let destination = destination.clone();
            let root = root.clone();
            thread::spawn(move || {
                let paused = AtomicBool::new(false);
                publish_missing_save(
                    &staged,
                    &destination,
                    &sha256_content_hash(b"incoming bytes"),
                    &root,
                    || {
                        if has_publication_sibling(destination.parent().unwrap())
                            && !paused.swap(true, Ordering::SeqCst)
                        {
                            ready_tx.send(()).unwrap();
                            release_rx.recv().unwrap();
                        }
                        Ok(())
                    },
                )
            })
        };

        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        fs::write(&destination, b"emulator-created bytes").unwrap();
        release_tx.send(()).unwrap();

        assert!(publisher.join().unwrap().is_err());
        assert_eq!(fs::read(&destination).unwrap(), b"emulator-created bytes");
        assert_eq!(fs::read(&staged).unwrap(), b"incoming bytes");
        assert!(!has_publication_sibling(&parent));
    }

    #[cfg(windows)]
    #[test]
    fn windows_canonicalized_save_paths_publish_without_replacing() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("saves");
        let parent = root.join("gb");
        fs::create_dir_all(&parent).unwrap();
        let canonical_root = fs::canonicalize(&root).unwrap();
        let canonical_parent = fs::canonicalize(&parent).unwrap();
        let staged = directory.path().join("incoming.stage");
        let destination = canonical_parent.join("Game.srm");
        fs::write(&staged, b"canonicalized save bytes").unwrap();

        publish_missing_save(
            &staged,
            &destination,
            &sha256_content_hash(b"canonicalized save bytes"),
            &canonical_root,
            || Ok(()),
        )
        .unwrap();

        assert_eq!(fs::read(&destination).unwrap(), b"canonicalized save bytes");

        let replacement = directory.path().join("replacement.stage");
        fs::write(&replacement, b"replacement bytes").unwrap();
        assert!(publish_missing_save(
            &replacement,
            &destination,
            &sha256_content_hash(b"replacement bytes"),
            &canonical_root,
            || Ok(()),
        )
        .is_err());
        assert_eq!(fs::read(&destination).unwrap(), b"canonicalized save bytes");
        assert!(!has_publication_sibling(&canonical_parent));
    }
}
