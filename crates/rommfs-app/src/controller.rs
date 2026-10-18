//! Headless app controller: the real state machine the GPUI window renders.
//! Drives workers via commands, consumes `AppEvent`s into `UiState`.
//! Tests exercise this path — no duplicate state machine in the UI (PRD §6).

use crate::save_sync_agent::{
    MemoryCardConfig, SaveSyncAgent, SaveSyncCommandGate, SaveSyncEnablement,
};
use anyhow::Context as _;
use rommfs_core::cache::clock::{Clock, SystemClock, DEFAULT_EVICTION_THRESHOLD_SECS};
#[cfg(any(not(windows), test))]
use rommfs_core::cache::NoopHydratedRemover;
use rommfs_core::cache::{CacheIndex, Evictor, HydratedRemover, LiveState};
#[cfg(test)]
use rommfs_core::catalogue::build_catalogue;
use rommfs_core::catalogue::{server_id_of, Catalogue, RomKey};
use rommfs_core::catalogue_store::{CatalogueSnapshot, CatalogueStore};
use rommfs_core::connection_settings::{
    ConnectionPreferences, ConnectionSettings, IntegrationMode,
};
use rommfs_core::download::{ContentSource, DownloadManager};
use rommfs_core::error::{Error, Result};
use rommfs_core::events::{
    AppEvent, EventSink, ForeignEntry, ForeignKind, Level, LogBuffer, MemoryCardConflictStatus,
    MemoryCardFolderStatus, SaveSyncGameStatus, SaveSyncQueueStatus,
};
use rommfs_core::filesystem::RommFs;
use rommfs_core::romm::{
    iso8601_utc, Credentials, PairedSession, PairingPoll, PairingRequest, RomDto, RommClient,
    RomsPage,
};
use rommfs_core::save_sync::{
    discover_card_folders, discover_installations, map_catalogue, resolve_retrobat_gb_profile,
    validate_installation, ConsentSettings, DiscoveryInput, InstallationCandidate,
    InstallationProblem, MappingReport, RetroBatGbProfile, SaveSyncJournal, SaveSyncScope,
    SaveSyncSettingsStore, SnapshotState, UnmatchedFile,
};
use rommfs_core::tree::RommTree;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub use commands::Command;
use commands::{invalidates_save_sync_scope, CommandEnvelope};
use retrobat_setup::MetadataPublication;
pub use state::*;

mod auxiliary;
mod catalogue;
mod commands;
mod connection;
mod installer;
mod memory_cards;
mod mount;
mod play_data;
mod retrobat_setup;
mod save_files;
mod save_sync_setup;
mod state;

const EVICTION_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Page size for the catalogue walk. RomM caps the limit at 10 000; the
/// returned `limit` is respected if the server clamps it lower.
const CATALOGUE_PAGE_LIMIT: u64 = 500;
/// How many catalogue pages are fetched at once after the first page.
const CATALOGUE_PAGE_CONCURRENCY: usize = 4;
/// A snapshot whose last full refresh is older than this gets a full refresh
/// again, because the server's `updated_at` signal may not cover a rescan that
/// only added, removed or renamed files.
const FULL_REFRESH_MAX_AGE_SECS: u64 = 24 * 60 * 60;
/// Overlap subtracted from the last refresh start when asking for changed
/// ROMs, so a ROM written while the previous fetch ran is not missed.
const INCREMENTAL_SKEW_SECS: u64 = 5 * 60;
/// How often the connected session polls for catalogue changes. This is the
/// real-time fallback: the RomM socket.io connect handler resolves identity
/// from the web session cookie only, so a bearer-only session cannot subscribe.
const CATALOGUE_POLL_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Owns the worker session (client + catalogue + mount + evict timer).
/// Created on Connect; events flow back over the `EventSink` channel.
pub struct Controller {
    sink: EventSink,
    cmd_tx: mpsc::Sender<CommandEnvelope>,
    // worker thread handle held for shutdown joining
    worker: std::sync::Mutex<Option<JoinHandle<()>>>,
    save_sync_gate: SaveSyncCommandGate,
    connection_gate: SaveSyncCommandGate,
}

impl Controller {
    /// Spawn the controller + worker loop. Returns (controller, event rx).
    pub fn spawn() -> (Self, mpsc::Receiver<AppEvent>) {
        let (sink, event_rx) = rommfs_core::events::channel();
        let sink = match diagnostic_log_directory() {
            Some(directory) => match sink.clone().with_log_directory(&directory) {
                Ok(durable) => durable,
                Err(error) => {
                    sink.emit(AppEvent::log(
                        Level::Error,
                        "diagnostics",
                        format!("Diagnostic log unavailable: {error}"),
                    ));
                    sink
                }
            },
            None => sink,
        };
        crate::trace_bridge::TraceBridge::install(sink.clone());
        let (cmd_tx, cmd_rx) = mpsc::channel::<CommandEnvelope>();
        let worker_sink = sink.clone();
        let save_sync_gate = SaveSyncCommandGate::new();
        let worker_gate = save_sync_gate.clone();
        let connection_gate = SaveSyncCommandGate::new();
        let worker_connection_gate = connection_gate.clone();
        let worker = std::thread::Builder::new()
            .name("rommfs-worker".to_string())
            .spawn(move || worker_loop(cmd_rx, worker_sink, worker_gate, worker_connection_gate))
            .expect("spawn worker thread");
        let initial_epoch = save_sync_gate.invalidate();
        let _ = cmd_tx.send(CommandEnvelope {
            command: Command::RestoreConnection,
            save_sync_epoch: initial_epoch,
            connection_epoch: connection_gate.current_epoch(),
        });
        (
            Self {
                sink,
                cmd_tx,
                worker: std::sync::Mutex::new(Some(worker)),
                save_sync_gate,
                connection_gate,
            },
            event_rx,
        )
    }

    pub fn send(&self, cmd: Command) {
        let save_sync_epoch = if invalidates_save_sync_scope(&cmd) {
            self.save_sync_gate.invalidate()
        } else {
            self.save_sync_gate.current_epoch()
        };
        let connection_epoch = if matches!(
            cmd,
            Command::BeginPairing { .. }
                | Command::CancelPairing
                | Command::Logout
                | Command::RestoreConnection
                | Command::Connect { .. }
                | Command::Shutdown
        ) {
            self.connection_gate.invalidate()
        } else {
            self.connection_gate.current_epoch()
        };
        let _ = self.cmd_tx.send(CommandEnvelope {
            command: cmd,
            save_sync_epoch,
            connection_epoch,
        });
    }

