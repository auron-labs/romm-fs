//! PS1/PS2 memory-card sync domain.
//!
//! A card is a file (DuckStation `.mcd`, PCSX2 `.ps2`) or a folder (PCSX2
//! folder cards) under the emulator's RetroBat `memcards` directory. RomM
//! stores each card per (user, emulator) as a history of zip versions, so the
//! identity here is (emulator, card name) plus the accepted version id and the
//! hash of the local card. This module holds only portable, network-free
//! logic: discovery, hashing, the decision rules, and bounded archive
//! read/write. The actor drives it and the client lives in `crate::romm`.

use super::path::ensure_no_reparse_components;
use crate::error::{Error, Result};
use std::fs;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

/// The server caps a card transfer here; the plan's "up to 256MB" limit.
pub use crate::romm::MAX_MEMORY_CARD_BYTES as MEMORY_CARD_MAX_BYTES;

/// Mirrors RomM's `EMULATOR_NAME_RE` (`^[A-Za-z0-9][A-Za-z0-9 ._-]*$`): the
/// emulator is a folder name under the user's memory_cards directory, not a
/// fixed emulator list.
pub fn validate_emulator_name(name: &str) -> Result<()> {
    let mut characters = name.chars();
    let first_is_alphanumeric = characters.next().is_some_and(|c| c.is_ascii_alphanumeric());
    let rest_are_valid = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-'));
    if first_is_alphanumeric && rest_are_valid {
        Ok(())
    } else {
        Err(Error::Unsupported(format!(
            "not a valid RomM emulator name: {name:?}"
        )))
    }
}

/// Only the two emulators the plan names; no speculative emulator support.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryCardEmulator {
    Duckstation,
    Pcsx2,
}

impl MemoryCardEmulator {
    pub const ALL: [MemoryCardEmulator; 2] = [Self::Duckstation, Self::Pcsx2];

