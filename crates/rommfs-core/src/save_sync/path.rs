use crate::error::{Error, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

/// Game Boy SRAM is small; refuse unexpected files rather than copying an
/// unbounded path into the private capture store.
pub const MAX_SAVE_BYTES: u64 = 1024 * 1024;

/// The P2 profile accepts exactly one safe system directory and one `.srm`
/// filename. In particular, Windows ADS and traversal syntax are rejected on
/// every host, not just when running on Windows.
pub fn validate_relative_save_path(path: &Path) -> std::result::Result<(), String> {
    let mut path_components = path.components();
    let system = match path_components.next() {
        Some(Component::Normal(component)) => component
            .to_str()
            .ok_or_else(|| "save target path is not valid Unicode".to_string())?,
        _ => return Err("save target must be a relative path without traversal".into()),
    };
    let filename = match path_components.next() {
        Some(Component::Normal(component)) => component
            .to_str()
            .ok_or_else(|| "save target path is not valid Unicode".to_string())?,
        _ => return Err("save target must contain one system directory and one filename".into()),
    };
    if path_components.next().is_some() {
        return Err("save target must contain one system directory and one filename".into());
    }
    if !system.eq_ignore_ascii_case("gb")
        || !filename.rsplit_once('.').is_some_and(|(stem, extension)| {
            !stem.is_empty() && extension.eq_ignore_ascii_case("srm")
        })
    {
        return Err("save target is outside the supported Game Boy SRAM profile".into());
    }
    validate_windows_path_component(system)?;
    validate_windows_path_component(filename)?;
    Ok(())
}

/// Reject components that have special meaning or are not representable as a
/// normal Windows filesystem name. Kept host-independent for export paths.
pub fn validate_windows_path_component(component: &str) -> std::result::Result<(), String> {
    if component.is_empty()
        || component.trim().is_empty()
        || component.ends_with(['.', ' '])
        || component.chars().any(|character| {
            (character as u32) < 0x20
                || character == '\u{7f}'
                || ['<', '>', ':', '"', '|', '?', '*', '/', '\\'].contains(&character)
        })
    {
        return Err("save target contains a Windows-invalid path component".into());
    }
    let stem = component.split('.').next().unwrap_or_default();
    let stem = stem.trim_end_matches(['.', ' ']);
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM1", "COM2", "COM3", "COM4", "COM5",
        "COM6", "COM7", "COM8", "COM9", "COM¹", "COM²", "COM³", "LPT1", "LPT2", "LPT3", "LPT4",
        "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "LPT¹", "LPT²", "LPT³",
    ];
    if RESERVED.contains(&stem.to_ascii_uppercase().as_str()) {
        return Err("save target uses a reserved Windows device name".into());
    }
    Ok(())
}

pub fn resolve_save_target(root: &Path, relative_path: &Path) -> Result<PathBuf> {
    validate_relative_save_path(relative_path).map_err(Error::Unsupported)?;
    let target = root.join(relative_path);
    ensure_no_reparse_components(root)?;
    ensure_no_reparse_components(&target)?;
    Ok(target)
}

/// Keep every existing directory from the volume root through `path` open
/// without delete sharing. On Windows this blocks rename/reparse substitution
/// while a save file is opened, hashed, copied, or atomically published.
pub struct SaveDirectoryChainGuard {
    #[cfg(windows)]
    _handles: Vec<File>,
}

pub fn hold_save_directory_chain(path: &Path) -> Result<SaveDirectoryChainGuard> {
    ensure_no_reparse_components(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY,
            FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };

        let mut directories = path.ancestors().collect::<Vec<_>>();
        directories.reverse();
        let mut handles = Vec::with_capacity(directories.len());
        for directory in directories {
            let handle = OpenOptions::new()
                // Attribute-only handles do not reliably prevent directory
                // rename on Windows Server 2022. Request list access as well
                // so this open handle protects the directory namespace; omit
                // FILE_SHARE_DELETE to block rename/reparse substitution.
                .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(directory)?;
            let metadata = handle.metadata()?;
            if is_reparse_point(&metadata) || !metadata.is_dir() {
                return Err(Error::Unsupported(format!(
                    "save path traverses a symbolic link or reparse point: {}",
                    directory.display()
                )));
            }
            handles.push(handle);
        }
        Ok(SaveDirectoryChainGuard { _handles: handles })
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(SaveDirectoryChainGuard {})
    }
}

pub(crate) struct SaveReadHandle {
    file: File,
    _directories: SaveDirectoryChainGuard,
}

impl Read for SaveReadHandle {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.file.read(buffer)
    }
}