    pub fn sink(&self) -> EventSink {
        self.sink.clone()
    }
    /// Request cleanup once and transfer the join to the caller. Desktop UI
    /// callers join off-thread so the closing window remains responsive.
    pub fn begin_shutdown(&self) -> Option<JoinHandle<()>> {
        let worker = self
            .worker
            .lock()
            .expect("controller worker mutex poisoned")
            .take();
        if worker.is_some() {
            self.sink.emit(AppEvent::ShutdownStarting);
            self.send(Command::Shutdown);
        }
        worker
    }

    pub fn shutdown(&self) {
        if let Some(worker) = self.begin_shutdown() {
            let _ = worker.join();
        }
    }
}

impl Drop for Controller {
    /// The application session, not a visible window, owns shutdown.
    fn drop(&mut self) {
        self.shutdown();
    }
}

// ---------------------------------------------------------------------------
// Worker side (all network/filesystem work off the UI thread, PRD R5)
// ---------------------------------------------------------------------------

/// What the worker holds about the active server session. One server, one
/// mount — rebuilt on Connect/StartMount, never shared across servers.
struct PendingPairing {
    client: Arc<RommClient>,
    url: String,
    identifier: String,
    request: PairingRequest,
    expires: Instant,
    next_poll: Instant,
    interval: Duration,
    epoch: u64,
}

struct Worker {
    auxiliary_job: Option<auxiliary::AuxiliaryJob>,
    startup_restoring: bool,
    startup_requires_mount: bool,
    save_file_rules: Option<rommfs_core::save_sync::SaveFileRules>,
    installer: installer::InstallerJob,
    metadata_job: Option<mpsc::Receiver<Result<rommfs_core::retrobat::ArtworkBatch>>>,
    metadata_handle: Option<JoinHandle<()>>,
    /// Monotonic identity of the current artwork pass; progress from an earlier
    /// pass carries a smaller id and is dropped by the UI.
    metadata_job_id: u64,
    metadata_active: Option<Arc<std::sync::atomic::AtomicBool>>,
    metadata_source: Option<(Catalogue, CatalogueSnapshot)>,
    metadata_ready: Option<rommfs_core::retrobat::ArtworkBatch>,
    metadata_wait_reported: bool,
    /// Gamelist generation and publication run off the worker loop; this is
    /// the in-flight job's result channel.
    metadata_publish: Option<mpsc::Receiver<MetadataPublication>>,
    metadata_publish_handle: Option<JoinHandle<()>>,
    /// Cancelled worker handles. Joining them on the spot would stall the
    /// loop behind in-flight HTTP timeouts; the loop reaps them instead.
    draining: Vec<JoinHandle<()>>,
    #[cfg(test)]
    rom_cache_directory: Option<PathBuf>,
    sink: EventSink,
    client: Option<Arc<RommClient>>,
    server_id: Option<String>,
    connection_settings: Option<ConnectionSettings>,
    preferences: Option<ConnectionPreferences>,
    pairing: Option<PendingPairing>,
    catalogue_store: Option<CatalogueStore>,
    catalogue_account: Option<String>,
    source_snapshot: Option<CatalogueSnapshot>,
    refresh_result: Option<mpsc::Receiver<(std::time::Instant, Result<CatalogueSnapshot>)>>,
    refresh_handle: Option<JoinHandle<()>>,
    refresh_active: Option<Arc<std::sync::atomic::AtomicBool>>,
    catalogue_operation_id: u64,
    /// Latest complete projection, retained for warm remounts.
    catalogue: Option<Catalogue>,
    /// ROM file names by key (content URL + log/evict context).
    names: HashMap<RomKey, String>,
    fs: Option<Arc<RommFs>>,
    mount: Option<ActiveMount>,
    save_sync_settings: Option<SaveSyncSettingsStore>,
    save_sync_storage_dir: Option<PathBuf>,
    save_sync_selected_root: Option<PathBuf>,
    save_sync_settings_problem: Option<String>,
    save_sync_enabled: bool,
    save_sync_debounce_secs: u32,
    save_sync_identity: Option<rommfs_core::romm::SaveSyncIdentity>,
    save_sync_scope: Option<SaveSyncScope>,
    save_sync_profile: Option<RetroBatGbProfile>,
    save_sync_mapping_report: Option<MappingReport>,
    save_sync_catalogue: Option<Catalogue>,
    save_sync_agent: Option<SaveSyncAgent>,
    save_sync_gate: SaveSyncCommandGate,
    connection_gate: SaveSyncCommandGate,
    current_connection_epoch: u64,
    current_save_sync_epoch: u64,
    save_sync_runtime_problem: Option<String>,
    save_sync_session_id: u64,
    play_data_enabled: bool,
    play_data_available: bool,
    play_data_problem: Option<String>,
    /// How many entries the last metadata pass hid ("hide RomM copies").
    hide_copies_count: usize,
    /// Linked gamelist modification times seen at the last check. `None` means
    /// the watch is not running.
    play_watch: Option<HashMap<String, Option<std::time::SystemTime>>>,
    play_watch_checked: Instant,
    /// Deadline for the next incremental catalogue poll — the real-time
    /// fallback while no socket is used. `None` while disconnected.
    next_catalogue_poll: Option<Instant>,
}

impl Worker {
    #[cfg(test)]
    fn new(sink: EventSink) -> Self {
        Self::new_with_gate(sink, SaveSyncCommandGate::new())
    }

