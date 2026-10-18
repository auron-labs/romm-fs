//! Elevate this executable for local setup only; never open an elevated UI/session.
use anyhow::Context;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
use windows_sys::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
};

const HELPER: &str = "--rommfs-retrobat-setup";

/// Called before GPUI, settings, credentials, or any background workers start.
/// Malformed helper invocations fail closed instead of launching the normal app.
pub fn run_helper(args: impl IntoIterator<Item = OsString>) -> Option<i32> {
    let mut args = args.into_iter().skip(1);
    if args.next().as_deref() != Some(OsStr::new(HELPER)) {
        return None;
    }
    let (Some(installation), Some(managed), Some(server)) = (args.next(), args.next(), args.next())
    else {
        return Some(1);
    };
    let Some(server) = server.to_str().filter(|s| !s.is_empty()) else {
        return Some(1);
    };
    if args.next().is_some() {
        return Some(1);
    }
    Some(
        match super::setup(Path::new(&installation), Path::new(&managed), server) {
            Ok(_) => 0,
            Err(error) => {
                eprintln!("{error:#}");
                1
            }
        },
    )
}

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

// Windows argv escaping: double backslashes preceding quotes or the closing
// quote. No shell/interpreter is involved; retain Unicode and trailing slashes.
fn parameters(args: &[&OsStr]) -> Vec<u16> {
    let mut out = Vec::new();
    for (index, arg) in args.iter().enumerate() {
        if index != 0 {
            out.push(b' ' as u16);
        }
        out.push(b'"' as u16);
        let mut slashes = 0;
        for unit in arg.encode_wide() {
            if unit == b'\\' as u16 {
                slashes += 1;
                continue;
            }
            out.extend(std::iter::repeat_n(
                b'\\' as u16,
                if unit == b'"' as u16 {
                    slashes * 2 + 1
                } else {
                    slashes
                },
            ));
            out.push(unit);
            slashes = 0;
        }
        out.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
        out.push(b'"' as u16);
    }
    out.push(0);
    out
}

pub(crate) fn run_elevated(
    installation: &Path,
    managed: &Path,
    server: &str,
) -> anyhow::Result<super::SetupReport> {
    let executable = wide(std::env::current_exe()?.as_os_str());
    let verb = wide(OsStr::new("runas"));
    let parameters = parameters(&[
        OsStr::new(HELPER),
        installation.as_os_str(),
        managed.as_os_str(),
        OsStr::new(server),
    ]);
    // SAFETY: zero is a valid initial shell info; the UTF-16 buffers outlive
    // ShellExecuteExW and contain terminal NULs.
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
    info.lpVerb = verb.as_ptr();
    info.lpFile = executable.as_ptr();
    info.lpParameters = parameters.as_ptr();
    info.nShow = 0;
    // SAFETY: info and all referenced buffers are valid for this call.
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(1223) {
            anyhow::bail!("Windows permission was declined, so nothing was changed. Retry and approve the Windows prompt to link RomM games into RetroBat");
        }
        return Err(error).context("request Windows permission for RetroBat setup");
    }
    anyhow::ensure!(
        !info.hProcess.is_null(),
        "Windows did not return a setup process handle"
    );
    let process = Process(info.hProcess);
    // This runs on the controller worker, not the UI thread. Drain the helper
    // before processing another command or exiting; never kill setup mid-write.
    // SAFETY: process owns a valid process handle through both calls.
    let status = unsafe { WaitForSingleObject(process.0, INFINITE) };
    anyhow::ensure!(
        status == WAIT_OBJECT_0,
        "could not wait for elevated RetroBat setup: {}",
        std::io::Error::last_os_error()
    );
    let mut code = 0;
    // SAFETY: the process handle is live and code is a valid output pointer.
    if unsafe { GetExitCodeProcess(process.0, &mut code) } == 0 {
        return Err(std::io::Error::last_os_error()).context("read elevated setup result");
    }
    anyhow::ensure!(code == 0, "Elevated RetroBat setup did not complete. Close RetroBat and its emulators, check folder access, then retry (helper exit code {code})");
    // Inspect only: the normal app never repeats privileged mutations.
    let outcome = rommfs_core::retrobat::inspect_setup(installation, managed, server)?;
    Ok(super::SetupReport {
        messages: vec![outcome.summary(), super::switch_status(installation)],
        skipped: outcome.skipped,
    })
}

struct Process(windows_sys::Win32::Foundation::HANDLE);
impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns the handle and closes it exactly once.
        unsafe { CloseHandle(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_round_trip_through_windows_parser() {
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::UI::Shell::CommandLineToArgvW;
        let values = [
            "program",
            "",
            "C:\\My games\\",
            "quotes\" and \\\"",
            "日本語",
            "https://romm.test/a?x=&y=1",
        ];
        let args: Vec<_> = values.iter().map(OsStr::new).collect();
        let command = parameters(&args);
        let mut count = 0;
        // SAFETY: command is NUL terminated; count is a valid output pointer.
        let parsed = unsafe { CommandLineToArgvW(command.as_ptr(), &mut count) };
        assert!(!parsed.is_null());
        assert_eq!(count as usize, values.len());
        for (index, expected) in values.iter().enumerate() {
            // SAFETY: parsed contains count valid NUL terminated strings.
            let actual = unsafe {
                let ptr = *parsed.add(index);
                let mut len = 0;
                while *ptr.add(len) != 0 {
                    len += 1;
                }
                String::from_utf16(std::slice::from_raw_parts(ptr, len)).unwrap()
            };
            assert_eq!(&actual, expected);
        }
        // SAFETY: CommandLineToArgvW allocates with LocalAlloc.
        unsafe { LocalFree(parsed.cast()) };
    }

    #[test]
    fn helper_rejects_malformed_requests_without_starting_ui() {
        let args = |values: &[&str]| values.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(run_helper(args(&["app"])), None);
        assert_eq!(run_helper(args(&["app", HELPER])), Some(1));
        assert_eq!(
            run_helper(args(&["app", HELPER, "relative", "relative", "server"])),
            Some(1)
        );
        assert_eq!(
            run_helper(args(&[
                "app",
                HELPER,
                "C:\\RetroBat",
                "C:\\managed",
                "server",
                "extra"
            ])),
            Some(1)
        );
    }
}