    pub fn name(self) -> &'static str {
        match self {
            Self::Duckstation => "duckstation",
            Self::Pcsx2 => "pcsx2",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Duckstation => "PS1 memory cards (DuckStation)",
            Self::Pcsx2 => "PS2 memory cards (PCSX2)",
        }
    }

    /// Relative to the effective RetroBat `saves` root.
    pub fn relative_dir(self) -> &'static str {
        match self {
            Self::Duckstation => "psx/duckstation/memcards",
            Self::Pcsx2 => "ps2/pcsx2/memcards",
        }
    }

    pub fn file_extension(self) -> &'static str {
        match self {
            Self::Duckstation => "mcd",
            Self::Pcsx2 => "ps2",
        }
    }

    /// DuckStation uses file cards only; PCSX2 also has folder cards.
    pub fn allows_folder_cards(self) -> bool {
        matches!(self, Self::Pcsx2)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredCard {
    /// RomM card name: the file stem for a file card, the folder name otherwise.
    pub name: String,
    /// Save-root-relative path of the card file or folder.
    pub relative_path: PathBuf,
    pub folder: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardFolder {
    pub emulator: MemoryCardEmulator,
    pub label: &'static str,
    pub relative_dir: PathBuf,
    pub cards: Vec<DiscoveredCard>,
}

/// Discover one emulator's card folder. The folder is returned whenever the
/// `memcards` directory exists, even with no cards yet, so a remote-only card
/// can still be consented to and restored. A missing or non-directory folder is
/// skipped; a reparse point is not followed.
pub fn discover_card_folder(
    saves_root: &Path,
    emulator: MemoryCardEmulator,
) -> Result<Option<CardFolder>> {
    let relative_dir = PathBuf::from(emulator.relative_dir());
    let dir = saves_root.join(&relative_dir);
    match fs::symlink_metadata(&dir) {
        Ok(metadata) if metadata.is_dir() && !super::path_is_reparse_point(&metadata) => {}
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    ensure_no_reparse_components(&dir)?;
    let cards = scan_cards(&dir, &relative_dir, emulator)?;
    Ok(Some(CardFolder {
        emulator,
        label: emulator.label(),
        relative_dir,
        cards,
    }))
}

/// Every emulator card folder present under a save root.
pub fn discover_card_folders(saves_root: &Path) -> Result<Vec<CardFolder>> {
    let mut folders = Vec::new();
    for emulator in MemoryCardEmulator::ALL {
        if let Some(folder) = discover_card_folder(saves_root, emulator)? {
            folders.push(folder);
        }
    }
    Ok(folders)
}

fn scan_cards(
    dir: &Path,
    relative_dir: &Path,
    emulator: MemoryCardEmulator,
) -> Result<Vec<DiscoveredCard>> {
    let mut cards = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if super::path_is_reparse_point(&metadata) {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().into_owned();
        // Conflict siblings and staging artifacts live in the same folder;
        // they are never cards.
        if file_name.contains(".rommfs-") || file_name.starts_with(".rommfs") {
            continue;
        }
        if metadata.is_file() {
            let extension = Path::new(&file_name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case(emulator.file_extension()));
            let Some(stem) = Path::new(&file_name).file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if extension {
                cards.push(DiscoveredCard {
                    name: stem.to_owned(),
                    relative_path: relative_dir.join(&file_name),
                    folder: false,
                });
            }
        } else if metadata.is_dir() && emulator.allows_folder_cards() {
            cards.push(DiscoveredCard {
                name: file_name.clone(),
                relative_path: relative_dir.join(&file_name),
                folder: true,
            });
        }
    }
    cards.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(cards)
}

/// A stable hash of the local card as it stands now. A file hashes its bytes;
/// a folder hashes its sorted `name:content-hash` lines so an untouched folder
/// card compares equal across scans.
pub fn hash_local_card(path: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path)?;
    if super::path_is_reparse_point(&metadata) {
        return Err(Error::Unsupported(
            "memory card path is a symbolic link or reparse point".into(),
        ));
    }
    if metadata.is_file() {
        return hash_file(path);
    }
    if metadata.is_dir() {
        let mut parts = Vec::new();
        collect_folder_hashes(path, path, &mut parts)?;
        parts.sort();
        return Ok(super::sha256_content_hash(parts.join("\n").as_bytes()));
    }
    Err(Error::Unsupported(
        "memory card is not a file or folder".into(),
    ))
}

fn hash_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    super::hash_save_content(&mut file)
}

/// The card's current hash: `Ok(None)` when it does not exist, an error when it
/// exists but cannot be read or is a reparse point. Callers must never treat an
/// error as "absent" — doing so would overwrite an unreadable local card.
pub fn read_local_card_hash(path: &Path) -> Result<Option<String>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            ensure_not_reparse(&metadata, path)?;
            Ok(Some(hash_local_card(path)?))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn collect_folder_hashes(root: &Path, dir: &Path, parts: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        // A reparse point inside a folder card is rejected, not skipped: it is
        // part of the card, and replacing the folder would drop it.
        ensure_not_reparse(&metadata, &path)?;
        let relative = relative_name(root, &path);
        if metadata.is_dir() {
            collect_folder_hashes(root, &path, parts)?;
        } else if metadata.is_file() {
            parts.push(format!("{relative}:{}", hash_file(&path)?));
        }
    }
    Ok(())
}

fn ensure_not_reparse(metadata: &fs::Metadata, path: &Path) -> Result<()> {
    if super::path_is_reparse_point(metadata) {
        return Err(Error::Unsupported(format!(
            "memory card contains a symbolic link or reparse point: {}",
            path.display()
        )));
    }
    Ok(())
}

fn relative_name(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The most recent modification time across a card's files, used for the quiet
/// period before an upload. `None` when the card does not exist.
pub fn latest_card_modified(path: &Path) -> Result<Option<std::time::SystemTime>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if super::path_is_reparse_point(&metadata) {
        return Err(Error::Unsupported(
            "memory card path is a symbolic link or reparse point".into(),
        ));
    }
    if metadata.is_file() {
        return Ok(metadata.modified().ok());
    }
    if metadata.is_dir() {
        let mut latest = metadata.modified().ok();
        collect_latest_modified(path, &mut latest)?;
        return Ok(latest);
    }
    Ok(None)
}

fn collect_latest_modified(dir: &Path, latest: &mut Option<std::time::SystemTime>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        ensure_not_reparse(&metadata, &path)?;
        record_modified(metadata.modified().ok(), latest);
        if metadata.is_dir() {
            collect_latest_modified(&path, latest)?;
        }
    }
    Ok(())
}

