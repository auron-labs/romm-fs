//! Official release discovery and verified, streamed installer downloads.
//! The current package is an interactive .NET wizard, not Inno Setup/NSIS.
use crate::save_sync::{ensure_no_reparse_components, hold_save_directory_chain};
use crate::{Error, Result};
use isahc::prelude::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const RELEASE_API: &str = "https://api.github.com/repos/RetroBat-Official/retrobat/releases/latest";
const MAX_RELEASE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct InstallerRelease {
    version: String,
    filename: String,
    url: String,
    size: u64,
    sha256: String,
}
impl InstallerRelease {
    pub fn version(&self) -> &str {
        &self.version
    }
    pub fn size(&self) -> u64 {
        self.size
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}

fn parse_release(bytes: &[u8]) -> Result<InstallerRelease> {
    let release: Release = serde_json::from_slice(bytes)
        .map_err(|_| Error::Unsupported("Official release metadata is invalid".into()))?;
    if release.draft || release.prerelease || release.tag_name.is_empty() {
        return Err(Error::Unsupported(
            "No official stable RetroBat release is available".into(),
        ));
    }
    let mut candidates = release.assets.into_iter().filter(|a| {
        a.name.starts_with("RetroBat-v") && a.name.ends_with("-stable-win64-setup.exe")
    });
    let asset = candidates.next().ok_or_else(|| {
        Error::Unsupported("Official release has no supported Windows installer".into())
    })?;
    if candidates.next().is_some() || asset.size == 0 {
        return Err(Error::Unsupported(
            "Official installer selection is ambiguous or empty".into(),
        ));
    }
    crate::save_sync::validate_windows_path_component(&asset.name).map_err(Error::Unsupported)?;
    let prefix = "https://github.com/RetroBat-Official/retrobat/releases/download/";
    if !asset.browser_download_url.starts_with(prefix) {
        return Err(Error::Unsupported(
            "Installer is not hosted in the official release repository".into(),
        ));
    }
    validate_download_url(&asset.browser_download_url)?;
    let digest = asset
        .digest
        .as_deref()
        .and_then(|d| d.strip_prefix("sha256:"))
        .filter(|d| d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| {
            Error::Unsupported(
                "Official installer has no SHA-256 integrity digest; it will not be executed"
                    .into(),
            )
        })?;
    Ok(InstallerRelease {
        version: release.tag_name,
        filename: asset.name,
        url: asset.browser_download_url,
        size: asset.size,
        sha256: digest.to_ascii_lowercase(),
    })
}

fn validate_download_url(url: &str) -> Result<()> {
    let uri: isahc::http::Uri = url
        .parse()
        .map_err(|_| Error::Unsupported("Invalid official download URL".into()))?;
    if uri.scheme_str() != Some("https")
        || !matches!(
            uri.authority().map(|a| a.as_str()),
            Some(
                "github.com"
                    | "api.github.com"
                    | "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
            )
        )
        || url.contains('#')
    {
        return Err(Error::Unsupported(
            "Official download redirected outside approved HTTPS hosts".into(),
        ));
    }
    Ok(())
}

fn request(url: &str) -> Result<isahc::Response<isahc::Body>> {
    let mut url = url.to_owned();
    for _ in 0..6 {
        validate_download_url(&url)?;
        let response = isahc::Request::get(&url)
            .header("User-Agent", "RomMFS")
            .header(
                "Accept",
                "application/octet-stream, application/vnd.github+json",
            )
            .connect_timeout(Duration::from_secs(15))
            .low_speed_timeout(1, Duration::from_secs(30))
            .body(())
            .map_err(|_| Error::Transport("Installer request could not be built".into()))?
            .send()
            .map_err(|_| {
                Error::Transport("Official installer request failed; check your connection".into())
            })?;
        if response.status().is_redirection() {
            url = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| {
                    Error::Unsupported("Official download has an invalid redirect".into())
                })?
                .to_owned();
            continue;
        }
        if !response.status().is_success() {
            return Err(Error::Http {
                status: response.status().as_u16(),
                message: "Official RetroBat release request failed".into(),
            });
        }
        return Ok(response);
    }
    Err(Error::Unsupported(
        "Too many official download redirects".into(),
    ))
}

