use rommfs_core::error::{Error, Result};
#[cfg(test)]
use rommfs_core::save_sync::sha256_content_hash;
use rommfs_core::save_sync::{
    ensure_no_reparse_components, hash_save_content, hold_save_directory_chain,
    path_is_reparse_point, validate_windows_path_component, SaveSyncScope,
};
use std::fs::{self, File, OpenOptions};
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
    let _parents = prepare_save_parent(target, approved_root)?;
    publish_no_clobber(staged, target, expected_hash, || {
        validate_save_destination(target, approved_root)?;
        validate_before_publish()
    })
}

fn prepare_save_parent(
    target: &Path,
    root: &Path,
) -> Result<rommfs_core::save_sync::SaveDirectoryChainGuard> {
    validate_destination_syntax(target)?;
    target.strip_prefix(root).map_err(|_| {
        Error::Unsupported("Save destination is outside the approved folder".into())
    })?;
    let parent = target
        .parent()
        .ok_or_else(|| Error::Unsupported("Save destination has no parent".into()))?;
    ensure_no_reparse_components(parent)?;
    let existing = parent
        .ancestors()
        .find(|p| p.is_dir())
        .ok_or_else(|| Error::Unsupported("Save folder has no existing ancestor".into()))?;
    let mut guard = hold_save_directory_chain(existing)?;
    let mut path = existing.to_path_buf();
    for part in parent
        .strip_prefix(existing)
        .map_err(|_| Error::Unsupported("Save parent is invalid".into()))?
        .components()
    {
        path.push(part.as_os_str());
        match fs::create_dir(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
        guard = hold_save_directory_chain(&path)?;
    }
    Ok(guard)
}

/// Preserve the old baseline at a journaled sibling path, then atomically
/// create the new file without clobbering a concurrent emulator-created file.
/// On Windows the baseline handle denies writers/deletion and is renamed by
/// handle, not by a path that could be exchanged after verification.
#[allow(clippy::too_many_arguments)]
pub(super) fn publish_changed_save(
    staged: &Path,
    target: &Path,
    expected_hash: &str,
    baseline_hash: &str,
    backup: &Path,
    root: &Path,
    validate: impl Fn() -> Result<()>,
) -> Result<()> {
    let _parents = prepare_save_parent(target, root)?;
    validate_save_destination(target, root)?;
    validate()?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            DELETE, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_SHARE_READ,
        };
        options
            .access_mode(FILE_GENERIC_READ | DELETE)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let mut baseline = options.open(target)?;
    let metadata = baseline.metadata()?;
    if !metadata.is_file()
        || path_is_reparse_point(&metadata)
        || hash_save_content(&mut baseline)? != baseline_hash
    {
        return Err(Error::Unsupported(
            "Local save changed or is active; both versions were kept".into(),
        ));
    }
    let sibling = flushed_sibling_copy(staged, target, expected_hash, &|| {
        validate_save_destination(target, root)?;
        validate()
    })?;
    let result = (|| {
        validate()?;
        ensure_no_reparse_components(backup)?;
        if backup.exists() {
            return Err(Error::Unsupported(
                "Save backup already exists; review the previous interrupted replacement".into(),
            ));
        }
        rename_baseline(&baseline, target, backup)?;
        // Linux portable tests cannot establish emulator locks. Rehash the
        // moved baseline as well; a changed file is never discarded.
        use std::io::{Seek, SeekFrom};
        baseline.seek(SeekFrom::Start(0))?;
        if hash_save_content(&mut baseline)? != baseline_hash {
            return Err(Error::Unsupported(format!(
                "Save changed during replacement; its bytes are preserved at {}",
                backup.display()
            )));
        }
        validate()?;
        atomic_create_only(&sibling, target)?;
        sync_parent(target)
    })();
    drop(baseline);
    if result.is_err() {
        let _ = fs::remove_file(&sibling);
        // Restore only into a missing target. Never overwrite a file that
        // appeared during the interruption; preserve the backup for review.
        if backup.exists() && !target.exists() {
            let _ = atomic_create_only(backup, target);
        }
    }
    result
}

