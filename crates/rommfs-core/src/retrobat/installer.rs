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
use std::process::{Child, Command};
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

/// Launch the official wizard without guessing unsupported unattended flags.
/// Its own Cancel action controls installation; RomMFS never kills it mid-write.
pub fn launch_installer(release: &InstallerRelease, path: &Path) -> Result<Child> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Unsupported("Installer has no parent folder".into()))?;
    let _guard = hold_save_directory_chain(parent)?;
    let mut file = verified_file(release, path)?;
    verify_bytes(release, &mut file)?;
    #[cfg(windows)]
    {
        Command::new(path)
            .current_dir(parent)
            .spawn()
            .map_err(Error::Io)
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new(path);
        Err(Error::Unsupported(
            "RetroBat installer execution is available on Windows only".into(),
        ))
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
}