pub fn discover_installer() -> Result<InstallerRelease> {
    let mut response = request(RELEASE_API)?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .take(MAX_RELEASE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RELEASE_BYTES {
        return Err(Error::Unsupported(
            "Official release metadata exceeds its inspection limit".into(),
        ));
    }
    parse_release(&bytes)
}

/// Stream into a private temporary file; publish only complete digest-verified
/// bytes. Cancellation never leaves an executable partial download.
pub fn download_installer(
    release: &InstallerRelease,
    directory: &Path,
    active: &AtomicBool,
    progress: impl FnMut(u64, u64),
) -> Result<PathBuf> {
    if !active.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    let mut response = request(&release.url)?;
    receive_installer(release, directory, response.body_mut(), active, progress)
}

fn receive_installer(
    release: &InstallerRelease,
    directory: &Path,
    reader: &mut impl Read,
    active: &AtomicBool,
    mut progress: impl FnMut(u64, u64),
) -> Result<PathBuf> {
    ensure_no_reparse_components(directory)?;
    std::fs::create_dir_all(directory)?;
    let _guard = hold_save_directory_chain(directory)?;
    let mut temp = tempfile::NamedTempFile::new_in(directory)?;
    let mut hash = Sha256::new();
    let mut received = 0;
    let mut buffer = [0; 256 * 1024];
    loop {
        if !active.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        received += n as u64;
        if received > release.size {
            return Err(Error::Unsupported(
                "Installer exceeds the official release size".into(),
            ));
        }
        temp.write_all(&buffer[..n])?;
        hash.update(&buffer[..n]);
        progress(received, release.size);
    }
    if received != release.size || format!("{:x}", hash.finalize()) != release.sha256 {
        return Err(Error::Unsupported(
            "Installer integrity check failed; no installer was published or executed".into(),
        ));
    }
    if !active.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    temp.as_file().sync_all()?;
    let destination = directory.join(format!("{}-{}", release.sha256, release.filename));
    ensure_no_reparse_components(&destination)?;
    match temp.persist_noclobber(&destination) {
        Ok(_) => {}
        Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
            let mut existing = verified_file(release, &destination)?;
            verify_bytes(release, &mut existing)?;
        }
        Err(e) => return Err(Error::Io(e.error)),
    }
    Ok(destination)
}

fn verified_file(release: &InstallerRelease, path: &Path) -> Result<File> {
    ensure_no_reparse_components(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Prevent replacement or modification between verification and CreateProcess.
        options.share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() != release.size {
        return Err(Error::Unsupported(
            "Cached installer is not the official release file".into(),
        ));
    }
    Ok(file)
}
fn verify_bytes(release: &InstallerRelease, file: &mut File) -> Result<()> {
    let mut hash = Sha256::new();
    std::io::copy(file, &mut hash)?;
    if format!("{:x}", hash.finalize()) != release.sha256 {
        return Err(Error::Unsupported(
            "Cached installer integrity check failed".into(),
        ));
    }
    Ok(())
}

/// A launched installer process. The wizard is independent of RomMFS; this
/// handle only lets a worker thread wait for it to exit.
pub struct LaunchedInstaller {
    #[cfg(windows)]
    process: ProcessHandle,
}

impl LaunchedInstaller {
    /// Block until the installer exits and return its exit code.
    pub fn wait(self) -> Result<u32> {
        #[cfg(windows)]
        {
            self.process.wait()
        }
        #[cfg(not(windows))]
        {
            Err(Error::Unsupported(
                "RetroBat installer execution is available on Windows only".into(),
            ))
        }
    }
}

#[cfg(windows)]
struct ProcessHandle(windows_sys::Win32::Foundation::HANDLE);
// A process handle may be used and closed from any thread.
#[cfg(windows)]
unsafe impl Send for ProcessHandle {}
#[cfg(windows)]
impl ProcessHandle {
    fn wait(&self) -> Result<u32> {
        use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, WaitForSingleObject, INFINITE,
        };
        // SAFETY: the handle is a valid process handle owned by `self`.
        unsafe {
            if WaitForSingleObject(self.0, INFINITE) != WAIT_OBJECT_0 {
                return Err(Error::Io(std::io::Error::last_os_error()));
            }
            let mut code = 0u32;
            if GetExitCodeProcess(self.0, &mut code) == 0 {
                return Err(Error::Io(std::io::Error::last_os_error()));
            }
            Ok(code)
        }
    }
}
#[cfg(windows)]
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        // SAFETY: the handle is owned by `self` and closed exactly once.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}