    fn new_with_gate(sink: EventSink, save_sync_gate: SaveSyncCommandGate) -> Self {
        let current_save_sync_epoch = save_sync_gate.current_epoch();
        Self {
            startup_restoring: false,
            startup_requires_mount: false,
            auxiliary_job: None,
            save_file_rules: None,
            installer: installer::InstallerJob::default(),
            metadata_job: None,
            metadata_handle: None,
            metadata_job_id: 0,
            metadata_active: None,
            metadata_source: None,
            metadata_ready: None,
            metadata_wait_reported: false,
            metadata_publish: None,
            metadata_publish_handle: None,
            draining: Vec::new(),
            #[cfg(test)]
            rom_cache_directory: None,
            sink,
            client: None,
            server_id: None,
            catalogue: None,
            connection_settings: None,
            preferences: None,
            pairing: None,
            catalogue_store: None,
            catalogue_account: None,
            source_snapshot: None,
            refresh_result: None,
            refresh_handle: None,
            refresh_active: None,
            catalogue_operation_id: 0,
            names: HashMap::new(),
            fs: None,
            mount: None,
            save_sync_settings: None,
            save_sync_storage_dir: None,
            save_sync_selected_root: None,
            save_sync_settings_problem: None,
            save_sync_enabled: false,
            save_sync_debounce_secs: rommfs_core::save_sync::DEFAULT_DEBOUNCE_SECS,
            save_sync_identity: None,
            save_sync_scope: None,
            save_sync_profile: None,
            save_sync_mapping_report: None,
            save_sync_catalogue: None,
            save_sync_agent: None,
            save_sync_gate,
            connection_gate: SaveSyncCommandGate::new(),
            current_connection_epoch: 0,
            current_save_sync_epoch,
            save_sync_runtime_problem: None,
            save_sync_session_id: 0,
            play_data_enabled: false,
            play_data_available: false,
            play_data_problem: None,
            hide_copies_count: 0,
            play_watch: None,
            play_watch_checked: Instant::now(),
            next_catalogue_poll: None,
        }
    }

    fn with_persistent_save_sync_settings(
        sink: EventSink,
        save_sync_gate: SaveSyncCommandGate,
    ) -> Self {
        let settings_path = save_sync_settings_path();
        let (save_sync_settings, save_sync_settings_problem) = match settings_path.as_ref() {
            Some(path) => match SaveSyncSettingsStore::open(path) {
                Ok(settings) => (Some(settings), None),
                Err(error) => (None, Some(format!("Settings could not be opened: {error}"))),
            },
            None => (
                None,
                Some("No application settings directory is available.".into()),
            ),
        };
        let save_sync_selected_root = save_sync_settings
            .as_ref()
            .and_then(|settings| settings.selected_installation().ok().flatten());
        let mut worker = Self::new_with_gate(sink, save_sync_gate);
        if let Some(parent) = settings_path.as_ref().and_then(|p| p.parent()) {
            match ConnectionSettings::open(&parent.join("connection.db")) {
                Ok(store) => worker.connection_settings = Some(store),
                Err(error) => worker.log(Level::Warn, "settings", error.to_string()),
            }
            match CatalogueStore::open(&parent.join("catalogue.db")) {
                Ok(store) => worker.catalogue_store = Some(store),
                Err(error) => worker.log(
                    Level::Warn,
                    "catalogue",
                    format!("Catalogue persistence unavailable: {error}"),
                ),
            }
        }
        worker.save_sync_settings = save_sync_settings;
        worker.save_sync_storage_dir =
            settings_path.and_then(|path| path.parent().map(Path::to_path_buf));
        worker.save_sync_selected_root = save_sync_selected_root;
        worker.save_sync_settings_problem = save_sync_settings_problem;
        worker
    }

    fn log(&self, level: Level, op: &'static str, message: impl Into<String>) {
        self.sink.emit(AppEvent::log(level, op, message));
    }

    /// Drop finished cancelled workers without blocking on the ones still
    /// draining in-flight work.
    fn reap_draining(&mut self) {
        self.draining.retain(|handle| !handle.is_finished());
    }

    /// Join every cancelled worker. Used at connection change and shutdown,
    /// where the owned threads must be fully drained before returning.
    fn drain_handles(&mut self) {
        for handle in self.draining.drain(..) {
            let _ = handle.join();
        }
    }
}

fn worker_loop(
    rx: mpsc::Receiver<CommandEnvelope>,
    sink: EventSink,
    save_sync_gate: SaveSyncCommandGate,
    connection_gate: SaveSyncCommandGate,
) {
    let mut worker = Worker::with_persistent_save_sync_settings(sink, save_sync_gate);
    worker.connection_gate = connection_gate;
    worker_loop_with_interval_from(rx, worker, EVICTION_INTERVAL);
}

