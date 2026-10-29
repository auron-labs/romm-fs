//! Opt-in PS1/PS2 memory-card sync.
//!
//! The reconciliation loop is owned by [`super::SaveSyncAgent`], so it shares
//! the save actor's lifecycle (start/stop/drop). It keeps its own polling thread
//! because a card transfer can be a 256 MB archive that must not block the save
//! actor loop, and a card is scanned on its own quiet-period cadence. It reuses
//! `SaveSyncScope`/`SaveSyncIdentity`, the per-folder consent in
//! `SaveSyncSettingsStore`, the `stopped_retrobat_guard` emulator check, and the
//! portable card domain in `save_sync::memory_cards`.

use rommfs_core::error::Result;
use rommfs_core::events::{
    AppEvent, EventSink, Level, MemoryCardConflictStatus, MemoryCardFolderStatus,
};
use rommfs_core::romm::{MemoryCard, MemoryCardVersion, RommClient, SaveSyncIdentity};
use rommfs_core::save_sync::{
    conflict_sibling, discover_card_folder, discover_card_folders, extract_card_archive,
    hash_local_card, latest_card_modified, plan_card_action, read_local_card_hash,
    remote_card_restore_target, write_card_archive, CardBaseline, CardDecision, CardFolder,
    DiscoveredCard, MemoryCardJournal, RemoteCardVersion, SaveSyncScope,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

const MIN_SCAN_INTERVAL: Duration = Duration::from_secs(2);

pub(crate) struct MemoryCardConfig {
    pub scope: SaveSyncScope,
    /// Folders the user has explicitly consented to.
    pub folders: Vec<CardFolder>,
    pub identity: SaveSyncIdentity,
    pub client: Arc<RommClient>,
    pub journal_path: PathBuf,
    pub debounce_secs: u32,
    pub session_id: u64,
}

/// The card reconciliation thread. Owned by `SaveSyncAgent`.
pub(crate) struct MemoryCardAgent {
    stop: Sender<()>,
    active: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl MemoryCardAgent {
    pub(crate) fn start(config: MemoryCardConfig, sink: EventSink) -> Result<Self> {
        let (stop, stop_rx) = mpsc::channel();
        let active = Arc::new(AtomicBool::new(true));
        let actor_active = Arc::clone(&active);
        let handle = thread::Builder::new()
            .name("rommfs-memory-card".into())
            .spawn(move || run(config, stop_rx, actor_active, sink))?;
        Ok(Self {
            stop,
            active,
            handle: Some(handle),
        })
    }

    pub(crate) fn stop(&mut self) {
        self.cancel();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    pub(crate) fn cancel(&self) {
        self.active.store(false, Ordering::SeqCst);
        let _ = self.stop.send(());
    }
}

impl Drop for MemoryCardAgent {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run(config: MemoryCardConfig, stop_rx: Receiver<()>, active: Arc<AtomicBool>, sink: EventSink) {
    let journal = match MemoryCardJournal::open(&config.journal_path, config.scope.clone()) {
        Ok(journal) => journal,
        Err(error) => {
            sink.emit(AppEvent::log(
                Level::Error,
                "memory-cards",
                format!("memory-card sync could not start: {error}"),
            ));
            return;
        }
    };
    let interval =
        Duration::from_secs(u64::from(config.debounce_secs.max(1))).max(MIN_SCAN_INTERVAL);
    loop {
        if !active.load(Ordering::SeqCst) {
            break;
        }
        // A shared card is written for a whole session, so the DuckStation and
        // PCSX2 images must be closed. Exclusive handles are held for the pass;
        // when the guard cannot be established nothing syncs and the reason is
        // reported.
        let (emulator_running, _guard, unavailable) =
            match crate::controller::card_emulator_state(&config.scope.installation_root) {
                crate::controller::CardEmulatorState::Stopped(guards) => {
                    (false, Some(guards), None)
                }
                crate::controller::CardEmulatorState::Running => (true, None, None),
                crate::controller::CardEmulatorState::Unavailable(reason) => {
                    (true, None, Some(reason))
                }
            };
        let mut report = reconcile_once(&config, &journal, emulator_running, &active, &sink);
        if let Some(reason) = unavailable {
            report.problem.get_or_insert(reason);
        }
        if active.load(Ordering::SeqCst) {
            sink.emit(AppEvent::MemoryCardsUpdated {
                session_id: config.session_id,
                available: config.identity.can_read_saves() && config.identity.can_write_saves(),
                folders: report.folders,
                conflicts: report.conflicts,
                problem: report.problem,
            });
        }
        match stop_rx.recv_timeout(interval) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
    active.store(false, Ordering::SeqCst);
}

pub(crate) struct MemoryCardReport {
    pub folders: Vec<MemoryCardFolderStatus>,
    pub conflicts: Vec<MemoryCardConflictStatus>,
    pub problem: Option<String>,
}

/// One reconciliation pass. Every consented folder is processed even when it
/// holds no local card, so a card synced from another machine is restored;
/// each card is independent, so one failure is logged and the rest still run.
pub(crate) fn reconcile_once(
    config: &MemoryCardConfig,
    journal: &MemoryCardJournal,
    emulator_running: bool,
    active: &Arc<AtomicBool>,
    sink: &EventSink,
) -> MemoryCardReport {
    let root = config.scope.effective_saves_root.clone();
    let mut report = MemoryCardReport {
        folders: Vec::new(),
        conflicts: Vec::new(),
        problem: None,
    };
    if !active.load(Ordering::Acquire) {
        return report;
    }
    let discovered = match discover_card_folders(&root) {
        Ok(discovered) => discovered,
        Err(error) => {
            report.problem = Some(error.to_string());
            Vec::new()
        }
    };
    for folder in &discovered {
        let enabled = is_consented(config, folder);
        report.folders.push(folder_status(folder, enabled));
    }
    // A consented folder that discovery no longer sees still syncs (for example
    // a remote-only restore into a folder that does not exist yet).
    for folder in &config.folders {
        let seen = discovered.iter().any(|discovered| {
            discovered.emulator == folder.emulator && discovered.relative_dir == folder.relative_dir
        });
        if !seen {
            report.folders.push(MemoryCardFolderStatus {
                emulator: folder.emulator.name().to_owned(),
                label: folder.label.to_owned(),
                relative_dir: folder.relative_dir.to_string_lossy().replace('\\', "/"),
                cards: 0,
                enabled: true,
            });
        }
    }
    for folder in &config.folders {
        if let Err(error) = sync_folder(
            config,
            journal,
            folder,
            emulator_running,
            active,
            sink,
            &mut report,
        ) {
            sink.emit(AppEvent::log(
                Level::Warn,
                "memory-cards",
                format!("{}: {error}", folder.label),
            ));
            report.problem.get_or_insert_with(|| error.to_string());
        }
    }
    report
}

fn is_consented(config: &MemoryCardConfig, folder: &CardFolder) -> bool {
    config.folders.iter().any(|consented| {
        consented.emulator == folder.emulator && consented.relative_dir == folder.relative_dir
    })
}

fn folder_status(folder: &CardFolder, enabled: bool) -> MemoryCardFolderStatus {
    MemoryCardFolderStatus {
        emulator: folder.emulator.name().to_owned(),
        label: folder.label.to_owned(),
        relative_dir: folder.relative_dir.to_string_lossy().replace('\\', "/"),
        cards: folder.cards.len(),
        enabled,
    }
}

fn sync_folder(
    config: &MemoryCardConfig,
    journal: &MemoryCardJournal,
    folder: &CardFolder,
    emulator_running: bool,
    active: &Arc<AtomicBool>,
    sink: &EventSink,
    report: &mut MemoryCardReport,
) -> Result<()> {
    let root = &config.scope.effective_saves_root;
    let local = discover_card_folder(root, folder.emulator)?
        .map(|folder| folder.cards)
        .unwrap_or_default();
    let local_names: HashSet<String> = local.iter().map(|card| card.name.clone()).collect();
    // The list route returns the caller's own cards; public/shared cards are
    // never treated as ours, so only this account's cards are used.
    let remote_cards: Vec<MemoryCard> = config
        .client
        .memory_cards(folder.emulator.name())
        .map_err(|failure| failure.error)?
        .into_iter()
        .filter(|card| card.user_id == config.identity.account_id)
        .collect();

    for card in &local {
        match sync_local_card(
            config,
            journal,
            folder,
            card,
            &remote_cards,
            emulator_running,
            active,
        ) {
            Ok(Some(conflict)) => report.conflicts.push(conflict),
            Ok(None) => {}
            Err(error) => sink.emit(AppEvent::log(
                Level::Warn,
                "memory-cards",
                format!("{} {}: {error}", folder.label, card.name),
            )),
        }
    }

    for remote in &remote_cards {
        if local_names.contains(&remote.name) {
            continue;
        }
        match restore_remote_card(config, journal, folder, remote, emulator_running, active) {
            Ok(Some(conflict)) => report.conflicts.push(conflict),
            Ok(None) => {}
            Err(error) => sink.emit(AppEvent::log(
                Level::Warn,
                "memory-cards",
                format!("{} {}: {error}", folder.label, remote.name),
            )),
        }
    }
    Ok(())
}

fn sync_local_card(
    config: &MemoryCardConfig,
    journal: &MemoryCardJournal,
    folder: &CardFolder,
    card: &DiscoveredCard,
    remote_cards: &[MemoryCard],
    emulator_running: bool,
    active: &Arc<AtomicBool>,
) -> Result<Option<MemoryCardConflictStatus>> {
    let emulator = folder.emulator.name();
    let path = config.scope.effective_saves_root.join(&card.relative_path);
    let relative = portable(&card.relative_path);
    let local_hash = read_local_card_hash(&path)?;
    let baseline = journal.baseline(emulator, &card.name)?.unwrap_or_default();
    let (remote_card, remote_version) = resolve_remote(config, remote_cards, &card.name)?;
    let decision_remote = remote_version.as_ref().map(decision_version);
    let decision = plan_card_action(
        local_hash.as_deref(),
        &baseline,
        decision_remote.as_ref(),
        emulator_running,
    );
    match decision {
        CardDecision::EmulatorRunning | CardDecision::Settled => Ok(None),
        CardDecision::Upload => {
            let Some(local_hash) = local_hash else {
                return Ok(None);
            };
            if !quiet(&path, config.debounce_secs)? {
                return Ok(None);
            }
            let card_id = match remote_card {
                Some(card) => card.id,
                None => {
                    config
                        .client
                        .create_memory_card(&card.name, emulator)
                        .map_err(|failure| failure.error)?
                        .id
                }
            };
            let mut archive = temp_archive(config)?;
            write_card_archive(&path, archive.as_file_mut())?;
            // Windows temp files are created with share_mode(0): the upload
            // re-opens by path, so the writer handle must be closed first.
            let archive = archive.into_temp_path();
            let version = config
                .client
                .upload_memory_card_version(
                    card_id,
                    &format!("{}.card.zip", card.name),
                    &archive,
                    active,
                )
                .map_err(|failure| failure.error)?;
            journal.record(
                emulator,
                &card.name,
                &relative,
                &CardBaseline {
                    local_hash: Some(local_hash),
                    version_id: Some(version.id),
                    remote_hash: version.content_hash,
                },
            )?;
            Ok(None)
        }
        CardDecision::Download { version_id } => {
            let Some(version) = remote_version.as_ref() else {
                return Ok(None);
            };
            let archive = download_archive(config, version, active)?;
            if !active.load(Ordering::Acquire) {
                // Cancelled: the local card is left exactly as it was.
                return Ok(None);
            }
            // Never overwrite a card the user changed while the download ran.
            // An unreadable local card is an error, not "changed to nothing".
            if read_local_card_hash(&path)? != baseline.local_hash {
                return preserve_conflict(
                    &config.scope.effective_saves_root,
                    journal,
                    folder,
                    &path,
                    card.folder,
                    version,
                    &archive,
                );
            }
            extract_card_archive(&archive, &path, card.folder)?;
            let new_hash = hash_local_card(&path)?;
            journal.record(
                emulator,
                &card.name,
                &relative,
                &CardBaseline {
                    local_hash: Some(new_hash),
                    version_id: Some(version_id),
                    remote_hash: version.content_hash.clone(),
                },
            )?;
            Ok(None)
        }
        CardDecision::Conflict { version_id } => {
            let sibling = conflict_sibling(&path, card.folder, version_id)?;
            if !sibling.exists() {
                let Some(version) = remote_version.as_ref() else {
                    return Ok(None);
                };
                let archive = download_archive(config, version, active)?;
                if !active.load(Ordering::Acquire) {
                    return Ok(None);
                }
                extract_card_archive(&archive, &sibling, card.folder)?;
            }
            journal.record_failure(emulator, &card.name, &relative, CONFLICT_REASON)?;
            Ok(Some(MemoryCardConflictStatus {
                emulator: emulator.to_owned(),
                card_name: card.name.clone(),
                relative_path: relative,
                remote_version_id: version_id,
            }))
        }
    }
}

/// Restore a card that exists on RomM but not on this machine. The archive
/// layout is inferred from the downloaded zip; a single top-level file restores
/// as `<card>.<ext>`, anything else as a folder named after the card.
fn restore_remote_card(
    config: &MemoryCardConfig,
    journal: &MemoryCardJournal,
    folder: &CardFolder,
    remote: &MemoryCard,
    emulator_running: bool,
    active: &Arc<AtomicBool>,
) -> Result<Option<MemoryCardConflictStatus>> {
    let emulator = folder.emulator.name();
    let baseline = journal
        .baseline(emulator, &remote.name)?
        .unwrap_or_default();
    let Some(version) = newest_version(config, remote)? else {
        return Ok(None);
    };
    let decision = plan_card_action(
        None,
        &baseline,
        Some(&decision_version(&version)),
        emulator_running,
    );
    if !matches!(decision, CardDecision::Download { .. }) {
        return Ok(None);
    }
    let archive = download_archive(config, &version, active)?;
    if !active.load(Ordering::Acquire) {
        return Ok(None);
    }
    let restore = remote_card_restore_target(&archive, folder.emulator, &remote.name)?;
    let path = config
        .scope
        .effective_saves_root
        .join(&restore.relative_path);
    let relative = portable(&restore.relative_path);
    // A card that exists locally while downloading — even one this process
    // cannot read, or a reparse point — must never be overwritten. Existence is
    // checked with `symlink_metadata`, not by hashing.
    if std::fs::symlink_metadata(&path).is_ok() {
        return preserve_conflict(
            &config.scope.effective_saves_root,
            journal,
            folder,
            &path,
            restore.folder,
            &version,
            &archive,
        );
    }
    extract_card_archive(&archive, &path, restore.folder)?;
    let new_hash = hash_local_card(&path)?;
    journal.record(
        emulator,
        &remote.name,
        &relative,
        &CardBaseline {
            local_hash: Some(new_hash),
            version_id: Some(version.id),
            remote_hash: version.content_hash.clone(),
        },
    )?;
    Ok(None)
}

const CONFLICT_REASON: &str =
    "local changed while a remote version was being applied; the remote copy is kept as a .rommfs-conflict sibling";

/// Keep the downloaded remote version beside the local card, version-specific,
/// and report the card for review. The local card is never touched.
fn preserve_conflict(
    saves_root: &Path,
    journal: &MemoryCardJournal,
    folder: &CardFolder,
    path: &Path,
    folder_card: bool,
    version: &MemoryCardVersion,
    archive: &tempfile::TempPath,
) -> Result<Option<MemoryCardConflictStatus>> {
    // A folder card's name is its whole folder name; a file card drops only its
    // final extension. `file_stem` would truncate a dotted folder name.
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let card_name = if folder_card {
        file_name
    } else {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(file_name)
    };
    let relative = portable(path.strip_prefix(saves_root).unwrap_or(path));
    let sibling = conflict_sibling(path, folder_card, version.id)?;
    if !sibling.exists() {
        extract_card_archive(archive, &sibling, folder_card)?;
    }
    journal.record_failure(
        folder.emulator.name(),
        card_name,
        &relative,
        CONFLICT_REASON,
    )?;
    Ok(Some(MemoryCardConflictStatus {
        emulator: folder.emulator.name().to_owned(),
        card_name: card_name.to_owned(),
        relative_path: relative,
        remote_version_id: version.id,
    }))
}

fn resolve_remote(
    config: &MemoryCardConfig,
    remote_cards: &[MemoryCard],
    card_name: &str,
) -> Result<(Option<MemoryCard>, Option<MemoryCardVersion>)> {
    let Some(card) = remote_cards.iter().find(|card| card.name == card_name) else {
        return Ok((None, None));
    };
    Ok((Some(card.clone()), newest_version(config, card)?))
}

fn newest_version(
    config: &MemoryCardConfig,
    card: &MemoryCard,
) -> Result<Option<MemoryCardVersion>> {
    Ok(config
        .client
        .memory_card_versions(card.id)
        .map_err(|failure| failure.error)?
        .into_iter()
        .next())
}

fn decision_version(version: &MemoryCardVersion) -> RemoteCardVersion {
    RemoteCardVersion {
        id: version.id,
        content_hash: version.content_hash.clone(),
    }
}

fn download_archive(
    config: &MemoryCardConfig,
    version: &MemoryCardVersion,
    active: &Arc<AtomicBool>,
) -> Result<tempfile::TempPath> {
    let mut archive = temp_archive(config)?;
    {
        let mut file = archive.as_file_mut();
        config
            .client
            .download_memory_card_version(
                version.id,
                version.file_size_bytes,
                &mut file,
                active.as_ref(),
            )
            .map_err(|failure| failure.error)?;
    }
    // Close the handle: Windows temp files carry share_mode(0), and every
    // consumer below re-opens the archive by path.
    Ok(archive.into_temp_path())
}

fn temp_archive(config: &MemoryCardConfig) -> Result<tempfile::NamedTempFile> {
    let directory = config
        .journal_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir);
    tempfile::Builder::new()
        .prefix("rommfs-card-")
        .suffix(".zip")
        .tempfile_in(directory)
        .map_err(Into::into)
}

/// Whether a changed card has settled for the debounce period. A read error, a
/// missing modification time, or a future timestamp all mean "not settled", so
/// a card is never uploaded on an unknown age.
fn quiet(path: &Path, debounce_secs: u32) -> Result<bool> {
    let Some(modified) = latest_card_modified(path)? else {
        return Ok(false);
    };
    match modified.elapsed() {
        Ok(age) => Ok(age >= Duration::from_secs(u64::from(debounce_secs))),
        Err(_) => Ok(false),
    }
}

fn portable(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