fn record_modified(
    candidate: Option<std::time::SystemTime>,
    latest: &mut Option<std::time::SystemTime>,
) {
    if let Some(candidate) = candidate {
        if latest.is_none_or(|current| candidate > current) {
            *latest = Some(candidate);
        }
    }
}

fn card_size(path: &Path) -> Result<u64> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_file() {
        Ok(metadata.len())
    } else if metadata.is_dir() {
        let mut total = 0u64;
        collect_folder_size(path, &mut total)?;
        Ok(total)
    } else {
        Err(Error::Unsupported(
            "memory card is not a file or folder".into(),
        ))
    }
}

fn collect_folder_size(dir: &Path, total: &mut u64) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        ensure_not_reparse(&metadata, &path)?;
        if metadata.is_dir() {
            collect_folder_size(&path, total)?;
        } else if metadata.is_file() {
            *total = total.saturating_add(metadata.len());
        }
    }
    Ok(())
}

/// Write the card as a zip whose entry names are the card's path relative to
/// the `memcards` directory, so unpacking into that directory recreates the
/// card exactly. RomM only requires a safe, readable zip; the broker's exact
/// unpack target is not pinned, so this preserves the card's own layout.
pub fn write_card_archive<W: Write + Seek>(card_path: &Path, archive: W) -> Result<()> {
    let metadata = fs::symlink_metadata(card_path)?;
    if super::path_is_reparse_point(&metadata) {
        return Err(Error::Unsupported(
            "memory card path is a symbolic link or reparse point".into(),
        ));
    }
    if card_size(card_path)? > MEMORY_CARD_MAX_BYTES {
        return Err(Error::Unsupported(format!(
            "memory card exceeds the {MEMORY_CARD_MAX_BYTES}-byte transfer limit"
        )));
    }
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut zip = zip::ZipWriter::new(archive);
    if metadata.is_file() {
        let name = card_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| Error::Unsupported("memory card name is not valid Unicode".into()))?;
        add_archive_file(&mut zip, options, name, card_path)?;
    } else if metadata.is_dir() {
        let mut files = Vec::new();
        collect_folder_files(card_path, card_path, &mut files)?;
        files.sort();
        for (name, path) in files {
            add_archive_file(&mut zip, options, &name, &path)?;
        }
    } else {
        return Err(Error::Unsupported(
            "memory card is not a file or folder".into(),
        ));
    }
    zip.finish().map_err(zip_error)?;
    Ok(())
}

fn add_archive_file<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    options: zip::write::SimpleFileOptions,
    name: &str,
    path: &Path,
) -> Result<()> {
    if validate_archive_path(name).is_err() {
        return Err(Error::Unsupported(format!(
            "memory card entry has an unsafe name: {name:?}"
        )));
    }
    zip.start_file(name.replace('\\', "/"), options)
        .map_err(zip_error)?;
    let mut file = fs::File::open(path)?;
    std::io::copy(&mut file, zip)?;
    Ok(())
}

fn collect_folder_files(root: &Path, dir: &Path, files: &mut Vec<(String, PathBuf)>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        ensure_not_reparse(&metadata, &path)?;
        let relative = relative_name(root, &path);
        if metadata.is_dir() {
            collect_folder_files(root, &path, files)?;
        } else if metadata.is_file() {
            files.push((relative, path));
        }
    }
    Ok(())
}

/// Extract a downloaded card archive onto `destination`: the card file for a
/// file card, the card directory for a folder card. The extract is bounded to
/// [`MEMORY_CARD_MAX_BYTES`] of decompressed data and rejects traversal,
/// absolute paths, and symlink entries. It is staged beside the destination
/// and renamed into place, so a failed extract never truncates the existing
/// card.
pub fn extract_card_archive(archive: &Path, destination: &Path, folder: bool) -> Result<()> {
    let file = fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).map_err(zip_error)?;
    let parent = destination.parent().ok_or_else(|| {
        Error::Unsupported("memory card destination has no parent directory".into())
    })?;
    ensure_no_reparse_components(parent)?;
    fs::create_dir_all(parent)?;
    let staging = parent.join(format!(".rommfs-card-{}", uuid::Uuid::new_v4()));
    if folder {
        extract_card_folder(&mut zip, &staging)?;
        if let Err(error) = replace_directory(&staging, destination) {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        Ok(())
    } else {
        if let Err(error) = extract_card_file(&mut zip, &staging) {
            let _ = fs::remove_file(&staging);
            return Err(error);
        }
        if let Err(error) = replace_file(&staging, destination) {
            let _ = fs::remove_file(&staging);
            return Err(error);
        }
        Ok(())
    }
}