fn worker_loop_with_interval_from(
    rx: mpsc::Receiver<CommandEnvelope>,
    mut worker: Worker,
    eviction_interval: Duration,
) {
    let mut next_eviction = None;
    loop {
        worker.reap_draining();
        worker.poll_pairing();
        worker.poll_catalogue_refresh();
        worker.poll_catalogue_refresh_timer();
        worker.poll_play_watch();
        worker.poll_metadata();
        if worker.installer.poll(&worker.sink) {
            worker.preselect_installed_retrobat();
        }
        worker.poll_auxiliary();
        let received = if worker.refresh_result.is_some()
            || worker.auxiliary_job.is_some()
            || worker.installer.is_running()
            || worker.pairing.is_some()
            || worker.metadata_job.is_some()
            || worker.metadata_ready.is_some()
            || worker.metadata_publish.is_some()
        {
            rx.recv_timeout(Duration::from_millis(100))
        } else if worker.mount.is_some() {
            let deadline = *next_eviction.get_or_insert_with(|| Instant::now() + eviction_interval);
            if Instant::now() >= deadline {
                worker.evict_once();
                next_eviction = Some(Instant::now() + eviction_interval);
                continue;
            }
            // Wake at least every play-watch interval so a gamelist change is
            // noticed without an unrelated event.
            rx.recv_timeout(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(play_data::PLAY_WATCH_INTERVAL),
            )
        } else if let Some(deadline) = worker.next_catalogue_poll {
            // Connected but idle: wake for the next incremental catalogue poll.
            next_eviction = None;
            rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        } else {
            next_eviction = None;
            rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
        };
        let envelope = match received {
            Ok(envelope) => envelope,
            Err(mpsc::RecvTimeoutError::Timeout)
                if worker.refresh_result.is_some()
                    || worker.auxiliary_job.is_some()
                    || worker.installer.is_running()
                    || worker.pairing.is_some()
                    || worker.metadata_job.is_some()
                    || worker.metadata_ready.is_some()
                    || worker.metadata_publish.is_some() =>
            {
                continue
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // A short wake only polls the play-data watch; eviction is
                // still driven by its own deadline.
                if Instant::now() >= next_eviction.unwrap_or_else(Instant::now) {
                    worker.evict_once();
                    next_eviction = Some(Instant::now() + eviction_interval);
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let CommandEnvelope {
            command: cmd,
            save_sync_epoch,
            connection_epoch,
        } = envelope;
        worker.current_save_sync_epoch = save_sync_epoch;
        worker.current_connection_epoch = connection_epoch;
        let shutdown = matches!(cmd, Command::Shutdown);
        // Commands run one at a time on the worker thread; UI stays
        // responsive while network/filesystem work blocks here.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match &cmd {
            Command::CancelAuxiliary => worker.cancel_auxiliary(),
            Command::MaterializeAuxiliary => worker.materialize_auxiliary(),
            Command::SetStartAtLogin { enabled } => worker.set_start_at_login(*enabled),
            Command::SelectSaveFolder { path } => worker.select_save_folder(path),
            Command::AssociateSaveFile {
                relative_path,
                rom_id,
            } => worker.associate_save_file(relative_path, *rom_id),
            Command::AssociateSaveFolder { prefix, rom_id } => {
                worker.associate_save_folder(prefix, *rom_id)
            }
            Command::DownloadRetroBatInstaller => {
                let directory = worker
                    .save_sync_storage_dir
                    .as_ref()
                    .map(|p| p.join("installers"));
                worker.installer.download(directory, &worker.sink);
            }
            Command::CancelRetroBatInstaller => worker.installer.cancel(&worker.sink),
            Command::LaunchRetroBatInstaller => worker.installer.launch(&worker.sink),
            Command::ShowRetroBatInstaller => worker.installer.show_file(&worker.sink),
            Command::SetupRetroBat {
                installation,
                managed_root,
            } => worker.setup_retrobat(installation, managed_root),
            Command::RemoveForeignLinks { root } => worker.remove_foreign_links(root),
            Command::SetIntegration {
                mode,
                retrobat_root,
            } => worker.set_integration(*mode, retrobat_root.clone()),
            Command::RestoreConnection => worker.restore_connection(),
            Command::BeginPairing { url } => worker.begin_pairing(url),
            Command::CancelPairing => worker.cancel_pairing(),
            Command::Logout => worker.logout(),
            Command::Connect {
                url,
                username,
                password,
            } => worker.connect(url, username, password),
            Command::SetMountRoot { path } => worker.set_mount_root(path),
            Command::StartMount { path } => worker.start_mount(path),
            Command::StopMount => worker.stop_mount(),
            Command::RestartMount { path } => worker.restart_mount(path),
            Command::RefreshCatalogue => worker.refresh_catalogue(),
            Command::RefreshMetadata => worker.queue_metadata(),
            Command::RefreshSaveSync => worker.check_save_sync(),
            Command::SelectSaveSyncInstallation { path } => {
                worker.select_save_sync_installation(path)
            }
            Command::SetSaveSyncEnabled { enabled } => worker.set_save_sync_enabled(*enabled),
            Command::SetSaveSyncDebounce { seconds } => worker.set_save_sync_debounce(*seconds),
            Command::SetPlayDataSync { enabled } => worker.set_play_data_enabled(*enabled),
            Command::SetHideOwnedCopies { enabled } => worker.set_hide_owned_copies(*enabled),
            Command::ExportSaveSyncIncoming {
                session_id,
                incoming_id,
                destination,
            } => worker.export_save_sync_incoming(*session_id, incoming_id, destination),
            Command::SetMemoryCardFolderEnabled {
                emulator,
                relative_dir,
                enabled,
            } => worker.set_memory_card_folder_enabled(emulator, relative_dir, *enabled),
            Command::Shutdown => {
                worker.cancel_auxiliary();
                worker.installer.cancel(&worker.sink);
                worker.cancel_pairing();
                worker.cancel_catalogue_refresh();
                worker.stop_save_sync_agent();
                worker.stop_mount();
            }
        }));
        if let Err(payload) = result {
            // Mid-flight a sibling crate's todo!() (or any panic) must
            // surface as a visible failure, not a dead worker (PRD: errors
            // become real AppEvents). The message is a code location, never
            // a credential.
            let msg = panic_message(&payload);
            worker.log(Level::Error, "worker", format!("operation panicked: {msg}"));
            match &cmd {
                Command::RestoreConnection
                | Command::BeginPairing { .. }
                | Command::CancelPairing
                | Command::Logout
                | Command::Connect { .. } => worker.sink.emit(AppEvent::ConnectFailed {
                    reason: format!("internal error: {msg}"),
                }),
                Command::StartMount { .. } | Command::RestartMount { .. } => {
                    worker.sink.emit(AppEvent::MountFailed {
                        reason: format!("internal error: {msg}"),
                    })
                }
                Command::StopMount => {}
                Command::SetMountRoot { .. } => {}
                Command::SetupRetroBat { .. } => worker.sink.emit(AppEvent::RetroBatSetup {
                    running: false,
                    detail: "RetroBat setup stopped".into(),
                    skipped: Vec::new(),
                    error: Some(format!("internal error: {msg}")),
                }),
                Command::RemoveForeignLinks { .. } => {}
                Command::SetIntegration { .. }
                | Command::RefreshCatalogue
                | Command::RefreshMetadata
                | Command::RefreshSaveSync
                | Command::SelectSaveSyncInstallation { .. }
                | Command::SetSaveSyncEnabled { .. }
                | Command::SetSaveSyncDebounce { .. }
                | Command::SetPlayDataSync { .. }
                | Command::SetHideOwnedCopies { .. }
                | Command::ExportSaveSyncIncoming { .. }
                | Command::SetMemoryCardFolderEnabled { .. } => {}
                Command::SelectSaveFolder { .. }
                | Command::AssociateSaveFile { .. }
                | Command::AssociateSaveFolder { .. } => {}
                Command::Shutdown => {}
                Command::MaterializeAuxiliary => {}
                Command::CancelAuxiliary => {}
                Command::SetStartAtLogin { .. } => {}
                Command::DownloadRetroBatInstaller
                | Command::CancelRetroBatInstaller
                | Command::LaunchRetroBatInstaller
                | Command::ShowRetroBatInstaller => {}
            }
        }
        if shutdown {
            break;
        }
    }
    worker.cancel_auxiliary();
    worker.cancel_catalogue_refresh();
    worker.cancel_metadata();
    if worker.save_sync_agent.is_some() {
        worker.stop_save_sync_agent();
    }
    if worker.mount.is_some() {
        worker.stop_mount();
    }
    if worker.installer.is_running() {
        worker.installer.cancel(&worker.sink);
    }
    worker.drain_handles();
}

