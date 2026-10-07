//! ACLs reject ordinary data/namespace writes while retaining WRITE_DAC for CFAPI.
use anyhow::Context;
use std::fs::File;
use std::os::windows::io::AsRawHandle;
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SetSecurityInfo,
    SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    GetSecurityDescriptorControl, GetSecurityDescriptorDacl, DACL_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION, SE_DACL_PROTECTED, UNPROTECTED_DACL_SECURITY_INFORMATION,
};

// The deny ACE must precede the allows: access checks walk the DACL in order.
// Without it the OWNER_RIGHTS allow grants DELETE to the interactive user, so
// "read-only" placeholders stayed deletable. Authenticated Users is denied
// every data/namespace write (delete child, delete, write/append data, write
// EA, write attributes) while SYSTEM and the implicit owner WRITE_DAC remain,
// which is how allow_delete and CfCreatePlaceholders keep working.
pub(crate) const READ_ONLY: &str =
    "D:P(D;OICI;0x10156;;;AU)(A;OICI;FRFX;;;WD)(A;OICI;0x001600a9;;;OW)(A;OICI;FA;;;SY)";
const DELETE_OWNED: &str = "D:P(A;;FRFX;;;WD)(A;;FA;;;OW)(A;;FA;;;SY)";

struct Descriptor(*mut std::ffi::c_void);
impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: both APIs below allocate descriptors with LocalAlloc.
        unsafe {
            LocalFree(self.0);
        }
    }
}

pub(crate) fn snapshot(file: &File) -> anyhow::Result<String> {
    let mut descriptor = Descriptor(std::ptr::null_mut());
    // SAFETY: file is live; output is freed by Descriptor.
    let result = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut descriptor.0,
        )
    };
    anyhow::ensure!(
        result == 0,
        "GetSecurityInfo failed: {}",
        std::io::Error::from_raw_os_error(result as i32)
    );
    let mut text = std::ptr::null_mut();
    let mut length = 0;
    // SAFETY: Windows allocated a valid descriptor; text is a LocalAlloc output.
    if unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor.0,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut text,
            &mut length,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    let _allocation = Descriptor(text.cast());
    // SAFETY: successful conversion returned length UTF-16 units including NUL.
    let units = unsafe { std::slice::from_raw_parts(text, length as usize) };
    let end = units.iter().position(|&c| c == 0).unwrap_or(units.len());
    Ok(String::from_utf16(&units[..end])?)
}

pub(crate) fn set(file: &File, sddl: &str) -> anyhow::Result<()> {
    let text: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    let mut descriptor = Descriptor(std::ptr::null_mut());
    // SAFETY: terminated input and writable output; Descriptor frees allocation.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor.0,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    apply(file, descriptor.0)
}

pub(crate) fn restore(file: &File, sddl: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!sddl.is_empty(), "missing original directory ACL");
    set(file, sddl)
}

fn apply(file: &File, descriptor: *mut std::ffi::c_void) -> anyhow::Result<()> {
    let mut present = 0;
    let mut defaulted = 0;
    let mut dacl = std::ptr::null_mut();
    // SAFETY: descriptor is a live, aligned Windows security descriptor.
    if unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) }
        == 0
    {
        return Err(std::io::Error::last_os_error()).context("read directory ACL");
    }
    anyhow::ensure!(present != 0, "security descriptor has no DACL");
    let mut control = 0;
    let mut revision = 0;
    // SAFETY: descriptor was validated by Windows' SDDL parser.
    if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let inheritance = if control & SE_DACL_PROTECTED != 0 {
        PROTECTED_DACL_SECURITY_INFORMATION
    } else {
        UNPROTECTED_DACL_SECURITY_INFORMATION
    };
    // SAFETY: file is open with WRITE_DAC; dacl lives through this call.
    let result = unsafe {
        SetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | inheritance,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            dacl,
            std::ptr::null_mut(),
        )
    };
    anyhow::ensure!(
        result == 0,
        "SetSecurityInfo failed: {}",
        std::io::Error::from_raw_os_error(result as i32)
    );
    Ok(())
}

pub(crate) fn allow_delete(file: &File) -> anyhow::Result<()> {
    set(file, DELETE_OWNED)
}