/// Launch the official wizard elevated (it requires administrator rights, which
/// CreateProcess cannot grant) without guessing unsupported unattended flags.
/// Its own Cancel action controls installation; RomMFS never kills it mid-write.
pub fn launch_installer(release: &InstallerRelease, path: &Path) -> Result<LaunchedInstaller> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Unsupported("Installer has no parent folder".into()))?;
    let _guard = hold_save_directory_chain(parent)?;
    let mut file = verified_file(release, path)?;
    verify_bytes(release, &mut file)?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::{
            ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
        };
        fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
            value.encode_wide().chain(std::iter::once(0)).collect()
        }
        const SW_SHOWNORMAL: i32 = 1;
        let verb = wide(std::ffi::OsStr::new("runas"));
        let file_name = wide(path.as_os_str());
        let directory = wide(parent.as_os_str());
        // SAFETY: zeroed is a valid initial SHELLEXECUTEINFOW (null pointers,
        // zero fields); the NUL-terminated buffers outlive the call.
        let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file_name.as_ptr();
        info.lpDirectory = directory.as_ptr();
        info.nShow = SW_SHOWNORMAL;
        // SAFETY: `info` is fully initialised and its pointers stay valid.
        if unsafe { ShellExecuteExW(&mut info) } == 0 {
            return Err(Error::Io(std::io::Error::last_os_error()));
        }
        if info.hProcess.is_null() {
            return Err(Error::Unsupported(
                "Windows did not return a handle for the installer process".into(),
            ));
        }
        Ok(LaunchedInstaller {
            process: ProcessHandle(info.hProcess),
        })
    }
    #[cfg(not(windows))]
    {
        Err(Error::Unsupported(
            "RetroBat installer execution is available on Windows only".into(),
        ))
    }
}

/// Plain-language outcome of a failed installer launch.
#[derive(Debug, PartialEq, Eq)]
pub struct LaunchFailure {
    /// The user declined the Windows permission prompt; not a real failure.
    pub cancelled: bool,
    pub detail: String,
    /// Plain cause first, raw OS text last; `None` when cancelled.
    pub explanation: Option<String>,
}

const ERROR_CANCELLED: i32 = 1223;

pub fn describe_launch_failure(error: &Error) -> LaunchFailure {
    match error {
        Error::Io(io) => launch_failure_for_os_code(io.raw_os_error(), &io.to_string()),
        other => LaunchFailure {
            cancelled: false,
            detail: "Couldn't start the RetroBat installer".into(),
            explanation: Some(other.to_string()),
        },
    }
}