fn fetch_catalogue(
    client: &RommClient,
    sink: &EventSink,
    operation_id: u64,
    prior: Option<CatalogueSnapshot>,
) -> Result<CatalogueSnapshot> {
    let started = Instant::now();
    let started_unix = rommfs_core::cache::now_unix_secs();
    if !sink.is_active() {
        return Err(Error::Cancelled);
    }
    // Incremental when a snapshot for this server/account exists and its last
    // full refresh is recent. A server that rejects `updated_after` or
    // `/identifiers`, or a delta that cannot be reconciled, falls back to the
    // full walk below; an auth or permission failure is not hidden by one.
    if let Some(prior) = prior.filter(|prior| {
        started_unix.saturating_sub(prior.full_refreshed_at) < FULL_REFRESH_MAX_AGE_SECS
    }) {
        match fetch_catalogue_incremental(client, sink, operation_id, &prior, started, started_unix)
        {
            Ok(snapshot) => return Ok(snapshot),
            Err(error) if incremental_can_fall_back(&error) => {
                sink.emit(AppEvent::log(
                    Level::Info,
                    "catalogue",
                    format!(
                        "operation={operation_id} incremental refresh unavailable, running a full refresh: {error}"
                    ),
                ));
            }
            Err(error) => return Err(error),
        }
    }
    fetch_catalogue_full(client, sink, operation_id, started, started_unix)
}

/// An incremental failure falls back to a full refresh unless it is a
/// cancellation or a session/permission problem the full walk would hit too.
fn incremental_can_fall_back(error: &Error) -> bool {
    !error.needs_sign_in() && !matches!(error, Error::Cancelled | Error::Forbidden(_))
}

fn fetch_catalogue_full(
    client: &RommClient,
    sink: &EventSink,
    operation_id: u64,
    started: Instant,
    started_unix: u64,
) -> Result<CatalogueSnapshot> {
    let request_started = Instant::now();
    let platforms = client.platforms();
    sink.emit(AppEvent::log(
        Level::Info,
        "http",
        format!(
            "operation={operation_id} GET /api/platforms elapsed_ms={} result={}",
            request_started.elapsed().as_millis(),
            http_result(&platforms)
        ),
    ));
    let platforms = platforms?;
    let roms = fetch_rom_pages(client, sink, operation_id, None, started)?;
    sink.emit(AppEvent::log(
        Level::Info,
        "catalogue",
        format!(
            "operation={operation_id} fetched {} platforms and {} ROMs in {}ms kind=full",
            platforms.len(),
            roms.len(),
            started.elapsed().as_millis()
        ),
    ));
    Ok(CatalogueSnapshot {
        platforms,
        roms,
        refreshed_at: started_unix,
        full_refreshed_at: started_unix,
    })
}

/// Merge `updated_after=<last refresh start − skew>` against the snapshot and
/// the authoritative identifier set: replace changed ROMs, add new ones and
/// drop deleted ones. An id that is visible but neither in the snapshot nor in
/// the delta cannot be reconciled here, so the caller runs a full refresh.
fn fetch_catalogue_incremental(
    client: &RommClient,
    sink: &EventSink,
    operation_id: u64,
    prior: &CatalogueSnapshot,
    started: Instant,
    started_unix: u64,
) -> Result<CatalogueSnapshot> {
    let request_started = Instant::now();
    let platforms = client.platforms();
    sink.emit(AppEvent::log(
        Level::Info,
        "http",
        format!(
            "operation={operation_id} GET /api/platforms elapsed_ms={} result={}",
            request_started.elapsed().as_millis(),
            http_result(&platforms)
        ),
    ));
    let platforms = platforms?;
    if !sink.is_active() {
        return Err(Error::Cancelled);
    }
    let request_started = Instant::now();
    let identifiers = client.rom_identifiers();
    sink.emit(AppEvent::log(
        Level::Info,
        "http",
        format!(
            "operation={operation_id} GET /api/roms/identifiers elapsed_ms={} result={}",
            request_started.elapsed().as_millis(),
            http_result(&identifiers)
        ),
    ));
    let identifier_set: HashSet<i64> = identifiers?.into_iter().collect();
    // Cover the whole window since the previous refresh *start*, with a skew
    // so a ROM written while that refresh ran is not missed.
    let updated_after = iso8601_utc(prior.refreshed_at.saturating_sub(INCREMENTAL_SKEW_SECS));
    let delta = fetch_rom_pages(client, sink, operation_id, Some(&updated_after), started)?;

    let prior_ids: HashSet<i64> = prior.roms.iter().map(|rom| rom.id).collect();
    let delta_map: HashMap<i64, RomDto> = delta
        .into_iter()
        .filter(|rom| identifier_set.contains(&rom.id))
        .map(|rom| (rom.id, rom))
        .collect();
    // Keep the snapshot's order, applying each changed ROM in place.
    let mut roms: Vec<RomDto> = Vec::with_capacity(identifier_set.len());
    for rom in &prior.roms {
        if !identifier_set.contains(&rom.id) {
            continue;
        }
        roms.push(
            delta_map
                .get(&rom.id)
                .cloned()
                .unwrap_or_else(|| rom.clone()),
        );
    }
    let mut added: Vec<i64> = delta_map
        .keys()
        .filter(|id| !prior_ids.contains(id))
        .copied()
        .collect();
    added.sort_unstable();
    for id in added {
        if let Some(rom) = delta_map.get(&id) {
            roms.push(rom.clone());
        }
    }
    if roms.len() != identifier_set.len() {
        return Err(Error::InvalidCatalogue(
            "incremental refresh could not reconcile every visible ROM id".into(),
        ));
    }
    sink.emit(AppEvent::log(
        Level::Info,
        "catalogue",
        format!(
            "operation={operation_id} fetched {} platforms and {} ROMs in {}ms kind=incremental",
            platforms.len(),
            roms.len(),
            started.elapsed().as_millis()
        ),
    ));
    Ok(CatalogueSnapshot {
        platforms,
        roms,
        refreshed_at: started_unix,
        full_refreshed_at: prior.full_refreshed_at,
    })
}

