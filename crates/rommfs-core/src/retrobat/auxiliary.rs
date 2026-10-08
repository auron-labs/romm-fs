//! Explicit auxiliary materialization; never runs during catalogue loading.
//! Unknown categories/archives are diagnosed instead of inventing extraction
//! routes. Existing or emulator-modified files are never overwritten.
use crate::romm::{FirmwareDto, PlatformDto, RomDto, RommClient};
use crate::save_sync::{
    ensure_no_reparse_components, hash_save_content, hold_save_directory_chain,
    validate_windows_path_component,
};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
pub struct AuxiliaryReport {
    pub published: usize,
    pub unchanged: usize,
    pub warnings: Vec<String>,
}
#[derive(Default, Deserialize, Serialize)]
struct Index {
    schema: u32,
    server: String,
    account: i64,
    files: BTreeMap<String, Owned>,
}
#[derive(Deserialize, Serialize)]
struct Owned {
    source: String,
    version: String,
    hash: String,
    pending: bool,
}
enum Source {
    Firmware(FirmwareDto),
    Cheat {
        file_id: i64,
        name: String,
        size: u64,
        version: String,
    },
}
impl Source {
    fn identity(&self) -> String {
        match self {
            Self::Firmware(f) => format!("firmware:{}", f.id),
            Self::Cheat { file_id, .. } => format!("rom-file:{file_id}"),
        }
    }
    fn version(&self) -> String {
        match self {
            Self::Firmware(f) => format!("{}:{}", f.updated_at, f.file_size_bytes),
            Self::Cheat { version, .. } => version.clone(),
        }
    }
    fn size(&self) -> u64 {
        match self {
            Self::Firmware(f) => f.file_size_bytes,
            Self::Cheat { size, .. } => *size,
        }
    }
    fn describe(&self) -> String {
        match self {
            Self::Firmware(f) => format!("firmware {} {:?}", f.id, f.file_name),
            Self::Cheat { file_id, name, .. } => format!("ROM file {file_id} {name:?}"),
        }
    }
    fn download(&self, client: &RommClient, writer: &mut dyn Write) -> Result<u64> {
        match self {
            Self::Firmware(f) => client.download_firmware(f, writer),
            Self::Cheat { file_id, name, .. } => {
                client.download_original_file(*file_id, name, writer, &mut |_, _| {})
            }
        }
    }
}
#[allow(clippy::too_many_arguments)]
pub fn materialize_auxiliaries(
    client: &RommClient,
    server: &str,
    account: i64,
    installation: &Path,
    platforms: &[PlatformDto],
    roms: &[RomDto],
    active: &AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> Result<AuxiliaryReport> {
    let (bios, cheats) = super::auxiliary_paths(installation)?;
    let cheats = cheats.join("retroarch");
    let mut report = AuxiliaryReport::default();
    let platforms: HashMap<_, _> = platforms.iter().map(|p| (p.id, p)).collect();
    let mut files = Vec::new();
    match client.firmware() {
        Ok(firmware)=> for f in firmware {
            let result=(||->Result<Vec<PathBuf>> {
                validate_windows_path_component(&f.file_name).map_err(Error::Unsupported)?;
                if f.id<=0 || f.missing_from_fs { return Err(Error::Unsupported("Firmware file is missing; rescan it on RomM".into())); }
                let platform=platforms.get(&f.platform_id).ok_or_else(||Error::Unsupported("Firmware platform is unavailable".into()))?;
                if platform.slug=="switch" || platform.fs_slug=="switch" {
                    let mut destinations=Vec::new();
                    for emulator in ["eden","citron"] {
                        let emulator_root=installation.join("emulators").join(emulator);
                        if !emulator_root.is_dir() { continue; }
                        let destination=if matches!(f.file_name.as_str(),"prod.keys"|"title.keys") { emulator_root.join("user/keys").join(&f.file_name) }
                        else if f.file_name.to_ascii_lowercase().ends_with(".nca") { emulator_root.join("user/nand/system/Contents/registered").join(&f.file_name) }
                        else { return Err(Error::Unsupported("Switch firmware must be individual unpacked .nca files or prod.keys/title.keys; upload unpacked originals to RomM, not an archive".into())); };
                        destinations.push(destination);
                    }
                    if destinations.is_empty() { return Err(Error::Unsupported("Install Eden or Citron before materializing Switch firmware/keys".into())); }
                    Ok(destinations)
                } else { Ok(vec![bios.join(&f.file_name)]) }
            })();
            match result { Ok(paths)=> for path in paths { files.push((path,Source::Firmware(f.clone()))); }, Err(e)=>report.warnings.push(format!("Firmware {}: {e}",f.id)) }
        },
        Err(e)=>report.warnings.push(format!("Firmware inventory unavailable: {e}. Browser sign-in must grant firmware.read; retry after signing in again.")),
    }
    for rom in roms {
        for file in rom
            .files
            .iter()
            .filter(|f| f.category.as_deref() == Some("cheat"))
        {
            let result = (|| -> Result<PathBuf> {
                if rom.missing_from_fs || file.id <= 0 {
                    return Err(Error::Unsupported(
                        "Cheat file is missing; rescan it on RomM".into(),
                    ));
                }
                if !file.file_name.to_ascii_lowercase().ends_with(".cht") {
                    return Err(Error::Unsupported("Only RetroArch .cht files have a verified cheat destination; other emulator cheats need a specific destination contract".into()));
                }
                let platform = platforms
                    .values()
                    .find(|p| p.fs_slug == rom.platform_fs_slug)
                    .ok_or_else(|| Error::Unsupported("Cheat platform is unavailable".into()))?;
                validate_windows_path_component(&platform.fs_slug).map_err(Error::Unsupported)?;
                let relative = crate::layout::relative_file_path(rom, file).ok_or_else(|| {
                    Error::Unsupported("Cheat source path is outside its game".into())
                })?;
                crate::save_sync::validate_relative_save_path(Path::new(&relative))
                    .map_err(Error::Unsupported)?;
                Ok(cheats.join(&platform.fs_slug).join(relative))
            })();
            match result {
                Ok(path) => files.push((
                    path,
                    Source::Cheat {
                        file_id: file.id,
                        name: file.file_name.clone(),
                        size: file.file_size_bytes,
                        version: format!(
                            "{:?}:{:?}:{}",
                            file.sha1_hash, file.last_modified, file.file_size_bytes
                        ),
                    },
                )),
                Err(e) => report
                    .warnings
                    .push(format!("ROM {} cheat {}: {e}", rom.id, file.id)),
            }
        }
    }
    let index_dir = installation.join(".rommfs-auxiliary");
    ensure_no_reparse_components(&index_dir)?;
    let _install_guard = hold_save_directory_chain(installation)?;
    fs::create_dir_all(&index_dir)?;
    let _index_guard = hold_save_directory_chain(&index_dir)?;
    let index_path = index_dir.join("ownership.json");
    ensure_no_reparse_components(&index_path)?;
    let mut index = if index_path.exists() {
        let file = File::open(&index_path)?;
        if file.metadata()?.len() > 8 * 1024 * 1024 {
            return Err(Error::Unsupported(
                "Auxiliary ownership index is too large".into(),
            ));
        }
        serde_json::from_reader::<_, Index>(file).map_err(|_| {
            Error::Unsupported(
                "Auxiliary ownership index is invalid; existing files were left untouched".into(),
            )
        })?
    } else {
        Index {
            schema: 1,
            server: server.into(),
            account,
            ..Default::default()
        }
    };
    if index.schema != 1 || index.server != server || index.account != account {
        return Err(Error::Unsupported(
            "Auxiliary files belong to another server/account; existing files were left untouched"
                .into(),
        ));
    }
    // Library entries can legitimately share a destination (the same BIOS
    // uploaded under two platforms). Identical sources publish once; genuinely
    // different sources for one destination are reported and only that file is
    // skipped instead of aborting the whole run.
    let mut grouped: BTreeMap<String, (PathBuf, Vec<Source>)> = BTreeMap::new();
    for (path, source) in files {
        grouped
            .entry(path.to_string_lossy().to_lowercase())
            .or_insert_with(|| (path, Vec::new()))
            .1
            .push(source);
    }
    let mut files = Vec::with_capacity(grouped.len());
    for (_, (destination, group)) in grouped {
        let mut distinct: Vec<Source> = Vec::new();
        for source in group {
            if !distinct.iter().any(|s| s.identity() == source.identity()) {
                distinct.push(source);
            }
        }
        let first = &distinct[0];
        if distinct.iter().all(|s| s.size() == first.size()) {
            // Equal size/version metadata is not content identity. Keep every
            // distinct source so publication can compare the downloaded hashes.
            files.push((destination, distinct));
        } else {
            report.warnings.push(format!(
                "Conflicting auxiliary sources target {}; it was skipped: {}",
                destination.display(),
                distinct
                    .iter()
                    .map(|s| s.describe())
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
    }
    let total = files.len();
    for (n, (path, sources)) in files.into_iter().enumerate() {
        if !active.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        let result = publish(
            client,
            &index_path,
            &mut index,
            &path,
            &sources,
            active,
            &mut report,
        );
        if !active.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        if let Err(e) = result {
            report.warnings.push(format!("{}: {e}", path.display()));
        }
        progress(n + 1, total);
    }
    Ok(report)
}
fn publish(
    client: &RommClient,
    index_path: &Path,
    index: &mut Index,
    path: &Path,
    sources: &[Source],
    active: &AtomicBool,
    report: &mut AuxiliaryReport,
) -> Result<()> {
    let source = sources.first().expect("nonempty auxiliary source group");
    ensure_no_reparse_components(path)?;
    let key = path
        .to_str()
        .ok_or_else(|| Error::Unsupported("Auxiliary destination is not Unicode".into()))?
        .to_owned();
    if path.exists() {
        let owned = index.files.get_mut(&key).ok_or_else(|| {
            Error::Unsupported("Destination is not RomMFS-owned; it was left untouched".into())
        })?;
        let mut file = File::open(path)?;
        if !file.metadata()?.is_file() || hash_save_content(&mut file)? != owned.hash {
            return Err(Error::Unsupported(
                "Emulator/user modified this file; the local version was preserved".into(),
            ));
        }
        if owned.source != source.identity() {
            return Err(Error::Unsupported(
                "Destination belongs to a different source; local bytes were preserved".into(),
            ));
        }
        if owned.version == source.version() {
            owned.pending = false;
            save_index(index_path, index)?;
            report.unchanged += 1;
            return Ok(());
        }
        return Err(Error::Unsupported("Server auxiliary version changed; the installed version was kept. Remove the unchanged owned file explicitly, then materialize again to install the new version".into()));
    }
    let parent = path
        .parent()
        .ok_or_else(|| Error::Unsupported("Auxiliary destination has no parent".into()))?;
    ensure_no_reparse_components(parent)?;
    let existing = parent
        .ancestors()
        .find(|p| p.is_dir())
        .ok_or_else(|| Error::Unsupported("Auxiliary destination has no ancestor".into()))?;
    let mut guard = hold_save_directory_chain(existing)?;
    let mut chain = existing.to_path_buf();
    for part in parent
        .strip_prefix(existing)
        .map_err(|_| Error::Unsupported("Auxiliary parent is invalid".into()))?
        .components()
    {
        chain.push(part.as_os_str());
        match fs::create_dir(&chain) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
        guard = hold_save_directory_chain(&chain)?;
    }
    let _parent_guard = guard;
    let (temp, hash) = download_source(client, parent, source, active)?;
    for other in &sources[1..] {
        let (_, other_hash) = download_source(client, parent, other, active)?;
        if other_hash != hash {
            return Err(Error::Unsupported(format!(
                "Conflicting auxiliary contents; destination was skipped: {}",
                sources
                    .iter()
                    .map(Source::describe)
                    .collect::<Vec<_>>()
                    .join("; ")
            )));
        }
    }
    if !active.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    if sources.len() > 1 {
        report.warnings.push(format!(
            "Content-verified duplicate auxiliary sources for {} publish once: {}",
            path.display(),
            sources
                .iter()
                .map(Source::describe)
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    index.files.insert(
        key.clone(),
        Owned {
            source: source.identity(),
            version: source.version(),
            hash,
            pending: true,
        },
    );
    // Persist intent first: interrupted creation is recoverably owned only when
    // its content hash matches. Never adopt an unrelated file based on a name.
    save_index(index_path, index)?;
    temp.persist_noclobber(path)
        .map_err(|e| Error::Io(e.error))?;
    index
        .files
        .get_mut(&key)
        .expect("publication intent exists")
        .pending = false;
    save_index(index_path, index)?;
    report.published += 1;
    Ok(())
}

fn download_source(
    client: &RommClient,
    parent: &Path,
    source: &Source,
    active: &AtomicBool,
) -> Result<(tempfile::NamedTempFile, String)> {
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    let mut writer = BoundedWriter {
        inner: temp.as_file_mut(),
        active,
        remaining: source.size(),
    };
    source.download(client, &mut writer)?;
    if writer.remaining != 0 {
        return Err(Error::Unsupported(
            "Auxiliary download was shorter than its inventoried size".into(),
        ));
    }
    if !active.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    temp.as_file().sync_all()?;
    let hash = hash_save_content(&mut temp.reopen()?)?;
    Ok((temp, hash))
}
fn save_index(path: &Path, index: &Index) -> Result<()> {
    ensure_no_reparse_components(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| Error::Unsupported("Index has no parent".into()))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(&mut temp, index).map_err(|e| Error::Unsupported(e.to_string()))?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| Error::Io(e.error))?;
    Ok(())
}
struct BoundedWriter<'a> {
    inner: &'a mut File,
    active: &'a AtomicBool,
    remaining: u64,
}
impl Write for BoundedWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if !self.active.load(Ordering::Acquire) {
            return Err(std::io::Error::other("Auxiliary download cancelled"));
        }
        if bytes.len() as u64 > self.remaining {
            return Err(std::io::Error::other(
                "Auxiliary download exceeds its inventoried size",
            ));
        }
        let n = self.inner.write(bytes)?;
        self.remaining -= n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