fn extract_card_file(zip: &mut zip::ZipArchive<fs::File>, staging: &Path) -> Result<()> {
    let mut index = None;
    for candidate in 0..zip.len() {
        let entry = zip.by_index(candidate).map_err(zip_error)?;
        // Every entry is validated, not just the one that is written.
        validate_entry(&entry)?;
        if entry.is_dir() {
            continue;
        }
        if index.is_some() {
            return Err(Error::Unsupported(
                "memory card archive holds more than one file".into(),
            ));
        }
        index = Some(candidate);
    }
    let Some(index) = index else {
        return Err(Error::Unsupported("memory card archive is empty".into()));
    };
    let mut entry = zip.by_index(index).map_err(zip_error)?;
    let mut output = fs::File::create(staging)?;
    copy_bounded(&mut entry, &mut output, MEMORY_CARD_MAX_BYTES)?;
    Ok(())
}

fn extract_card_folder(zip: &mut zip::ZipArchive<fs::File>, staging: &Path) -> Result<()> {
    fs::create_dir(staging)?;
    let result = (|| {
        let mut budget = MEMORY_CARD_MAX_BYTES;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index).map_err(zip_error)?;
            let components = validate_entry(&entry)?;
            let mut output_path = staging.to_path_buf();
            for component in &components {
                output_path.push(component);
            }
            if entry.is_dir() {
                fs::create_dir_all(&output_path)?;
                continue;
            }
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut output = fs::File::create(&output_path)?;
            budget = copy_bounded(&mut entry, &mut output, budget)?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(staging);
    }
    result
}

fn replace_file(staging: &Path, destination: &Path) -> Result<()> {
    // `symlink_metadata` also catches a broken symlink, which `exists()` misses.
    if fs::symlink_metadata(destination).is_ok() {
        ensure_no_reparse_components(destination)?;
        let backup = backup_path(destination)?;
        fs::rename(destination, &backup)?;
        match fs::rename(staging, destination) {
            Ok(()) => {
                let _ = fs::remove_file(&backup);
                Ok(())
            }
            Err(error) => {
                let _ = fs::rename(&backup, destination);
                Err(error.into())
            }
        }
    } else {
        fs::rename(staging, destination)?;
        Ok(())
    }
}

fn replace_directory(staging: &Path, destination: &Path) -> Result<()> {
    // `symlink_metadata` also catches a broken symlink, which `exists()` misses.
    if fs::symlink_metadata(destination).is_ok() {
        ensure_no_reparse_components(destination)?;
        let backup = backup_path(destination)?;
        fs::rename(destination, &backup)?;
        match fs::rename(staging, destination) {
            Ok(()) => {
                let _ = fs::remove_dir_all(&backup);
                Ok(())
            }
            Err(error) => {
                let _ = fs::rename(&backup, destination);
                Err(error.into())
            }
        }
    } else {
        fs::rename(staging, destination)?;
        Ok(())
    }
}

/// A sibling that holds the previous card while a replacement is renamed in.
/// The `.rommfs-` prefix keeps it out of card discovery and ordinary mapping.
fn backup_path(destination: &Path) -> Result<PathBuf> {
    let parent = destination
        .parent()
        .ok_or_else(|| Error::Unsupported("memory card destination has no parent".into()))?;
    Ok(parent.join(format!(".rommfs-backup-{}", uuid::Uuid::new_v4())))
}