/// Fetch every page of `GET /api/roms` (optionally filtered by
/// `updated_after`). The first page reports the total; the rest are fetched
/// `CATALOGUE_PAGE_CONCURRENCY` at a time and reassembled in offset order. The
/// declared total is authoritative: a mismatched offset, a first page that is
/// not exactly `min(limit, total)`, a short page before the total, or a
/// reassembly count that does not match fails the fetch.
fn fetch_rom_pages(
    client: &RommClient,
    sink: &EventSink,
    operation_id: u64,
    updated_after: Option<&str>,
    started: Instant,
) -> Result<Vec<RomDto>> {
    if !sink.is_active() {
        return Err(Error::Cancelled);
    }
    let request_started = Instant::now();
    let first = client.roms_page_flagged(CATALOGUE_PAGE_LIMIT, 0, true, updated_after);
    sink.emit(AppEvent::log(
        Level::Info,
        "http",
        format!(
            "operation={operation_id} GET /api/roms limit={CATALOGUE_PAGE_LIMIT} offset=0 elapsed_ms={} result={}",
            request_started.elapsed().as_millis(),
            http_result(&first)
        ),
    ));
    let first = first?;
    if first.offset != 0 {
        return Err(Error::InvalidCatalogue(
            "catalogue pagination offset did not advance correctly".into(),
        ));
    }
    // Respect the server's effective limit when it clamps the request, but a
    // zero limit cannot page and must not be silently replaced.
    let limit = first.limit;
    if limit == 0 {
        return Err(Error::InvalidCatalogue(
            "catalogue reported a zero page limit".into(),
        ));
    }
    let first_count = first.items.len() as u64;
    if first_count > limit {
        return Err(Error::InvalidCatalogue(
            "catalogue page exceeded the requested limit".into(),
        ));
    }
    let mut roms = first.items;
    let total = first.total;
    emit_catalogue_pages_progress(sink, operation_id, first_count, total, started);
    if !sink.is_active() {
        return Err(Error::Cancelled);
    }
    let Some(total) = total else {
        // An older server that ignores `with_total`: walk sequentially until a
        // short page. A first page that is already short is the whole
        // catalogue, so no further request is made.
        if first_count == limit {
            let mut offset = first_count;
            loop {
                if !sink.is_active() {
                    return Err(Error::Cancelled);
                }
                let page = client.roms_page_flagged(limit, offset, false, updated_after)?;
                if page.offset != offset {
                    return Err(Error::InvalidCatalogue(
                        "catalogue pagination offset did not advance correctly".into(),
                    ));
                }
                let count = page.items.len() as u64;
                if count > limit {
                    return Err(Error::InvalidCatalogue(
                        "catalogue page exceeded the requested limit".into(),
                    ));
                }
                roms.extend(page.items);
                emit_catalogue_pages_progress(sink, operation_id, roms.len() as u64, None, started);
                if count < limit {
                    break;
                }
                offset += count;
            }
        }
        return Ok(roms);
    };
    // With a declared total the first page must be exactly full, or exactly the
    // total when the whole catalogue fits on one page.
    let expected_first = limit.min(total);
    if first_count != expected_first {
        return Err(Error::InvalidCatalogue(format!(
            "catalogue first page had {first_count} of {expected_first} expected items"
        )));
    }
    if first_count >= total {
        return Ok(roms);
    }
    let mut offsets = Vec::new();
    let mut offset = first_count;
    while offset < total {
        offsets.push(offset);
        offset += limit;
    }
    for chunk in offsets.chunks(CATALOGUE_PAGE_CONCURRENCY) {
        if !sink.is_active() {
            return Err(Error::Cancelled);
        }
        let pages: Vec<(u64, u128, Result<RomsPage>)> = std::thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|&offset| {
                    scope.spawn(move || {
                        let request_started = Instant::now();
                        let page = client.roms_page_flagged(limit, offset, false, updated_after);
                        (offset, request_started.elapsed().as_millis(), page)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| {
                    handle.join().unwrap_or_else(|_| {
                        (
                            0,
                            0,
                            Err(Error::Transport("catalogue page worker panicked".into())),
                        )
                    })
                })
                .collect()
        });
        for (offset, elapsed, page) in pages {
            sink.emit(AppEvent::log(
                Level::Info,
                "http",
                format!(
                    "operation={operation_id} GET /api/roms limit={limit} offset={offset} elapsed_ms={elapsed} result={}",
                    http_result(&page)
                ),
            ));
            let page = page?;
            if page.offset != offset {
                return Err(Error::InvalidCatalogue(
                    "catalogue pagination offset did not advance correctly".into(),
                ));
            }
            let count = page.items.len() as u64;
            if count > limit {
                return Err(Error::InvalidCatalogue(
                    "catalogue page exceeded the requested limit".into(),
                ));
            }
            if count != limit && offset + count < total {
                return Err(Error::InvalidCatalogue(
                    "catalogue page ended before the declared total".into(),
                ));
            }
            roms.extend(page.items);
        }
        emit_catalogue_pages_progress(sink, operation_id, roms.len() as u64, Some(total), started);
        if !sink.is_active() {
            return Err(Error::Cancelled);
        }
    }
    if roms.len() as u64 != total {
        return Err(Error::InvalidCatalogue(format!(
            "catalogue reassembly mismatch: {} of {}",
            roms.len(),
            total
        )));
    }
    Ok(roms)
}

fn emit_catalogue_pages_progress(
    sink: &EventSink,
    operation_id: u64,
    completed: u64,
    total: Option<u64>,
    started: Instant,
) {
    sink.emit(AppEvent::CatalogueProgress {
        operation_id,
        stage: "Catalogue pages".into(),
        completed,
        total,
        elapsed_ms: started.elapsed().as_millis() as u64,
    });
}

