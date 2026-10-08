//! Link cleanup must not confuse data-bearing cloud/WOF reparse points with
//! symlinks. Callers protect the parent directory chain before inspecting or
//! deleting an entry; CFAPI roots use their own verified-root guard.
use crate::{Error, Result};
use std::path::Path;

/// True only for a symbolic link or junction, not an arbitrary reparse point.
pub fn path_is_symbolic_link(path: &Path) -> Result<bool> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        let file = open_link(
            path,
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )?;
        is_link_handle(&file)
    }
    #[cfg(not(windows))]
    {
        Ok(std::fs::symlink_metadata(path)?.file_type().is_symlink())
    }
}

/// Unlink only a re-verified symbolic link/junction. The caller must keep the
/// parent chain protected for the entire operation. On Windows verification
/// and deletion use the same exclusive, no-follow handle to prevent swapping
/// the link for a data file between those operations.
pub fn remove_symbolic_link(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FileDispositionInfo, SetFileInformationByHandle, DELETE, FILE_DISPOSITION_INFO,
            FILE_READ_ATTRIBUTES,
        };
        let file = open_link(path, DELETE | FILE_READ_ATTRIBUTES, 0)?;
        if !is_link_handle(&file)? {
            return Err(Error::Unsupported(
                "entry is not a symbolic link or junction; it was preserved".into(),
            ));
        }
        let info = FILE_DISPOSITION_INFO { DeleteFile: true };
        // SAFETY: the handle stays live and `info` has the required layout/size.
        if unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle().cast(),
                FileDispositionInfo,
                (&info as *const FILE_DISPOSITION_INFO).cast(),
                std::mem::size_of_val(&info) as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        if !path_is_symbolic_link(path)? {
            return Err(Error::Unsupported(
                "entry is not a symbolic link; it was preserved".into(),
            ));
        }
        std::fs::remove_file(path)?;
        Ok(())
    }
}

#[cfg(windows)]
fn open_link(path: &Path, access: u32, sharing: u32) -> Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    };
    Ok(std::fs::OpenOptions::new()
        .access_mode(access)
        .share_mode(sharing)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?)
}

#[cfg(windows)]
fn is_link_handle(file: &std::fs::File) -> Result<bool> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FileAttributeTagInfo, GetFileInformationByHandleEx, FILE_ATTRIBUTE_TAG_INFO,
    };
    let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: the handle stays live and the output buffer matches this class.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle().cast(),
            FileAttributeTagInfo,
            (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            std::mem::size_of_val(&info) as u32,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(is_link_tag(info.ReparseTag))
}

#[cfg(windows)]
fn is_link_tag(tag: u32) -> bool {
    use windows_sys::Win32::System::SystemServices::{
        IO_REPARSE_TAG_MOUNT_POINT, IO_REPARSE_TAG_SYMLINK,
    };
    matches!(tag, IO_REPARSE_TAG_MOUNT_POINT | IO_REPARSE_TAG_SYMLINK)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    use windows_sys::Win32::System::SystemServices::{
        IO_REPARSE_TAG_CLOUD, IO_REPARSE_TAG_MOUNT_POINT, IO_REPARSE_TAG_SYMLINK,
        IO_REPARSE_TAG_WOF,
    };

    #[test]
    fn link_removal_refuses_plain_files_and_directories() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("keep.srm");
        let directory = temp.path().join("keep");
        std::fs::write(&file, b"local save").unwrap();
        std::fs::create_dir(&directory).unwrap();
        for path in [&file, &directory] {
            assert!(!path_is_symbolic_link(path).unwrap());
            assert!(remove_symbolic_link(path).is_err());
        }
        assert_eq!(std::fs::read(&file).unwrap(), b"local save");
        assert!(directory.is_dir());
    }

    #[cfg(windows)]
    #[test]
    fn only_symlink_and_junction_tags_are_removable() {
        assert!(is_link_tag(IO_REPARSE_TAG_SYMLINK));
        assert!(is_link_tag(IO_REPARSE_TAG_MOUNT_POINT));
        for tag in [0, IO_REPARSE_TAG_CLOUD, IO_REPARSE_TAG_WOF] {
            assert!(!is_link_tag(tag));
        }
    }
}