pub(crate) fn has_rtc_companion(root: &Path, relative_path: &Path) -> Result<bool> {
    validate_relative_save_path(relative_path).map_err(Error::Unsupported)?;
    let companion = root.join(relative_path.with_extension("rtc"));
    ensure_no_reparse_components(&companion)?;
    match fs::symlink_metadata(&companion) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn ensure_no_rtc_companion(root: &Path, relative_path: &Path) -> Result<()> {
    if has_rtc_companion(root, relative_path)? {
        return Err(Error::Unsupported(
            "Gambatte RTC sidecars are not supported by the SRAM-only save profile".into(),
        ));
    }
    Ok(())
}

pub(crate) fn open_save_read(root: &Path, relative_path: &Path) -> Result<SaveReadHandle> {
    let path = resolve_save_target(root, relative_path)?;
    ensure_no_rtc_companion(root, relative_path)?;
    let parent = path
        .parent()
        .ok_or_else(|| Error::Unsupported("save target has no parent directory".into()))?;
    let directories = hold_save_directory_chain(parent)?;
    ensure_no_reparse_components(&path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ only: while this short-lived handle is open, other
        // processes cannot open the save for writing or deletion.
        options.share_mode(0x0000_0001);
        // Open the final reparse point itself so it can be rejected below.
        options.custom_flags(0x0020_0000);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || is_reparse_point(&metadata) {
        return Err(Error::Unsupported(
            "save target is not a plain single file".into(),
        ));
    }
    if metadata.len() == 0 || metadata.len() > MAX_SAVE_BYTES {
        return Err(Error::Unsupported(format!(
            "Game Boy SRAM must be nonempty and no larger than {MAX_SAVE_BYTES} bytes"
        )));
    }
    Ok(SaveReadHandle {
        file,
        _directories: directories,
    })
}

/// Checks each existing component along `path`, including its ancestors, without
/// following symlinks or reparse points.
///
/// Returns an error if an existing component is a symlink or reparse point. A
/// missing component allows the remaining suffix; this function never creates
/// filesystem entries.
pub fn ensure_no_reparse_components(path: &Path) -> Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) => {
                current.push(component.as_os_str());
                // A Windows prefix such as `\\?\C:` is not itself a
                // filesystem path. Inspect it only after its root is joined.
                continue;
            }
            Component::RootDir | Component::Normal(_) => {
                current.push(component.as_os_str());
            }
            Component::CurDir => continue,
            Component::ParentDir => {
                current.push("..");
                continue;
            }
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if is_reparse_point(&metadata) => {
                return Err(Error::Unsupported(format!(
                    "save path traverses a symbolic link or reparse point: {}",
                    current.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
pub(super) fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

pub fn path_is_reparse_point(metadata: &fs::Metadata) -> bool {
    is_reparse_point(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_absolute_traversal_ads_and_windows_invalid_components() {
        for path in [
            Path::new("/gb/game.srm"),
            Path::new("gb/../outside.srm"),
            Path::new("gb/C:game.srm"),
            Path::new("gb/game.srm:stream"),
            Path::new("gb/name. "),
            Path::new("gb/CON.srm"),
            Path::new("gb/CONIN$.srm"),
            Path::new("snes/game.srm"),
        ] {
            assert!(validate_relative_save_path(path).is_err(), "{path:?}");
        }
        assert!(validate_relative_save_path(Path::new("gb/Game.srm")).is_ok());
        assert!(validate_relative_save_path(&PathBuf::from("gb").join("Game.srm")).is_ok());
        #[cfg(not(windows))]
        assert!(validate_relative_save_path(Path::new("gb\\game.srm")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn target_resolution_rejects_nested_symlink_excursions() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("saves");
        let external = dir.path().join("elsewhere");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&external).unwrap();
        symlink(&external, root.join("gb")).unwrap();

        assert!(resolve_save_target(&root, Path::new("gb/game.srm")).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_save_read_handle_denies_concurrent_write_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("saves");
        fs::create_dir_all(root.join("gb")).unwrap();
        let relative = Path::new("gb/game.srm");
        let target = root.join(relative);
        fs::write(&target, b"SRAM bytes").unwrap();

        let reader = open_save_read(&root, relative).unwrap();
        assert!(OpenOptions::new().write(true).open(&target).is_err());
        assert!(fs::remove_file(&target).is_err());
        drop(reader);
        assert!(OpenOptions::new().write(true).open(&target).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn windows_directory_guard_blocks_rename_until_released() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("saves");
        fs::create_dir_all(parent.join("gb")).unwrap();
        let renamed = dir.path().join("saves-renamed");
        let ancestor = dir.path().to_path_buf();
        let renamed_ancestor = dir.path().with_file_name(format!(
            "{}-renamed",
            dir.path().file_name().unwrap().to_string_lossy()
        ));

        let guard = hold_save_directory_chain(&parent).unwrap();
        assert!(fs::rename(&parent, &renamed).is_err());
        assert!(fs::rename(&ancestor, &renamed_ancestor).is_err());

        drop(guard);
        fs::rename(&parent, &renamed).unwrap();
        fs::rename(&renamed, &parent).unwrap();
        fs::rename(&ancestor, &renamed_ancestor).unwrap();
        fs::rename(&renamed_ancestor, &ancestor).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_canonicalized_paths_accept_existing_and_missing_children() {
        let dir = tempfile::tempdir().unwrap();
        let canonical = fs::canonicalize(dir.path()).unwrap();
        let existing = canonical.join("existing");
        fs::create_dir(&existing).unwrap();

        assert_eq!(
            resolve_save_target(&existing, Path::new("gb/game.srm")).unwrap(),
            existing.join("gb/game.srm")
        );
        assert!(resolve_save_target(&canonical, Path::new("gb/game.srm")).is_ok());
        assert!(ensure_no_reparse_components(&canonical.join("missing/child")).is_ok());
        let _guard = hold_save_directory_chain(&existing).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_canonicalized_ancestor_junction_is_rejected() {
        use std::process::Command;

        let dir = tempfile::tempdir().unwrap();
        let canonical = fs::canonicalize(dir.path()).unwrap();
        let target = dir.path().join("junction-target");
        let junction = dir.path().join("junction");
        let canonical_junction = canonical.join("junction");
        fs::create_dir_all(target.join("gb")).unwrap();

        let output = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&target)
            .output()
            .expect("run cmd to create a directory junction");
        assert!(
            output.status.success(),
            "junction setup failed: stdout={}, stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let linked_root = canonical_junction.join("gb");
        let resolution = resolve_save_target(&canonical_junction, Path::new("gb/game.srm"));
        let guard = hold_save_directory_chain(&linked_root);
        fs::remove_dir(&junction).unwrap();

        assert!(resolution.is_err(), "save resolution accepted a junction");
        assert!(guard.is_err(), "directory guard accepted a junction");
    }
}