/// Validate one archive entry and return its normalized path components. Every
/// component is checked with the same Windows-name validator used for save
/// targets, so ADS (`name:stream`), reserved device names, trailing dots/spaces
/// and control characters are rejected. Both `/` and `\` split the name, so a
/// hand-written entry cannot hide a traversal in one opaque part.
fn validate_entry(entry: &zip::read::ZipFile<'_>) -> Result<Vec<String>> {
    if entry.is_symlink() {
        return Err(Error::Unsupported(
            "memory card archive contains a symbolic link".into(),
        ));
    }
    validate_archive_path(entry.name())
}

fn validate_archive_path(name: &str) -> Result<Vec<String>> {
    if name.is_empty() || name.contains('\0') {
        return Err(Error::Unsupported(
            "memory card archive has an empty entry name".into(),
        ));
    }
    if name.starts_with('/') || name.starts_with('\\') {
        return Err(Error::Unsupported(format!(
            "memory card archive contains an absolute path: {name}"
        )));
    }
    let bytes = name.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return Err(Error::Unsupported(format!(
            "memory card archive contains a drive path: {name}"
        )));
    }
    let mut components = Vec::new();
    for part in name.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(Error::Unsupported(format!(
                "memory card archive contains a traversal path: {name}"
            )));
        }
        super::validate_windows_path_component(part)
            .map_err(|error| Error::Unsupported(format!("{error}: {name}")))?;
        components.push(part.to_owned());
    }
    if components.is_empty() {
        return Err(Error::Unsupported(
            "memory card archive has an empty entry name".into(),
        ));
    }
    Ok(components)
}

fn copy_bounded(reader: &mut impl Read, writer: &mut impl Write, budget: u64) -> Result<u64> {
    let mut remaining = budget;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Ok(remaining);
        }
        if count as u64 > remaining {
            return Err(Error::Unsupported(format!(
                "memory card archive unpacks to over {MEMORY_CARD_MAX_BYTES} bytes"
            )));
        }
        remaining -= count as u64;
        writer.write_all(&buffer[..count])?;
    }
}

fn zip_error(error: zip::result::ZipError) -> Error {
    Error::Io(std::io::Error::other(error))
}

/// The accepted state of one card: the RomM version id and the local card hash
/// at the last reconciliation, plus the remote content hash then.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CardBaseline {
    pub version_id: Option<i64>,
    pub local_hash: Option<String>,
    pub remote_hash: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteCardVersion {
    pub id: i64,
    pub content_hash: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardDecision {
    /// Local and remote both match the baseline.
    Settled,
    /// The local card changed (or the remote baseline is gone); upload it.
    Upload,
    /// The remote is newer and the local card has not changed; download it.
    Download { version_id: i64 },
    /// Both sides changed; keep the local card and save the remote beside it.
    Conflict { version_id: i64 },
    /// A shared card is written for the whole session, so nothing syncs until
    /// the emulator has exited.
    EmulatorRunning,
}

/// Decide one card's action. `local_hash` is the card's hash as it is now
/// (`None` when the card is absent); `remote` is the newest remote version
/// (`None` when the card has no stored data yet).
pub fn plan_card_action(
    local_hash: Option<&str>,
    baseline: &CardBaseline,
    remote: Option<&RemoteCardVersion>,
    emulator_running: bool,
) -> CardDecision {
    if emulator_running {
        return CardDecision::EmulatorRunning;
    }
    let local_changed = match (local_hash, baseline.local_hash.as_deref()) {
        (Some(local), Some(base)) => local != base,
        (Some(_), None) => true,
        (None, Some(_)) => true,
        (None, None) => false,
    };
    let remote_changed = remote.map_or(baseline.version_id.is_some(), |version| {
        Some(version.id) != baseline.version_id
    });
    match (local_changed, remote_changed) {
        (false, false) => CardDecision::Settled,
        (false, true) => match remote {
            Some(version) => CardDecision::Download {
                version_id: version.id,
            },
            None => local_action(local_hash),
        },
        (true, false) => local_action(local_hash),
        (true, true) => match remote {
            Some(version) => CardDecision::Conflict {
                version_id: version.id,
            },
            None => local_action(local_hash),
        },
    }
}

fn local_action(local_hash: Option<&str>) -> CardDecision {
    if local_hash.is_some() {
        CardDecision::Upload
    } else {
        CardDecision::Settled
    }
}

/// The sibling path that keeps a remote copy beside a local card instead of
/// overwriting it. The `.rommfs-conflict` marker is already excluded from
/// ordinary save mapping, so the copy never becomes a per-game save.
/// The local destination for a remote-only card, inferred from its archive.
/// A single top-level file entry restores as `<card>.<ext>`; any other layout
/// restores as a folder named after the card. `card_name` comes from the
/// server, so it is validated as one safe path component first.
pub struct RemoteCardRestore {
    pub relative_path: PathBuf,
    pub folder: bool,
}

pub fn remote_card_restore_target(
    archive: &Path,
    emulator: MemoryCardEmulator,
    card_name: &str,
) -> Result<RemoteCardRestore> {
    super::path::validate_windows_path_component(card_name).map_err(Error::Unsupported)?;
    let relative_dir = PathBuf::from(emulator.relative_dir());
    let file = fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).map_err(zip_error)?;
    let mut files: Vec<Vec<String>> = Vec::new();
    for index in 0..zip.len() {
        let entry = zip.by_index(index).map_err(zip_error)?;
        // Every entry is validated, including directories.
        let components = validate_entry(&entry)?;
        if !entry.is_dir() {
            files.push(components);
        }
    }
    // A file card is exactly one top-level file entry carrying the emulator's
    // own extension. A folder layout (or a wrong extension) is never guessed
    // into a file card.
    let expected = format!(".{}", emulator.file_extension());
    let file_layout = files.len() == 1
        && files[0].len() == 1
        && files[0][0].to_ascii_lowercase().ends_with(&expected);
    if file_layout {
        return Ok(RemoteCardRestore {
            relative_path: relative_dir.join(format!("{card_name}.{}", emulator.file_extension())),
            folder: false,
        });
    }
    if !emulator.allows_folder_cards() {
        return Err(Error::Unsupported(
            "a DuckStation memory card must be a single .mcd file".into(),
        ));
    }
    Ok(RemoteCardRestore {
        relative_path: relative_dir.join(card_name),
        folder: true,
    })
}

