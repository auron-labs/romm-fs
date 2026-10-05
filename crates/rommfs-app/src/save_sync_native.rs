//! Windows-only, read-only sources for RetroBat candidate discovery.

use rommfs_core::save_sync::{DiscoveryInput, InstallationSource};
use std::path::PathBuf;
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
use windows_sys::Win32::System::ProcessStatus::EnumProcesses;
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};

const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;

pub(crate) fn discovery_input() -> DiscoveryInput {
    DiscoveryInput {
        drive_roots: retrobat_drive_roots(),
        process_images: retrobat_process_images(),
    }
}

fn retrobat_drive_roots() -> Vec<(PathBuf, InstallationSource)> {
    // SAFETY: `GetLogicalDrives` takes no pointers or handles from the caller.
    let drive_mask = unsafe { GetLogicalDrives() };
    let mut roots = Vec::new();
    for index in 0..26 {
        if drive_mask & (1 << index) == 0 {
            continue;
        }
        let letter = (b'A' + index as u8) as char;
        let drive = format!("{letter}:\\");
        let mut wide_drive: Vec<u16> = drive.encode_utf16().collect();
        wide_drive.push(0);
        // SAFETY: `wide_drive` is a live, null-terminated UTF-16 drive path for this call.
        let kind = unsafe { GetDriveTypeW(wide_drive.as_ptr()) };
        let source = match kind {
            DRIVE_FIXED => InstallationSource::FixedDrive,
            DRIVE_REMOVABLE => InstallationSource::RemovableDrive,
            _ => continue,
        };
        let root = PathBuf::from(format!("{letter}:\\RetroBat"));
        // A missing directory is an ordinary drive miss. Metadata errors are
        // also ignored here; installation validation treats denied reads as a
        // skipped candidate rather than aborting discovery.
        match std::fs::metadata(&root) {
            Ok(metadata) if metadata.is_dir() => roots.push((root, source)),
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                roots.push((root, source));
            }
            _ => {}
        }
    }
    roots
}

fn retrobat_process_images() -> Vec<PathBuf> {
    let mut process_ids = vec![0u32; 4096];
    let mut bytes_needed = 0u32;
    // SAFETY: both output pointers refer to writable values/buffer with the
    // exact byte capacity supplied to the API.
    let enumerated = unsafe {
        EnumProcesses(
            process_ids.as_mut_ptr(),
            (process_ids.len() * std::mem::size_of::<u32>()) as u32,
            &mut bytes_needed,
        )
    };
    if enumerated == 0 {
        return Vec::new();
    }
    process_ids
        .truncate((bytes_needed as usize / std::mem::size_of::<u32>()).min(process_ids.len()));

    let mut images = Vec::new();
    for process_id in process_ids.into_iter().filter(|id| *id != 0) {
        // SAFETY: the call uses scalar arguments only; a null result is checked
        // before the returned process handle is passed to another API.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
        if process.is_null() {
            continue;
        }
        let mut buffer = vec![0u16; 32_768];
        let mut length = buffer.len() as u32;
        // SAFETY: `process` is the live handle returned by `OpenProcess`, and
        // `buffer`/`length` are writable for the capacity passed to the API.
        let read =
            unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) };
        // SAFETY: `process` is still the live handle opened above and is closed
        // exactly once here, regardless of whether the query succeeded.
        unsafe { CloseHandle(process) };
        if read == 0 {
            continue;
        }
        let path = PathBuf::from(String::from_utf16_lossy(&buffer[..length as usize]));
        if path.file_name().is_some_and(|name| {
            name.eq_ignore_ascii_case("retrobat.exe")
                || name.eq_ignore_ascii_case("emulationstation.exe")
        }) {
            images.push(path);
        }
    }
    images
}