fn http_result<T>(result: &Result<T>) -> String {
    match result {
        Ok(_) => "ok".into(),
        Err(error) => error.to_string(),
    }
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

// ---------------------------------------------------------------------------
// Download wiring: RomM client -> ContentSource for the DownloadManager.
// ---------------------------------------------------------------------------

struct ClientSource {
    client: Arc<RommClient>,
    /// `RomKey -> content_name` (the `file_name` used in the content URL).
    names: HashMap<RomKey, String>,
}

impl ContentSource for ClientSource {
    fn fetch(
        &self,
        key: &RomKey,
        writer: &mut dyn std::io::Write,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<u64> {
        let name = self.names.get(key).ok_or_else(|| {
            Error::InvalidCatalogue(format!("rom {} not in catalogue", key.rom_id))
        })?;
        self.client
            .download_original_file(key.file_id, name, writer, progress)
    }
}

// ---------------------------------------------------------------------------
// Mount root + backend seam.
// ---------------------------------------------------------------------------

/// Deny setup/publication while Windows has a frontend or Switch emulator image
/// open. Exclusive writable handles also prevent launch during the operation.
pub(crate) fn stopped_retrobat_guard(installation: &Path) -> anyhow::Result<Vec<std::fs::File>> {
    rommfs_core::save_sync::ensure_no_reparse_components(installation)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let mut guards = Vec::new();
        for relative in [
            "retrobat.exe",
            "emulationstation/emulationstation.exe",
            "emulators/eden/eden.exe",
            "emulators/citron/citron.exe",
            "emulators/retroarch/retroarch.exe",
        ] {
            let path = installation.join(relative);
            rommfs_core::save_sync::ensure_no_reparse_components(&path)?;
            if !path.try_exists()? {
                continue;
            }
            guards.push(std::fs::OpenOptions::new().read(true).write(true).share_mode(0)
                .open(&path).with_context(|| format!("Stop RetroBat and its emulators before setup or metadata publication; exclusive access to {} is unavailable", path.display()))?);
        }
        Ok(guards)
    }
    #[cfg(not(windows))]
    {
        Ok(Vec::new())
    }
}

/// Whether a PS1/PS2 card emulator image is clear to touch a card.
pub(crate) enum CardEmulatorState {
    /// Exclusive handles are held for the pass.
    Stopped(Vec<std::fs::File>),
    /// An emulator image is open, so a shared card may be in use.
    #[cfg_attr(not(windows), allow(dead_code))]
    Running,
    /// The guard could not be established; the caller must not sync.
    Unavailable(String),
}

/// Deny card access while a DuckStation or PCSX2 image is open. Instead of
/// guessing executable names, every executable inside each installed emulator
/// folder is opened exclusively; an installed folder with no executable yields
/// [`CardEmulatorState::Unavailable`], never a silent pass.
pub(crate) fn card_emulator_state(installation: &Path) -> CardEmulatorState {
    if let Err(error) = rommfs_core::save_sync::ensure_no_reparse_components(installation) {
        return CardEmulatorState::Unavailable(error.to_string());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let mut guards = Vec::new();
        for emulator in ["duckstation", "pcsx2"] {
            let directory = installation.join("emulators").join(emulator);
            match std::fs::symlink_metadata(&directory) {
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => {
                    return CardEmulatorState::Unavailable(format!(
                        "{} is not a directory",
                        directory.display()
                    ))
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return CardEmulatorState::Unavailable(error.to_string()),
            }
            let mut executables = Vec::new();
            if let Err(error) = collect_executables(&directory, 0, &mut executables) {
                return CardEmulatorState::Unavailable(error.to_string());
            }
            if executables.is_empty() {
                return CardEmulatorState::Unavailable(format!(
                    "no executable found under {}",
                    directory.display()
                ));
            }
            for path in executables {
                if let Err(error) = rommfs_core::save_sync::ensure_no_reparse_components(&path) {
                    return CardEmulatorState::Unavailable(error.to_string());
                }
                match std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .share_mode(0)
                    .open(&path)
                {
                    Ok(file) => guards.push(file),
                    Err(error) if matches!(error.raw_os_error(), Some(5 | 32 | 33)) => {
                        return CardEmulatorState::Running
                    }
                    Err(error) => {
                        return CardEmulatorState::Unavailable(format!(
                            "{}: {error}",
                            path.display()
                        ))
                    }
                }
            }
        }
        CardEmulatorState::Stopped(guards)
    }
    #[cfg(not(windows))]
    {
        CardEmulatorState::Stopped(Vec::new())
    }
}

#[cfg(windows)]
fn collect_executables(
    directory: &Path,
    depth: u32,
    out: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    if depth > 3 {
        return Ok(());
    }
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect_executables(&path, depth + 1, out)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
        {
            out.push(path);
        }
    }
    Ok(())
}

pub(crate) fn check_mount_root(root: &Path, server_id: &str) -> Result<()> {
    let parent = root
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| {
            Error::Unsupported("choose a mount directory below an existing parent directory".into())
        })?;
    // Guard the existing ancestor chain before mutating anything: creating
    // under a link or reparse point would mutate its target, not the path the
    // user chose.
    let existing = parent
        .ancestors()
        .find(|path| path.exists())
        .ok_or_else(|| {
            Error::Unsupported("choose a mount directory below an existing parent directory".into())
        })?;
    let _guard = rommfs_core::save_sync::hold_save_directory_chain(existing).map_err(|e| {
        Error::Unsupported(format!("protect mount parent {}: {e}", existing.display()))
    })?;
    if let Err(e) = create_mount_dir_chain(existing, root) {
        return Err(mount_root_error(anyhow::Error::new(e).context(format!(
            "cannot create mount directory {}",
            root.display()
        ))));
    }
    #[cfg(windows)]
    rommfs_cfapi::check_prerequisites(root).map_err(mount_root_error)?;
    rommfs_cfapi::check_mount_root(root, server_id)
        .map(|_| ())
        .map_err(mount_root_error)
}

/// Create every missing component from `existing` down to `root`, rejecting an
/// existing component that is a link, reparse point, or non-directory before
/// it can be followed — `create_dir_all` alone would silently create into a
/// link's target. Each new component is held so a racing process cannot swap
/// it for a link before the next level is created.
fn create_mount_dir_chain(existing: &Path, root: &Path) -> Result<()> {
    let mut _created = Vec::new();
    let mut current = existing.to_path_buf();
    let suffix = root.strip_prefix(existing).unwrap_or(root);
    for component in suffix.components() {
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if !metadata.is_dir() || rommfs_core::save_sync::path_is_reparse_point(&metadata) {
                    return Err(Error::Unsupported(format!(
                        "mount path traverses a symbolic link or reparse point: {}",
                        current.display()
                    )));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&current).map_err(Error::Io)?;
            }
            Err(e) => return Err(Error::Io(e)),
        }
        _created.push(
            rommfs_core::save_sync::hold_save_directory_chain(&current).map_err(|e| {
                Error::Unsupported(format!(
                    "protect mount directory {}: {e}",
                    current.display()
                ))
            })?,
        );
    }
    Ok(())
}

pub(crate) fn claim_mount_root(root: &Path, server_id: &str) -> Result<()> {
    rommfs_cfapi::claim_mount_root(root, server_id).map_err(mount_root_error)
}