/// The sibling path that keeps one remote version beside a local card instead
/// of overwriting it. The version id is part of the name, so a conflict
/// reported for a newer remote version is preserved as its own file rather than
/// silently reusing older bytes. The `.rommfs-conflict` marker is excluded from
/// ordinary save mapping and from card discovery.
pub fn conflict_sibling(path: &Path, folder: bool, version_id: i64) -> Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Unsupported("memory card path has no parent directory".into()))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Error::Unsupported("memory card name is not valid Unicode".into()))?;
    if folder {
        return Ok(parent.join(format!("{name}.rommfs-conflict-v{version_id}")));
    }
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(name);
    let sibling = match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) => format!("{stem}.rommfs-conflict-v{version_id}.{extension}"),
        None => format!("{stem}.rommfs-conflict-v{version_id}"),
    };
    Ok(parent.join(sibling))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_copy_stops_at_the_budget() {
        let mut source = std::io::Cursor::new(vec![0u8; 10]);
        assert_eq!(copy_bounded(&mut source, &mut Vec::new(), 10).unwrap(), 0);
        let mut source = std::io::Cursor::new(vec![0u8; 10]);
        let mut sink = Vec::new();
        assert!(copy_bounded(&mut source, &mut sink, 4).is_err());
        assert!(sink.len() <= 4);
    }

    #[test]
    fn archive_entry_paths_are_validated_per_component() {
        for name in [
            "../evil",
            "a/../b",
            "..\\evil",
            "/abs/path",
            "C:/abs",
            "\\\\unc\\share",
            "trailing\\..",
            "Mcd001.mcd:stream",
            "CON",
            "name. ",
            "bad\u{1}name",
        ] {
            assert!(
                validate_archive_path(name).is_err(),
                "expected unsafe: {name}"
            );
        }
        // Valid directories and files survive, with both separators split.
        assert_eq!(
            validate_archive_path("Folder/sub/Mcd001.ps2").unwrap(),
            vec!["Folder", "sub", "Mcd001.ps2"]
        );
        assert_eq!(
            validate_archive_path("nested\\extra.bin").unwrap(),
            vec!["nested", "extra.bin"]
        );
    }
}