fn rename_baseline(file: &File, source: &Path, backup: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FileRenameInfo, SetFileInformationByHandle, FILE_RENAME_INFO,
        };
        // FILE_RENAME_INFO.FileName must be a fully qualified name: a plain
        // file name is resolved relative to the process working directory and
        // a bare `C:\` DOS path is misparsed. `\??\` is the Win32-visible NT
        // namespace prefix that the rename IRP accepts directly. The IRP
        // scans FileName as a NUL-terminated string regardless of
        // FileNameLength, so dwBufferSize must cover the terminator too —
        // otherwise the driver appends garbage read past the buffer.
        // NT-prefixed names bypass DOS separator normalization, so `/`
        // retained by portable slot paths would leave unresolvable
        // components — normalize separators before prefixing.
        let backup_string = backup.as_os_str().to_string_lossy().replace('/', "\\");
        let nt_name = if let Some(rest) = backup_string.strip_prefix(r"\\?\") {
            format!(r"\??\{rest}")
        } else if let Some(rest) = backup_string.strip_prefix(r"\\") {
            format!(r"\??\UNC\{rest}")
        } else {
            format!(r"\??\{backup_string}")
        };
        let name: Vec<u16> = nt_name.encode_utf16().collect();
        let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
        let length = offset + name.len() * 2 + 2;
        let mut buffer = vec![0u64; length.div_ceil(8)];
        let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        // SAFETY: u64 storage supplies FILE_RENAME_INFO alignment and enough
        // initialized bytes for the NUL-terminated trailing UTF-16 filename.
        // The exclusive plain-file handle remains live throughout the call.
        unsafe {
            (*info).Anonymous.ReplaceIfExists = false;
            (*info).RootDirectory = std::ptr::null_mut();
            (*info).FileNameLength = (name.len() * 2) as u32;
            std::ptr::copy_nonoverlapping(
                name.as_ptr(),
                buffer.as_mut_ptr().cast::<u8>().add(offset).cast::<u16>(),
                name.len(),
            );
            if SetFileInformationByHandle(
                file.as_raw_handle(),
                FileRenameInfo,
                info.cast(),
                length as u32,
            ) == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        let _ = source;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = file;
        fs::rename(source, backup).map_err(Error::Io)
    }
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
    let mut input = File::open(source)?;
    let metadata = input.metadata()?;
    if !metadata.is_file() {
        return Err(Error::Unsupported(
            "private incoming stage has an invalid size".into(),
        ));
    }
    if hash_save_content(&mut input)? != expected_hash {
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
        use std::io::{Seek, SeekFrom};
        input.seek(SeekFrom::Start(0))?;
        std::io::copy(&mut input, &mut file)?;
        file.sync_all()?;
        let mut verify = File::open(&sibling)?;
        if hash_save_content(&mut verify)? != expected_hash {
            return Err(Error::Unsupported(
                "Incoming file changed during publication staging".into(),
            ));
        }
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

    /// Elevated tokens hold restore-type privileges that override ACL denies
    /// for namespace mutations, so deny-based checks cannot bind them. Windows
    /// Server diagnostic hosts run elevated; client acceptance runs do not.
    #[cfg(windows)]
    fn elevated_token() -> bool {
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::Security::{
            GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
        };
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        // SAFETY: token is a live process handle; output is fixed-size.
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut elevation = TOKEN_ELEVATION::default();
            let mut length = 0;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                (&mut elevation as *mut TOKEN_ELEVATION).cast(),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut length,
            );
            CloseHandle(token);
            ok != 0 && elevation.TokenIsElevated != 0
        }
    }

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
        if elevated_token() {
            eprintln!("write-deny check skipped: elevated token bypasses ACL denies");
        } else {
            assert!(result.is_err(), "write-denied directory accepted");
        }
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
    fn baseline_replacement_preserves_concurrent_target_and_restores_on_cancellation() {
        for concurrent in [true, false] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("saves");
            fs::create_dir(&root).unwrap();
            let staged = directory.path().join("incoming.stage");
            let target = root.join("ordinary.dat");
            let backup = root.join(".ordinary.dat.rommfs-backup-test");
            fs::write(&staged, b"remote update").unwrap();
            fs::write(&target, b"baseline").unwrap();
            let interrupted = AtomicBool::new(false);
            let result = publish_changed_save(
                &staged,
                &target,
                &sha256_content_hash(b"remote update"),
                &sha256_content_hash(b"baseline"),
                &backup,
                &root,
                || {
                    if backup.exists() && !interrupted.swap(true, Ordering::SeqCst) {
                        if concurrent {
                            fs::write(&target, b"concurrent bytes")?;
                        } else {
                            return Err(Error::Cancelled);
                        }
                    }
                    Ok(())
                },
            );
            assert!(result.is_err());
            assert!(interrupted.load(Ordering::SeqCst));
            if concurrent {
                assert_eq!(fs::read(&target).unwrap(), b"concurrent bytes");
                assert_eq!(fs::read(&backup).unwrap(), b"baseline");
            } else {
                assert_eq!(fs::read(&target).unwrap(), b"baseline");
            }
            assert_eq!(fs::read(&staged).unwrap(), b"remote update");
            assert!(!has_publication_sibling(&root));
        }
    }

    /// Slot-reconstructed targets can carry portable `/` separators inside
    /// the `PathBuf`; the NT rename must normalize them rather than emit an
    /// unresolvable `\??\`-prefixed mixed-separator path.
    #[test]
    fn baseline_replacement_normalizes_portable_separators_in_multi_level_paths() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("saves");
        fs::create_dir(&root).unwrap();
        let parent = root.join("switch/Game/user");
        fs::create_dir_all(&parent).unwrap();
        let staged = directory.path().join("incoming.stage");
        let target = parent.join("0");
        // Build with '/' — what a path reconstructed from a portable file
        // slot retains in its OsStr.
        let backup = std::path::PathBuf::from(format!(
            "{}/switch/Game/user/.0.rommfs-backup-test",
            root.display()
        ));
        fs::write(&staged, b"remote update").unwrap();
        fs::write(&target, b"baseline").unwrap();
        publish_changed_save(
            &staged,
            &target,
            &sha256_content_hash(b"remote update"),
            &sha256_content_hash(b"baseline"),
            &backup,
            &root,
            || Ok(()),
        )
        .unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"remote update");
        assert_eq!(fs::read(&backup).unwrap(), b"baseline");
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