/// Aggregate the per-file warnings a catalogue build produces into one line
/// per reason with a count and a few examples. Per-file detail is dropped: a
/// large library produced thousands of near-identical lines every refresh.
fn emit_catalogue_warnings(sink: &EventSink, warnings: Vec<String>) {
    for message in summarize_catalogue_warnings(&warnings) {
        sink.emit(AppEvent::log(Level::Warn, "catalogue", message));
    }
}

#[derive(Default)]
struct WarningGroup {
    /// Distinct subjects, so a rom-level and file-level report of the same
    /// thing count once.
    subjects: std::collections::BTreeSet<(i64, i64)>,
    files: bool,
    other: usize,
}

const WARNING_EXAMPLES: usize = 5;

/// "rom 12 file 40: reason" → ((12, 40), reason); "rom 12: reason" →
/// ((12, 0), reason). Anything else passes through unparsed.
/// (rom id, file id or 0), whether it was file-scoped, the reason.
type ParsedWarning<'a> = ((i64, i64), bool, &'a str);

fn parse_catalogue_warning(warning: &str) -> Option<ParsedWarning<'_>> {
    let rest = warning.strip_prefix("rom ")?;
    let (rom, rest) = rest.split_once([' ', ':'])?;
    let rom: i64 = rom.parse().ok()?;
    // `rest` begins just after the separator that ended the id.
    let separator = warning.as_bytes()["rom ".len() + rom.to_string().len()];
    if separator == b':' {
        return Some(((rom, 0), false, rest.trim_start()));
    }
    let (file, reason) = rest.strip_prefix("file ")?.split_once(": ")?;
    Some(((rom, file.parse().ok()?), true, reason))
}

fn summarize_catalogue_warnings(warnings: &[String]) -> Vec<String> {
    let mut groups: std::collections::BTreeMap<String, WarningGroup> = Default::default();
    for warning in warnings {
        match parse_catalogue_warning(warning) {
            Some((subject, is_file, reason)) => {
                // Per-file detail ("adjusted to <name>") is not part of the reason.
                let reason = if reason.starts_with("visible path adjusted") {
                    "visible path adjusted"
                } else {
                    reason
                };
                let group = groups.entry(reason.to_string()).or_default();
                group.files |= is_file;
                group.subjects.insert(subject);
            }
            None => groups.entry(warning.clone()).or_default().other += 1,
        }
    }
    groups
        .into_iter()
        .map(|(reason, group)| {
            if group.subjects.is_empty() {
                return if group.other > 1 {
                    format!("{reason} (\u{d7}{})", group.other)
                } else {
                    reason
                };
            }
            let examples: Vec<String> = group
                .subjects
                .iter()
                .take(WARNING_EXAMPLES)
                .map(|(rom, file)| match file {
                    0 => format!("rom {rom}"),
                    file => format!("rom {rom} file {file}"),
                })
                .collect();
            format!(
                "{} {}: {reason} (e.g. {})",
                group.subjects.len(),
                if group.files { "files" } else { "roms" },
                examples.join(", ")
            )
        })
        .collect()
}

fn mount_root_error(e: anyhow::Error) -> Error {
    let mut message = format!("{e:#}");
    if let Some(io) = e.downcast_ref::<std::io::Error>() {
        let kind = io.kind();
        if kind == std::io::ErrorKind::PermissionDenied {
            message.push_str(
                "; choose a mount directory you can create and own, such as %USERPROFILE%\\RomM",
            );
        }
        Error::Io(std::io::Error::new(kind, message))
    } else {
        Error::Unsupported(message)
    }
}

#[cfg(windows)]
fn hydrated_remover(root: &Path) -> Arc<dyn HydratedRemover> {
    Arc::new(rommfs_cfapi::WindowsHydratedRemover::new(root))
}

#[cfg(not(windows))]
fn hydrated_remover(_root: &Path) -> Arc<dyn HydratedRemover> {
    Arc::new(NoopHydratedRemover)
}

/// A live mount owned and stopped on the worker thread.
struct ActiveMount {
    root: PathBuf,
    stop_fn: Box<dyn FnOnce()>,
}

impl ActiveMount {
    fn stop(self) {
        (self.stop_fn)();
    }
}

/// Connect the Windows Cloud Files provider over the portable core.
#[cfg(windows)]
fn start_mount_backend(fs: Arc<RommFs>, root: &Path) -> Result<ActiveMount> {
    let mount = rommfs_cfapi::WindowsMount::mount(fs, root).map_err(mount_root_error)?;
    Ok(ActiveMount {
        root: root.to_path_buf(),
        stop_fn: Box::new(move || mount.stop()),
    })
}

#[cfg(not(windows))]
fn start_mount_backend(_fs: Arc<RommFs>, _root: &Path) -> Result<ActiveMount> {
    Err(Error::Unsupported(
        "mounting requires Windows 10 1709+ with Cloud Files and a local NTFS directory".into(),
    ))
}

/// Private cache location, outside the projected tree (PRD §5) and scoped
/// per server so different servers never share entries (R4).
fn cache_dir_for(server_id: &str) -> PathBuf {
    let safe: String = server_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("rommfs").join("cache").join(if safe.is_empty() {
        "default".into()
    } else {
        safe
    })
}

pub fn diagnostic_log_directory() -> Option<PathBuf> {
    save_sync_settings_path().and_then(|path| path.parent()?.parent().map(|p| p.join("logs")))
}

fn save_sync_settings_path() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .map(|profile| PathBuf::from(profile).join("AppData").join("Local"))
        });
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    base.map(|path| path.join("rommfs").join("settings").join("save-sync.db"))
}

fn save_sync_discovery_input() -> DiscoveryInput {
    #[cfg(windows)]
    {
        crate::retrobat_discovery_windows::discovery_input()
    }
    #[cfg(not(windows))]
    {
        DiscoveryInput::default()
    }
}

fn selected_installation_problem(problem: InstallationProblem) -> String {
    match problem {
        InstallationProblem::Missing => {
            "The previously selected RetroBat installation is missing; choose another explicitly."
                .into()
        }
        InstallationProblem::Inaccessible => {
            "The previously selected RetroBat installation is inaccessible; sync is paused.".into()
        }
        InstallationProblem::Invalid(reason) => {
            format!("The selected path is not a supported RetroBat installation: {reason}.")
        }
    }
}

fn log_line(level: Level, op: &'static str, message: String) -> rommfs_core::events::LogLine {
    rommfs_core::events::LogLine {
        unix_secs: rommfs_core::cache::now_unix_secs(),
        level,
        op,
        message,
    }
}

#[cfg(test)]
mod tests;