pub fn launch_failure_for_os_code(code: Option<i32>, raw: &str) -> LaunchFailure {
    if code == Some(ERROR_CANCELLED) {
        return LaunchFailure {
            cancelled: true,
            detail: "Installation was cancelled \u{2014} Windows needs your permission to run the RetroBat installer.".into(),
            explanation: None,
        };
    }
    let cause = match code {
        Some(2 | 3) => "The installer file could not be found.",
        Some(5) => "Windows denied access to the installer file.",
        Some(740) => "The installer needs administrator permission.",
        Some(1260 | 1261) => "A Windows policy blocked the installer.",
        _ => "Windows could not run the installer.",
    };
    LaunchFailure {
        cancelled: false,
        detail: "Couldn't start the RetroBat installer".into(),
        explanation: Some(format!("{cause} Technical detail: {raw}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn release(bytes: &[u8]) -> InstallerRelease {
        InstallerRelease {
            version: "compatible-future".into(),
            filename: "setup.exe".into(),
            url: String::new(),
            size: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(bytes)),
        }
    }
    #[test]
    fn integrity_cancellation_and_existing_files_are_safe() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = b"fixture executable";
        let release = release(bytes);
        let active = AtomicBool::new(true);
        let mut counts = Vec::new();
        let path = receive_installer(&release, dir.path(), &mut &bytes[..], &active, |n, t| {
            counts.push((n, t))
        })
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(
            counts.last(),
            Some(&(bytes.len() as u64, bytes.len() as u64))
        );
        assert!(receive_installer(
            &release,
            dir.path(),
            &mut &b"corrupt"[..],
            &active,
            |_, _| {}
        )
        .is_err());
        assert!(
            receive_installer(&release, dir.path(), &mut &bytes[..], &active, |_, _| {}).is_ok()
        );
        active.store(false, Ordering::Release);
        assert!(matches!(
            receive_installer(&release, dir.path(), &mut &bytes[..], &active, |_, _| {}),
            Err(Error::Cancelled)
        ));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        std::fs::write(&path, b"unrelated contents").unwrap();
        active.store(true, Ordering::Release);
        assert!(
            receive_installer(&release, dir.path(), &mut &bytes[..], &active, |_, _| {}).is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"unrelated contents");
    }
    #[test]
    fn discovery_requires_official_stable_unique_digest_backed_package() {
        let json = serde_json::json!({"tag_name":"9.0", "draft":false,"prerelease":false,"assets":[{"name":"RetroBat-v9.0-stable-win64-setup.exe","browser_download_url":"https://github.com/RetroBat-Official/retrobat/releases/download/9.0/setup.exe","size":99,"digest":format!("sha256:{}", "a".repeat(64))}]});
        assert_eq!(
            parse_release(&serde_json::to_vec(&json).unwrap())
                .unwrap()
                .version(),
            "9.0"
        );
        for key in ["draft", "prerelease"] {
            let mut bad = json.clone();
            bad[key] = true.into();
            assert!(parse_release(&serde_json::to_vec(&bad).unwrap()).is_err());
        }
        let mut bad = json.clone();
        bad["assets"][0]["digest"] = serde_json::Value::Null;
        assert!(parse_release(&serde_json::to_vec(&bad).unwrap()).is_err());
        for url in [
            "http://github.com/file",
            "https://github.com.evil/file",
            "https://user@github.com/file",
            "https://github.com:443/file",
            "file:///tmp/package",
        ] {
            assert!(validate_download_url(url).is_err(), "{url}");
        }
    }
    #[test]
    fn launch_failures_are_described_in_plain_language() {
        let declined = launch_failure_for_os_code(Some(1223), "cancelled by the user");
        assert!(declined.cancelled);
        assert!(declined.detail.starts_with("Installation was cancelled"));
        assert_eq!(declined.explanation, None);
        let elevation = launch_failure_for_os_code(Some(740), "raw elevation text");
        assert!(!elevation.cancelled);
        assert_eq!(elevation.detail, "Couldn't start the RetroBat installer");
        assert_eq!(
            elevation.explanation.as_deref(),
            Some("The installer needs administrator permission. Technical detail: raw elevation text")
        );
        let unknown = launch_failure_for_os_code(None, "odd");
        assert!(unknown
            .explanation
            .unwrap()
            .starts_with("Windows could not"));
        let io = describe_launch_failure(&Error::Io(std::io::Error::from_raw_os_error(1223)));
        assert!(io.cancelled);
        let other = describe_launch_failure(&Error::Unsupported("integrity".into()));
        assert_eq!(other.explanation.as_deref(), Some("integrity"));
    }
}
