//! Small Windows boundary: per-user instance ownership and login registration.
use rommfs_core::{Error, Result};
#[cfg(any(windows, test))]
use std::path::Path;
#[cfg(all(windows, feature = "ui"))]
pub mod tray;

#[cfg(any(windows, test))]
pub fn startup_command(executable: &Path) -> Result<String> {
    if !executable.is_absolute() {
        return Err(Error::Unsupported(
            "Startup executable must have an absolute path".into(),
        ));
    }
    let path = executable
        .to_str()
        .ok_or_else(|| Error::Unsupported("Startup executable path is not Unicode".into()))?;
    if path.contains(['"', '\r', '\n', '\0']) {
        return Err(Error::Unsupported(
            "Startup executable path cannot be quoted safely".into(),
        ));
    }
    Ok(format!("\"{path}\" --startup"))
}

pub fn set_start_at_login(enabled: bool) -> Result<()> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
        use windows_sys::Win32::System::Registry::*;
        let path = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        let name = wide("RomMFS");
        let command = if enabled {
            Some(wide(&startup_command(&std::env::current_exe()?)?))
        } else {
            None
        };
        let mut key = std::ptr::null_mut();
        // SAFETY: terminated UTF-16 strings and writable out-handle are valid
        // for the synchronous call; this opens only the current user's Run key.
        let result = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                0,
                std::ptr::null(),
                0,
                KEY_SET_VALUE,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            )
        };
        if result != 0 {
            return Err(std::io::Error::from_raw_os_error(result as i32).into());
        }
        let result = if let Some(value) = command {
            // SAFETY: REG_SZ byte length includes the UTF-16 terminator.
            unsafe {
                RegSetValueExW(
                    key,
                    name.as_ptr(),
                    0,
                    REG_SZ,
                    value.as_ptr().cast(),
                    (value.len() * 2) as u32,
                )
            }
        } else {
            // SAFETY: live key and terminated value name; no other values touched.
            unsafe { RegDeleteValueW(key, name.as_ptr()) }
        };
        // SAFETY: key was returned by successful RegCreateKeyExW.
        unsafe {
            RegCloseKey(key);
        }
        if result != 0 && !(result == ERROR_FILE_NOT_FOUND && !enabled) {
            return Err(std::io::Error::from_raw_os_error(result as i32).into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        Err(Error::Unsupported(
            "Start at login is available on Windows only".into(),
        ))
    }
}

#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

#[cfg_attr(not(feature = "ui"), allow(dead_code))]
pub struct Instance {
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
}
#[cfg_attr(not(feature = "ui"), allow(dead_code))]
impl Instance {
    /// A session-local mutex coordinates visible and hidden startup launches.
    pub fn acquire() -> Result<Option<Self>> {
        #[cfg(windows)]
        {
            use windows_sys::Win32::{
                Foundation::{GetLastError, ERROR_ALREADY_EXISTS},
                System::Threading::CreateMutexW,
            };
            let name = wide("Local\\RomMFS");
            // SAFETY: terminated mutex name, default security, no initial lock.
            let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
            if handle.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            // SAFETY: read immediately after CreateMutexW, before any other API.
            let exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
            if exists {
                // SAFETY: successfully created/opened kernel handle.
                unsafe {
                    windows_sys::Win32::Foundation::CloseHandle(handle);
                }
                open_existing();
                return Ok(None);
            }
            Ok(Some(Self { handle }))
        }
        #[cfg(not(windows))]
        {
            Ok(Some(Self {}))
        }
    }
}
#[cfg(windows)]
impl Drop for Instance {
    fn drop(&mut self) {
        // SAFETY: this owner closes its live mutex exactly once.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}
#[cfg(windows)]
fn open_existing() {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let title = wide("RomMFS");
    // SAFETY: FindWindow searches the interactive user's desktop. Single-instance
    // ownership ensures only our running window has this application title.
    unsafe {
        let window = FindWindowW(std::ptr::null(), title.as_ptr());
        if !window.is_null() {
            ShowWindow(window, SW_SHOW);
            ShowWindow(window, SW_RESTORE);
            SetForegroundWindow(window);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn login_command_quotes_the_executable_and_rejects_injection() {
        let path = if cfg!(windows) {
            r"C:\directory with spaces\RomMFS.exe"
        } else {
            "/directory with spaces/RomMFS.exe"
        };
        assert_eq!(
            startup_command(Path::new(path)).unwrap(),
            format!("\"{path}\" --startup")
        );
        for path in ["relative.exe", "/path/\"bad.exe", "/path/\n.exe"] {
            assert!(startup_command(Path::new(path)).is_err());
        }
    }
}
