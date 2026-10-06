//! Headless app controller: the real state machine the GPUI window renders.
//! Drives workers via commands, consumes `AppEvent`s into `UiState`.
//! Tests exercise this path — no duplicate state machine in the UI (PRD §6).

use crate::save_sync_agent::{SaveSyncAgent, SaveSyncCommandGate};
use rommfs_core::cache::clock::{Clock, SystemClock, DEFAULT_EVICTION_THRESHOLD_SECS};
#[cfg(not(windows))]
use rommfs_core::cache::NoopHydratedRemover;
use rommfs_core::cache::{CacheIndex, Evictor, HydratedRemover, LiveState};
use rommfs_core::catalog::{build_catalogue, server_id_of, Catalogue, RomKey};
use rommfs_core::download::{ContentSource, DownloadManager};
use rommfs_core::error::{Error, Result};
use rommfs_core::events::{
    AppEvent, EventSink, Level, LogBuffer, SaveSyncGameStatus, SaveSyncQueueStatus,
};
use rommfs_core::fscore::RommFs;
use rommfs_core::romm::{Credentials, RommClient};
use rommfs_core::save_sync::{
    discover_installations, map_catalogue, resolve_retrobat_gb_profile, validate_installation,
    ConsentSettings, DiscoveryInput, InstallationCandidate, InstallationProblem, MappingReport,
    RetroBatGbProfile, SaveSyncJournal, SaveSyncScope, SaveSyncSettingsStore, SnapshotState,
};
use rommfs_core::tree::RommTree;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const EVICTION_INTERVAL: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnState {
    Idle,
    Connecting,
    Connected,
    Failed,
    SignInRequired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountState {
    NotMounted,
    Mounting,
    Mounted,
    Failed,
}

#[derive(Clone, Debug)]
pub struct DownloadView {
    pub rom_id: u64,
    pub file_name: String,
    pub received: u64,
    pub total: Option<u64>,
    /// None = in flight; Some(Ok) done; Some(Err) terminal failure state.
    pub finished: Option<std::result::Result<(), String>>,
}

#[derive(Clone, Debug)]
pub struct SaveSyncTransferView {
    pub rom_id: i64,
    pub revision: String,
    pub phase: String,
    pub detail: Option<String>,
}

/// How many download rows are kept on screen; oldest finished rows are
/// dropped first (the log carries the durable record anyway).
const MAX_DOWNLOAD_ROWS: usize = 128;

/// Everything the window draws — derived ONLY from real worker events.
pub struct UiState {
    pub conn: ConnState,
    pub conn_error: Option<String>,
    pub mount: MountState,
    pub mount_path: Option<String>,
    pub mount_error: Option<String>,
    pub downloads: Vec<DownloadView>,
    pub catalogue: Option<(usize, usize, usize)>, // platforms, roms, skipped
    pub save_sync_candidates: Vec<InstallationCandidate>,
    pub save_sync_skipped: usize,
    pub save_sync_selected_root: Option<String>,
    pub save_sync_documented_saves_root: Option<String>,
    pub save_sync_effective_saves_root: Option<String>,
    pub save_sync_profile_version: Option<String>,
    pub save_sync_account_id: Option<i64>,
    pub save_sync_mapped_targets: usize,
    pub save_sync_catalogue_unmapped: usize,
    pub save_sync_existing_saves: Option<rommfs_core::save_sync::ExistingSavePreview>,
    pub save_sync_preview: Vec<String>,
    pub save_sync_available: bool,
    pub save_sync_problem: Option<String>,
    pub save_sync_enabled: bool,
    pub save_sync_debounce_secs: u32,
    pub save_sync_games: Vec<SaveSyncGameStatus>,
    pub save_sync_reconciled_games: usize,
    pub save_sync_pending_incoming: usize,
    pub save_sync_attention_games: usize,
    pub save_sync_failure: Option<String>,
    pub save_sync_transfers: Vec<SaveSyncTransferView>,
    pub save_sync_session_id: u64,
    pub save_sync_server_id: Option<String>,
    pub save_sync_queue: Option<SaveSyncQueueStatus>,
    pub save_sync_authentication_required: bool,
    pub save_sync_export_errors: HashMap<String, String>,
    pub save_sync_export_feedback: HashMap<String, String>,
    pub log: LogBuffer,
}

impl UiState {
    pub fn new(log_cap: usize) -> Self {
        Self {
            conn: ConnState::Idle,
            conn_error: None,
            mount: MountState::NotMounted,
            mount_path: None,
            mount_error: None,
            downloads: Vec::new(),
            catalogue: None,
            save_sync_candidates: Vec::new(),
            save_sync_skipped: 0,
            save_sync_selected_root: None,
            save_sync_documented_saves_root: None,
            save_sync_effective_saves_root: None,
            save_sync_profile_version: None,
            save_sync_account_id: None,
            save_sync_mapped_targets: 0,
            save_sync_catalogue_unmapped: 0,
            save_sync_existing_saves: None,
            save_sync_preview: Vec::new(),
            save_sync_available: false,
            save_sync_problem: None,
            save_sync_enabled: false,
            save_sync_debounce_secs: rommfs_core::save_sync::DEFAULT_DEBOUNCE_SECS,
            save_sync_games: Vec::new(),
            save_sync_reconciled_games: 0,
            save_sync_pending_incoming: 0,
            save_sync_attention_games: 0,
            save_sync_failure: None,
            save_sync_transfers: Vec::new(),
            save_sync_session_id: 0,
            save_sync_server_id: None,
            save_sync_queue: None,
            save_sync_authentication_required: false,
            save_sync_export_errors: HashMap::new(),
            save_sync_export_feedback: HashMap::new(),
            log: LogBuffer::new(log_cap),
        }
    }

    /// Apply one worker event. This is THE transition table the UI uses.
    pub fn apply(&mut self, event: &AppEvent) {
        match event {
            AppEvent::Connecting => {
                self.conn = ConnState::Connecting;
                self.conn_error = None;
                // A new connection attempt supersedes any prior session's
                // catalogue/downloads — never show stale data for a server
                // we may no longer be talking to.
                self.catalogue = None;
                self.downloads.clear();
                self.save_sync_games.clear();
                self.save_sync_reconciled_games = 0;
                self.save_sync_pending_incoming = 0;
                self.save_sync_attention_games = 0;
                self.save_sync_failure = None;
                self.save_sync_transfers.clear();
                self.save_sync_queue = None;
                self.save_sync_authentication_required = false;
                self.save_sync_export_errors.clear();
                self.save_sync_export_feedback.clear();
                self.save_sync_enabled = false;
                self.save_sync_available = false;
                self.save_sync_account_id = None;
                self.save_sync_server_id = None;
                self.save_sync_effective_saves_root = None;
                self.save_sync_mapped_targets = 0;
                self.save_sync_catalogue_unmapped = 0;
                self.save_sync_existing_saves = None;
            }
            AppEvent::Connected => {
                self.conn = ConnState::Connected;
                self.conn_error = None;
            }
            AppEvent::ConnectFailed { reason } => {
                self.conn = ConnState::Failed;
                self.conn_error = Some(reason.clone());
            }
            AppEvent::SignInRequired => {
                self.conn = ConnState::SignInRequired;
                self.conn_error = Some("sign-in required".to_string());
            }

            AppEvent::CatalogueLoading => {}
            AppEvent::CatalogueLoaded {
                platforms,
                roms,
                skipped_unsupported,
            } => {
                self.catalogue = Some((*platforms, *roms, *skipped_unsupported));
            }
            AppEvent::CatalogueFailed { reason } => {
                // Auth may have succeeded while the catalogue fetch failed:
                // stay Connected but surface the error — a failed catalogue
                // must never render as a successful (empty) library (R1).
                self.conn_error = Some(reason.clone());
            }

            AppEvent::MountStarting { path } => {
                self.mount = MountState::Mounting;
                self.mount_path = Some(path.clone());
                self.mount_error = None;
            }
            AppEvent::MountStarted { path } => {
                self.mount = MountState::Mounted;
                self.mount_path = Some(path.clone());
                self.mount_error = None;
            }
            AppEvent::MountFailed { reason } => {
                self.mount = MountState::Failed;
                self.mount_error = Some(reason.clone());
            }
            AppEvent::MountStopping => {}
            AppEvent::MountStopped => {
                self.mount = MountState::NotMounted;
                // In-flight transfers died with the mount; don't leave rows
                // looking like they are still downloading.
                for d in &mut self.downloads {
                    if d.finished.is_none() {
                        d.finished = Some(Err("mount stopped".to_string()));
                    }
                }
            }

            AppEvent::SaveSyncUpdated {
                session_id,
                server_id,
                candidates,
                skipped,
                selected_root,
                documented_saves_root,
                effective_saves_root,
                profile_version,
                account_id,
                mapped_targets,
                catalogue_unmapped,
                existing_saves,
                preview_saves,
                available,
                selected_problem,
                enabled,
                debounce_secs,
            } => {
                if *session_id != self.save_sync_session_id {
                    return;
                }
                self.save_sync_candidates = candidates.clone();
                self.save_sync_server_id = server_id.clone();
                self.save_sync_skipped = *skipped;
                self.save_sync_selected_root = selected_root.clone();
                self.save_sync_documented_saves_root = documented_saves_root.clone();
                self.save_sync_effective_saves_root = effective_saves_root.clone();
                self.save_sync_profile_version = profile_version.clone();
                self.save_sync_account_id = *account_id;
                self.save_sync_mapped_targets = *mapped_targets;
                self.save_sync_catalogue_unmapped = *catalogue_unmapped;
                self.save_sync_existing_saves = existing_saves.clone();
                self.save_sync_preview = preview_saves.clone();
                self.save_sync_available = *available;
                self.save_sync_problem = selected_problem.clone();
                self.save_sync_enabled = *enabled;
                self.save_sync_debounce_secs = *debounce_secs;
            }
            AppEvent::SaveSyncReconciliation {
                session_id,
                game,
                reconciled_games,
                pending_incoming,
                attention_games,
                failure,
                ..
            } => {
                if *session_id != self.save_sync_session_id {
                    return;
                }
                if let Some(existing) = self
                    .save_sync_games
                    .iter_mut()
                    .find(|existing| existing.rom_id == game.rom_id)
                {
                    *existing = game.clone();
                } else {
                    self.save_sync_games.push(game.clone());
                }
                self.save_sync_reconciled_games = *reconciled_games;
                self.save_sync_pending_incoming = *pending_incoming;
                self.save_sync_attention_games = *attention_games;
                self.save_sync_failure = failure.clone();
            }
            AppEvent::SaveSyncTransferProgress {
                session_id,
                rom_id,
                revision,
                phase,
                detail,
            } => {
                if *session_id != self.save_sync_session_id {
                    return;
                }
                self.save_sync_transfers.push(SaveSyncTransferView {
                    rom_id: *rom_id,
                    revision: revision.clone(),
                    phase: phase.clone(),
                    detail: detail.clone(),
                });
                if self.save_sync_transfers.len() > 64 {
                    self.save_sync_transfers.remove(0);
                }
            }
            AppEvent::SaveSyncSessionChanged { session_id } => {
                if *session_id >= self.save_sync_session_id {
                    self.save_sync_session_id = *session_id;
                    self.save_sync_enabled = false;
                    self.save_sync_available = false;
                    self.save_sync_account_id = None;
                    self.save_sync_server_id = None;
                    self.save_sync_effective_saves_root = None;
                    self.save_sync_mapped_targets = 0;
                    self.save_sync_catalogue_unmapped = 0;
                    self.save_sync_existing_saves = None;
                    self.save_sync_games.clear();
                    self.save_sync_reconciled_games = 0;
                    self.save_sync_pending_incoming = 0;
                    self.save_sync_attention_games = 0;
                    self.save_sync_failure = None;
                    self.save_sync_transfers.clear();
                    self.save_sync_queue = None;
                    self.save_sync_authentication_required = false;
                    self.save_sync_export_errors.clear();
                    self.save_sync_export_feedback.clear();
                }
            }
            AppEvent::SaveSyncAuthenticationRequired { session_id } => {
                if *session_id == self.save_sync_session_id {
                    self.save_sync_authentication_required = true;
                    self.conn = ConnState::SignInRequired;
                }
            }
            AppEvent::SaveSyncQueueUpdated(status) => {
                if status.session_id == self.save_sync_session_id {
                    self.save_sync_reconciled_games = status.reconciled_games;
                    self.save_sync_pending_incoming = status.pending_incoming;
                    self.save_sync_attention_games = status.attention_games;
                    self.save_sync_failure = status.failure.clone();
                    self.save_sync_authentication_required = status.authentication_required;
                    self.save_sync_games = status.games.clone();
                    self.save_sync_queue = Some(status.clone());
                }
            }
            AppEvent::SaveSyncExportFinished {
                session_id,
                incoming_id,
                destination,
                error,
            } => {
                if *session_id == self.save_sync_session_id {
                    if let Some(error) = error {
                        self.save_sync_export_feedback.remove(incoming_id);
                        self.save_sync_export_errors
                            .insert(incoming_id.clone(), error.clone());
                    } else if let Some(destination) = destination {
                        self.save_sync_export_errors.remove(incoming_id);
                        self.save_sync_export_feedback
                            .insert(incoming_id.clone(), destination.clone());
                    }
                }
            }

            AppEvent::DownloadStarted {
                rom_id,
                file_name,
                total,
            } => {
                let row = self.download_row(*rom_id, file_name);
                row.received = 0;
                row.total = *total;
                row.finished = None;
            }
            AppEvent::DownloadProgress {
                rom_id,
                received,
                total,
            } => {
                let file_name = String::new();
                let row = self.download_row(*rom_id, &file_name);
                row.received = *received;
                if total.is_some() {
                    row.total = *total;
                }
            }
            AppEvent::DownloadFinished { rom_id, file_name } => {
                let row = self.download_row(*rom_id, file_name);
                row.finished = Some(Ok(()));
            }
            AppEvent::DownloadFailed {
                rom_id,
                file_name,
                reason,
            } => {
                let row = self.download_row(*rom_id, file_name);
                row.finished = Some(Err(reason.clone()));
            }

            AppEvent::Evicted { rom_id, file_name } => {
                self.log.push(log_line(
                    Level::Info,
                    "evict",
                    format!("evicted {file_name} (rom {rom_id})"),
                ));
            }
            AppEvent::EvictFailed { rom_id, reason } => {
                self.log.push(log_line(
                    Level::Error,
                    "evict",
                    format!("eviction failed for rom {rom_id}: {reason}"),
                ));
            }

            AppEvent::Log(line) => self.log.push(line.clone()),
        }
        self.prune_downloads();
    }

    /// Mark a mount request pending immediately so a second UI click cannot
    /// enqueue another start before the worker's MountStarting event arrives.
    pub fn request_mount(&mut self, path: String) {
        self.mount = MountState::Mounting;
        self.mount_path = Some(path);
        self.mount_error = None;
    }

    fn download_row(&mut self, rom_id: u64, file_name: &str) -> &mut DownloadView {
        if let Some(pos) = self.downloads.iter().position(|d| d.rom_id == rom_id) {
            let row = &mut self.downloads[pos];
            if !file_name.is_empty() {
                row.file_name = file_name.to_string();
            }
            return row;
        }
        self.downloads.push(DownloadView {
            rom_id,
            file_name: file_name.to_string(),
            received: 0,
            total: None,
            finished: None,
        });
        self.downloads.last_mut().unwrap()
    }

    fn prune_downloads(&mut self) {
        while self.downloads.len() > MAX_DOWNLOAD_ROWS {
            let drop_at = self
                .downloads
                .iter()
                .position(|d| d.finished.is_some())
                .unwrap_or(0);
            self.downloads.remove(drop_at);
        }
    }
}

/// Commands the window sends to the controller.
#[derive(Debug)]
pub enum Command {
    Connect {
        url: String,
        username: String,
        password: String,
    },
    StartMount {
        path: String,
    },
    StopMount,
    RefreshSaveSync,
    SelectSaveSyncInstallation {
        path: PathBuf,
    },
    SetSaveSyncEnabled {
        enabled: bool,
    },
    SetSaveSyncDebounce {
        seconds: u32,
    },
    ExportSaveSyncIncoming {
        session_id: u64,
        incoming_id: String,
        destination: PathBuf,
    },
    Shutdown,
}

struct CommandEnvelope {
    command: Command,
    save_sync_epoch: u64,
}

fn invalidates_save_sync_scope(command: &Command) -> bool {
    matches!(
        command,
        Command::Connect { .. }
            | Command::StartMount { .. }
            | Command::RefreshSaveSync
            | Command::SelectSaveSyncInstallation { .. }
            | Command::SetSaveSyncEnabled { enabled: false }
            | Command::Shutdown
    )
}

/// Owns the worker session (client + catalogue + mount + evict timer).
/// Created on Connect; events flow back over the `EventSink` channel.
pub struct Controller {
    sink: EventSink,
    cmd_tx: mpsc::Sender<CommandEnvelope>,
    // worker thread handle held for shutdown joining
    worker: Option<JoinHandle<()>>,
    save_sync_gate: SaveSyncCommandGate,
}

impl Controller {
    /// Spawn the controller + worker loop. Returns (controller, event rx).
    pub fn spawn() -> (Self, mpsc::Receiver<AppEvent>) {
        let (sink, event_rx) = rommfs_core::events::channel();
        let (cmd_tx, cmd_rx) = mpsc::channel::<CommandEnvelope>();
        let worker_sink = sink.clone();
        let save_sync_gate = SaveSyncCommandGate::new();
        let worker_gate = save_sync_gate.clone();
        let worker = std::thread::Builder::new()
            .name("rommfs-worker".to_string())
            .spawn(move || worker_loop(cmd_rx, worker_sink, worker_gate))
            .expect("spawn worker thread");
        let initial_epoch = save_sync_gate.invalidate();
        let _ = cmd_tx.send(CommandEnvelope {
            command: Command::RefreshSaveSync,
            save_sync_epoch: initial_epoch,
        });
        (
            Self {
                sink,
                cmd_tx,
                worker: Some(worker),
                save_sync_gate,
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
        let _ = self.cmd_tx.send(CommandEnvelope {
            command: cmd,
            save_sync_epoch,
        });
    }

    pub fn sink(&self) -> EventSink {
        self.sink.clone()
    }
}

impl Drop for Controller {
    /// Closing the app stops the worker and mount (PRD: no background
    /// process survives the window).
    fn drop(&mut self) {
        self.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

// ---------------------------------------------------------------------------
// Worker side (all network/filesystem work off the UI thread, PRD R5)
// ---------------------------------------------------------------------------

/// What the worker holds about the active server session. One server, one
/// mount — rebuilt on Connect/StartMount, never shared across servers.
struct Worker {
    sink: EventSink,
    client: Option<Arc<RommClient>>,
    server_id: Option<String>,
    /// Latest built catalogue; consumed into `RommTree` on mount.
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
    current_save_sync_epoch: u64,
    save_sync_runtime_problem: Option<String>,
    save_sync_session_id: u64,
}

impl Worker {
    #[cfg(test)]
    fn new(sink: EventSink) -> Self {
        Self::new_with_gate(sink, SaveSyncCommandGate::new())
    }

    fn new_with_gate(sink: EventSink, save_sync_gate: SaveSyncCommandGate) -> Self {
        let current_save_sync_epoch = save_sync_gate.current_epoch();
        Self {
            sink,
            client: None,
            server_id: None,
            catalogue: None,
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
            current_save_sync_epoch,
            save_sync_runtime_problem: None,
            save_sync_session_id: 0,
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
        worker.save_sync_settings = save_sync_settings;
        worker.save_sync_storage_dir =
            settings_path.and_then(|path| path.parent().map(Path::to_path_buf));
        worker.save_sync_selected_root = save_sync_selected_root;
        worker.save_sync_settings_problem = save_sync_settings_problem;
        worker
    }

    fn refresh_save_sync(&mut self) {
        self.refresh_save_sync_from(save_sync_discovery_input());
    }

    fn refresh_save_sync_from(&mut self, input: DiscoveryInput) {
        self.advance_save_sync_session();
        self.stop_save_sync_agent();
        let discovery = discover_installations(input);
        let mut selected_problem = self.save_sync_settings_problem.clone();
        let mut selected_root = None;
        let mut documented_saves_root = None;
        let mut effective_saves_root = None;
        let mut profile_version = None;
        let mut account_id = None;
        let mut mapped_targets = 0;
        let mut catalogue_unmapped = 0;
        let mut existing_saves = None;
        let mut preview_saves = Vec::new();
        let mut profile = None;
        let mut mapping_report = None;

        if let Some(root) = self.save_sync_selected_root.as_deref() {
            selected_root = Some(root.display().to_string());
            match validate_installation(root) {
                Err(problem) => selected_problem = Some(selected_installation_problem(problem)),
                Ok(info) => {
                    selected_root = Some(info.install_root.display().to_string());
                    documented_saves_root = Some(info.documented_saves_root.display().to_string());
                    let visible_gb_names = self
                        .save_sync_catalogue
                        .as_ref()
                        .map(|catalogue| {
                            catalogue
                                .entries
                                .iter()
                                .filter(|entry| entry.platform_dir.eq_ignore_ascii_case("gb"))
                                .map(|entry| entry.file_name.clone())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    match resolve_retrobat_gb_profile(root, &visible_gb_names) {
                        Err(error) => selected_problem = Some(error.to_string()),
                        Ok(resolved) => {
                            effective_saves_root =
                                Some(resolved.effective_saves_root.display().to_string());
                            profile_version = Some(resolved.version.clone());
                            if let Some(catalogue) = self.save_sync_catalogue.as_ref() {
                                match map_catalogue(catalogue, &resolved.effective_saves_root) {
                                    Ok(report) => {
                                        mapped_targets = report.supported_count();
                                        catalogue_unmapped = report.unmapped_count();
                                        existing_saves =
                                            Some(rommfs_core::save_sync::preview_existing_saves(
                                                &resolved.effective_saves_root,
                                                &report,
                                            ));
                                        preview_saves = report
                                            .mappings
                                            .iter()
                                            .take(5)
                                            .map(|mapping| {
                                                mapping.target_path.display().to_string()
                                            })
                                            .collect();
                                        mapping_report = Some(report);
                                    }
                                    Err(error) => selected_problem = Some(error.to_string()),
                                }
                            }
                            profile = Some(resolved);
                        }
                    }
                }
            }
        }

        if let Some(identity) = &self.save_sync_identity {
            account_id = Some(identity.account_id);
        } else if self.client.is_some() {
            selected_problem.get_or_insert_with(|| {
                "Save sync needs a verified authenticated account with me.read, assets.read, and assets.write.".into()
            });
        }

        let scope = match (
            self.server_id.as_ref(),
            self.save_sync_identity.as_ref(),
            self.save_sync_selected_root.as_ref(),
            profile.as_ref(),
        ) {
            (Some(server_id), Some(identity), Some(installation_root), Some(profile)) => {
                Some(SaveSyncScope {
                    server_id: server_id.clone(),
                    account_id: identity.account_id.to_string(),
                    installation_root: installation_root.clone(),
                    effective_saves_root: profile.effective_saves_root.clone(),
                })
            }
            _ => None,
        };
        let consent = match (self.save_sync_settings.as_ref(), scope.as_ref()) {
            (Some(settings), Some(scope)) => match settings.load(scope) {
                Ok(consent) => Some(consent),
                Err(error) => {
                    selected_problem = Some(format!("Consent settings could not be read: {error}"));
                    None
                }
            },
            _ => None,
        };
        self.save_sync_scope = scope.clone();
        self.save_sync_profile = profile;
        self.save_sync_mapping_report = mapping_report;
        if let Some(consent) = consent {
            self.save_sync_enabled = consent.enabled;
            self.save_sync_debounce_secs = consent.debounce_secs;
        } else {
            self.save_sync_enabled = false;
        }

        let save_permissions = self
            .save_sync_identity
            .as_ref()
            .is_some_and(|identity| identity.can_read_saves() && identity.can_write_saves());
        if self.save_sync_identity.is_some() && !save_permissions {
            selected_problem = Some(
                "The authenticated account lacks assets.read or assets.write; ROM access is unchanged.".into(),
            );
        }
        let available = scope.is_some()
            && save_permissions
            && self.save_sync_settings.is_some()
            && self
                .save_sync_mapping_report
                .as_ref()
                .is_some_and(|report| report.supported_count() > 0);

        #[cfg(windows)]
        let available = available
            && scope.as_ref().is_some_and(|scope| {
                match crate::save_sync_agent::check_save_root_writable(&scope.effective_saves_root)
                {
                    Ok(()) => true,
                    Err(error) => {
                        selected_problem = Some(error.to_string());
                        false
                    }
                }
            });

        if self.save_sync_enabled && available {
            if let (Some(scope), Some(report), Some(client), Some(identity)) = (
                scope.as_ref(),
                self.save_sync_mapping_report.as_ref(),
                self.client.as_ref(),
                self.save_sync_identity.as_ref(),
            ) {
                if !report.mappings.is_empty() {
                    if let Some(settings_dir) = &self.save_sync_storage_dir {
                        let journal_path = settings_dir.join("save-sync-journal.db");
                        let spool_path = settings_dir.join("save-sync-snapshots");
                        let epoch = self.current_save_sync_epoch;
                        if self.save_sync_gate.open(epoch) && self.save_sync_gate.enabled_for(epoch)
                        {
                            match SaveSyncAgent::start(
                                scope.clone(),
                                report.mappings.clone(),
                                identity.clone(),
                                Arc::clone(client),
                                journal_path,
                                spool_path,
                                self.save_sync_debounce_secs,
                                self.save_sync_session_id,
                                self.save_sync_gate.enablement(epoch),
                                self.sink.clone(),
                            ) {
                                Ok(agent) => self.save_sync_agent = Some(agent),
                                Err(error) => {
                                    self.save_sync_gate.close();
                                    selected_problem =
                                        Some(format!("Save watcher could not start: {error}"));
                                }
                            }
                        }
                    } else {
                        self.save_sync_gate.close();
                        selected_problem =
                            Some("No persistent save-sync directory is available.".into());
                    }
                }
            }
        } else {
            self.save_sync_gate.close();
        }

        if self.save_sync_agent.is_none() {
            if let (Some(scope), Some(report), Some(identity)) = (
                self.save_sync_scope.as_ref(),
                self.save_sync_mapping_report.as_ref(),
                self.save_sync_identity.as_ref(),
            ) {
                if let Err(error) =
                    self.emit_persisted_save_sync_review(scope, &report.mappings, identity)
                {
                    selected_problem =
                        Some(format!("Saved review state could not be read: {error}"));
                }
            }
        }

        if let Some(problem) = self.save_sync_runtime_problem.take() {
            selected_problem = Some(problem);
        }
        if let Some(settings_problem) = &self.save_sync_settings_problem {
            selected_problem = Some(match selected_problem {
                Some(problem) => format!("{problem} {settings_problem}"),
                None => settings_problem.clone(),
            });
        }
        self.sink.emit(AppEvent::SaveSyncUpdated {
            session_id: self.save_sync_session_id,
            server_id: self.server_id.clone(),
            candidates: discovery.candidates,
            skipped: discovery.skipped,
            selected_root,
            documented_saves_root,
            effective_saves_root,
            profile_version,
            account_id,
            mapped_targets,
            catalogue_unmapped,
            existing_saves,
            preview_saves,
            available,
            selected_problem,
            enabled: self.save_sync_enabled
                && self
                    .save_sync_gate
                    .enabled_for(self.current_save_sync_epoch),
            debounce_secs: self.save_sync_debounce_secs,
        });
    }

    fn select_save_sync_installation(&mut self, path: &Path) {
        self.stop_save_sync_agent();
        self.save_sync_enabled = false;
        self.save_sync_scope = None;
        // Explicit selection is remembered even while missing/invalid, so
        // refresh never changes to a different install on the user's behalf.
        if let Some(settings) = &self.save_sync_settings {
            if let Err(error) = settings.save_selected_installation(path) {
                self.save_sync_settings_problem = Some(format!("Selection was not saved: {error}"));
                self.log(
                    Level::Error,
                    "save-sync",
                    format!("Selection was not saved: {error}"),
                );
                self.refresh_save_sync();
                return;
            }
            self.save_sync_settings_problem = None;
        } else {
            self.save_sync_settings_problem = Some(
                "Selection cannot be persisted without the application settings database.".into(),
            );
        }
        self.save_sync_selected_root = Some(path.to_path_buf());
        self.refresh_save_sync();
    }

    fn set_save_sync_enabled(&mut self, enabled: bool) {
        if enabled {
            let ready = self.save_sync_scope.is_some()
                && self.save_sync_profile.is_some()
                && self.save_sync_identity.as_ref().is_some_and(|identity| {
                    identity.can_read_saves() && identity.can_write_saves()
                })
                && self
                    .save_sync_mapping_report
                    .as_ref()
                    .is_some_and(|report| report.supported_count() > 0);
            if !ready {
                self.save_sync_runtime_problem = Some(
                    "Enable refused: account, RetroBat 8.2.1 profile, and visible save mapping are not verified.".into(),
                );
                self.refresh_save_sync();
                return;
            }
            #[cfg(windows)]
            if let Some(scope) = &self.save_sync_scope {
                if let Err(error) =
                    crate::save_sync_agent::check_save_root_writable(&scope.effective_saves_root)
                {
                    self.save_sync_runtime_problem = Some(format!("Enable refused: {error}"));
                    self.refresh_save_sync();
                    return;
                }
            }
        }
        if let (Some(settings), Some(scope)) = (
            self.save_sync_settings.as_ref(),
            self.save_sync_scope.as_ref(),
        ) {
            let settings_value = ConsentSettings {
                enabled,
                debounce_secs: self.save_sync_debounce_secs,
            };
            if let Err(error) = settings.save(scope, settings_value) {
                self.save_sync_runtime_problem =
                    Some(format!("Consent could not be saved: {error}"));
                self.refresh_save_sync();
                return;
            }
        } else if enabled {
            self.save_sync_runtime_problem = Some("Consent scope is unavailable.".into());
            self.refresh_save_sync();
            return;
        }
        self.save_sync_enabled = enabled;
        self.refresh_save_sync();
    }

    fn set_save_sync_debounce(&mut self, seconds: u32) {
        if !(1..=3600).contains(&seconds) {
            self.save_sync_runtime_problem =
                Some("Debounce must be a whole number from 1 to 3600 seconds.".into());
            self.refresh_save_sync();
            return;
        }
        self.save_sync_debounce_secs = seconds;
        if let (Some(settings), Some(scope)) = (
            self.save_sync_settings.as_ref(),
            self.save_sync_scope.as_ref(),
        ) {
            if let Err(error) = settings.save(
                scope,
                ConsentSettings {
                    enabled: self.save_sync_enabled,
                    debounce_secs: seconds,
                },
            ) {
                self.save_sync_runtime_problem =
                    Some(format!("Debounce could not be saved: {error}"));
            }
        }
        self.refresh_save_sync();
    }

    fn export_save_sync_incoming(
        &mut self,
        session_id: u64,
        incoming_id: &str,
        destination: &Path,
    ) {
        if session_id != self.save_sync_session_id {
            return;
        }
        let result = (|| {
            if let Some(agent) = self.save_sync_agent.as_ref() {
                return agent.export_incoming(incoming_id, destination.to_path_buf());
            }
            let scope = self.save_sync_scope.as_ref().ok_or_else(|| {
                Error::Unsupported("save-sync scope is unavailable for export".into())
            })?;
            let mappings = &self
                .save_sync_mapping_report
                .as_ref()
                .ok_or_else(|| {
                    Error::Unsupported("save mappings are unavailable for export".into())
                })?
                .mappings;
            let identity = self.save_sync_identity.as_ref().ok_or_else(|| {
                Error::Unsupported("verified account identity is unavailable for export".into())
            })?;
            let settings_dir = self.save_sync_storage_dir.clone().ok_or_else(|| {
                Error::Unsupported("persistent save-sync directory is unavailable".into())
            })?;
            crate::save_sync_agent::export_pending_incoming(
                scope,
                mappings,
                identity,
                &settings_dir.join("save-sync-journal.db"),
                &settings_dir.join("save-sync-snapshots"),
                incoming_id,
                destination,
            )
        })();
        let (destination, error) = match result {
            Ok(()) => (Some(destination.display().to_string()), None),
            Err(error) => (None, Some(error.to_string())),
        };
        if let Some(error) = error.as_ref() {
            self.log(
                Level::Error,
                "save-sync",
                format!("incoming save export failed: {error}"),
            );
        }
        self.sink.emit(AppEvent::SaveSyncExportFinished {
            session_id,
            incoming_id: incoming_id.to_owned(),
            destination,
            error,
        });
    }

    fn advance_save_sync_session(&mut self) {
        self.save_sync_session_id = self.save_sync_session_id.saturating_add(1);
        self.sink.emit(AppEvent::SaveSyncSessionChanged {
            session_id: self.save_sync_session_id,
        });
    }

    fn emit_persisted_save_sync_review(
        &self,
        scope: &SaveSyncScope,
        mappings: &[rommfs_core::save_sync::SaveMapping],
        _identity: &rommfs_core::romm::SaveSyncIdentity,
    ) -> Result<()> {
        let Some(settings_dir) = &self.save_sync_storage_dir else {
            return Ok(());
        };
        let journal_path = settings_dir.join("save-sync-journal.db");
        if !journal_path.is_file() {
            return Ok(());
        }
        let journal = SaveSyncJournal::open(
            &journal_path,
            settings_dir.join("save-sync-snapshots"),
            scope.clone(),
        )?;
        let incoming_records = journal.incoming_saves()?;
        let incoming = incoming_records
            .iter()
            .map(|record| rommfs_core::events::SaveSyncIncomingStatus {
                incoming_id: record.id.clone(),
                rom_id: record.rom_key.rom_id,
                rom_name: mappings
                    .iter()
                    .find(|mapping| mapping.rom_key == record.rom_key)
                    .map(|mapping| mapping.visible_rom_name.clone())
                    .unwrap_or_else(|| format!("ROM {}", record.rom_key.rom_id)),
                remote_id: record.remote_id.clone(),
                content_hash: record.content_hash.clone(),
                reason: record.reason.clone(),
                state: record.state.clone(),
            })
            .collect::<Vec<_>>();
        let mut games = Vec::new();
        let mut attention_games = 0;
        let mut failure = None;
        for mapping in mappings {
            let Some(slot) = journal.slot(&mapping.rom_key)? else {
                continue;
            };
            if slot.needs_attention {
                attention_games += 1;
            }
            if failure.is_none() {
                failure = slot.last_failure.clone();
            }
            games.push(SaveSyncGameStatus {
                rom_id: mapping.rom_key.rom_id,
                rom_name: mapping.visible_rom_name.clone(),
                local_hash: slot.current_local_hash,
                remote_id: slot.remote_slot_id,
                remote_hash: slot.remote_baseline_hash,
                incoming_ids: incoming_records
                    .iter()
                    .filter(|record| record.rom_key == mapping.rom_key)
                    .map(|record| record.id.clone())
                    .collect(),
                issue: slot.last_failure.or_else(|| {
                    slot.local_removed.then(|| {
                        "the tracked local save was removed; the remote copy was preserved".into()
                    })
                }),
                installed_incoming: false,
            });
        }
        let pending_outbound = journal
            .snapshots()?
            .iter()
            .filter(|snapshot| snapshot.state != SnapshotState::RemoteComplete)
            .count()
            + journal.dirty_mappings()?.len();
        self.sink
            .emit(AppEvent::SaveSyncQueueUpdated(SaveSyncQueueStatus {
                session_id: self.save_sync_session_id,
                mapped_games: mappings.len(),
                // Persisted baselines are useful review context, but they are
                // not a fresh inventory verification for this session.
                reconciled_games: 0,
                pending_outbound,
                pending_incoming: incoming.len(),
                attention_games,
                network_paused: false,
                authentication_required: false,
                actor_failed: false,
                failure,
                games,
                incoming,
            }));
        Ok(())
    }

    fn stop_save_sync_agent(&mut self) {
        self.save_sync_gate.close();
        if let Some(mut agent) = self.save_sync_agent.take() {
            agent.stop();
        }
    }

    fn log(&self, level: Level, op: &'static str, message: impl Into<String>) {
        self.sink.emit(AppEvent::log(level, op, message));
    }

    fn connect(&mut self, url: &str, username: &str, password: &str) {
        // Credentials are used to authenticate and then dropped from this
        // stack frame; they are never stored in logs/events (R1).
        self.advance_save_sync_session();
        self.sink.emit(AppEvent::Connecting);
        self.stop_save_sync_agent();
        if self.mount.is_some() {
            self.fail_connect(&Error::Unsupported(
                "stop the mount before connecting elsewhere".into(),
            ));
            self.refresh_save_sync();
            return;
        }

        // Starting a new connection replaces the previous session, even when
        // URL validation or authentication fails. Keep the mounted-session
        // rejection above intact because that mount still owns its client.
        self.client = None;
        self.server_id = None;
        self.catalogue = None;
        self.save_sync_catalogue = None;
        self.save_sync_identity = None;
        self.save_sync_scope = None;
        self.save_sync_profile = None;
        self.save_sync_mapping_report = None;
        self.save_sync_enabled = false;
        self.names.clear();
        self.fs = None;

        let client = match RommClient::new(url) {
            Ok(c) => Arc::new(c),
            Err(e) => {
                self.fail_connect(&e);
                self.refresh_save_sync();
                return;
            }
        };
        let save_sync_identity =
            match client.authenticate_for_save_sync(Credentials { username, password }) {
                Ok(identity) => identity,
                Err(error) => {
                    self.fail_connect(&error);
                    self.refresh_save_sync();
                    return;
                }
            };

        self.client = Some(Arc::clone(&client));
        self.server_id = Some(server_id_of(url));
        self.save_sync_identity = save_sync_identity;
        self.sink.emit(AppEvent::Connected);
        self.log(Level::Info, "connect", format!("connected as {username}"));

        // The catalogue primes the UI (counts) but a failure here must not
        // fake an empty library — it surfaces as CatalogueFailed (R1).
        match self.load_catalogue(&client) {
            Ok(cat) => {
                self.store_catalogue(cat);
                self.refresh_save_sync();
            }
            Err(e) => {
                if e.needs_sign_in() {
                    self.sink.emit(AppEvent::SignInRequired);
                } else {
                    self.sink.emit(AppEvent::CatalogueFailed {
                        reason: e.to_string(),
                    });
                }
                self.log(Level::Error, "catalogue", e.to_string());
                self.refresh_save_sync();
            }
        }
    }

    /// Fetch platforms + all ROM pages and build the catalogue.
    /// `stop/start reloads it` (R1) — called on Connect and on every
    /// StartMount so the mounted snapshot is fresh.
    fn load_catalogue(&mut self, client: &RommClient) -> Result<Catalogue> {
        self.sink.emit(AppEvent::CatalogueLoading);
        let platforms = client.platforms()?;
        let roms = client.all_roms()?;
        let server_id = self.server_id.clone().unwrap_or_default();
        let sink = self.sink.clone();
        let cat = build_catalogue(&server_id, &platforms, &roms, move |warning| {
            sink.emit(AppEvent::log(Level::Warn, "catalogue", warning));
        })?;
        self.sink.emit(AppEvent::CatalogueLoaded {
            platforms: cat.platforms.len(),
            roms: cat.entries.len(),
            skipped_unsupported: cat.skipped_unsupported,
        });
        self.log(
            Level::Info,
            "catalogue",
            format!(
                "catalogue loaded: {} platforms, {} roms, {} skipped",
                cat.platforms.len(),
                cat.entries.len(),
                cat.skipped_unsupported
            ),
        );
        Ok(cat)
    }

    fn store_catalogue(&mut self, cat: Catalogue) {
        self.save_sync_catalogue = Some(cat.clone());
        self.names = cat
            .entries
            .iter()
            .map(|e| (e.key.clone(), e.file_name.clone()))
            .collect();
        self.catalogue = Some(cat);
    }

    fn fail_connect(&self, e: &Error) {
        if e.needs_sign_in() {
            self.sink.emit(AppEvent::SignInRequired);
        } else {
            self.sink.emit(AppEvent::ConnectFailed {
                reason: e.to_string(),
            });
        }
        self.log(Level::Error, "connect", e.to_string());
    }

    fn fail_mount(&self, stage: &str, path: &str, e: &Error) {
        let reason = format!("{stage} for mount {path:?} failed: {e}");
        self.sink.emit(AppEvent::MountFailed {
            reason: reason.clone(),
        });
        self.log(Level::Error, "mount", reason);
    }

    fn start_mount(&mut self, path: &str) {
        let path = path.trim();
        if self.mount.is_some() {
            // Duplicate starts can already be queued when the UI receives
            // MountStarting. Keep the real mounted state authoritative.
            if self.save_sync_enabled
                && !self
                    .save_sync_gate
                    .enabled_for(self.current_save_sync_epoch)
            {
                self.refresh_save_sync();
            }
            return;
        }
        let Some(client) = self.client.clone() else {
            self.fail_mount(
                "check connection",
                path,
                &Error::Auth("connect to a RomM server before mounting".into()),
            );
            self.refresh_save_sync();
            return;
        };
        let Some(server_id) = self.server_id.clone() else {
            self.fail_mount(
                "check connection",
                path,
                &Error::Auth("connect to a RomM server before mounting".into()),
            );
            self.refresh_save_sync();
            return;
        };

        self.sink.emit(AppEvent::MountStarting {
            path: path.to_string(),
        });
        let root = PathBuf::from(path);
        let started = Instant::now();
        self.log(
            Level::Info,
            "mount",
            format!("checking and preparing mount directory {path:?}"),
        );

        // Validate the root BEFORE claiming it: only empty dirs or roots we
        // previously marked for THIS server (PRD §5 — never over an existing
        // ROM library, never recursively cleared).
        if let Err(e) = check_mount_root(&root, &server_id) {
            self.fail_mount("prepare and validate directory", path, &e);
            self.refresh_save_sync();
            return;
        }

        // Mounts re-read the catalogue so stop/start never serves stale data.
        self.log(
            Level::Info,
            "mount",
            format!("loading catalogue for {path:?}"),
        );
        match self.load_catalogue(&client) {
            Ok(cat) => {
                self.store_catalogue(cat);
                self.refresh_save_sync();
            }
            Err(e) if e.needs_sign_in() => {
                self.sink.emit(AppEvent::SignInRequired);
                self.fail_mount("load catalogue", path, &e);
                self.refresh_save_sync();
                return;
            }
            Err(e) => {
                self.sink.emit(AppEvent::CatalogueFailed {
                    reason: e.to_string(),
                });
                self.fail_mount("load catalogue", path, &e);
                self.refresh_save_sync();
                return;
            }
        }

        self.log(
            Level::Info,
            "mount",
            format!("claiming ownership of {path:?}"),
        );
        if let Err(e) = claim_mount_root(&root, &server_id) {
            self.fail_mount("claim ownership", path, &e);
            return;
        }

        let remover = hydrated_remover(&root);
        self.log(
            Level::Info,
            "mount",
            format!(
                "opening private cache {}",
                cache_dir_for(&server_id).display()
            ),
        );
        let fs = match self.build_fs(&client, &root, remover) {
            Ok(fs) => fs,
            Err(e) => {
                self.fail_mount("open cache and build filesystem", path, &e);
                return;
            }
        };

        self.log(
            Level::Info,
            "mount",
            format!("starting Windows Cloud Files mount at {path:?}"),
        );
        match start_mount_backend(Arc::clone(&fs), &root) {
            Ok(mount) => {
                let mounted_path = mount.root.display().to_string();
                self.mount = Some(mount);
                self.fs = Some(fs);
                self.sink
                    .emit(AppEvent::MountStarted { path: mounted_path });
                self.log(
                    Level::Info,
                    "mount",
                    format!(
                        "mounted at {path} in {:.2}s",
                        started.elapsed().as_secs_f64()
                    ),
                );
                // One immediate sweep so expired entries from previous runs
                // are reclaimed (PRD R4); per-ROM outcomes become events.
                self.evict_once();
            }
            Err(e) => self.fail_mount("start filesystem backend", path, &e),
        }
    }

    /// Assemble the portable core objects the platform adapter mounts.
    /// CFAPI eviction dehydrates the NTFS copy before removing private bytes.
    fn build_fs(
        &mut self,
        client: &Arc<RommClient>,
        _root: &Path,
        remover: Arc<dyn HydratedRemover>,
    ) -> Result<Arc<RommFs>> {
        let server_id = self.server_id.clone().unwrap_or_default();
        let index = CacheIndex::open(cache_dir_for(&server_id))?;
        let live = Arc::new(LiveState::default());
        let source = Arc::new(ClientSource {
            client: Arc::clone(client),
            names: self.names.clone(),
        });
        let cat = self.catalogue.take();
        let cat = match cat {
            Some(c) => c,
            None => return Err(Error::InvalidCatalogue("no catalogue loaded".into())),
        };
        let expected: HashMap<RomKey, u64> = cat
            .entries
            .iter()
            .map(|e| (e.key.clone(), e.size))
            .collect();
        let versions: HashMap<RomKey, Option<String>> = cat
            .entries
            .iter()
            .map(|e| (e.key.clone(), e.version.as_ref().map(|v| v.0.clone())))
            .collect();
        let downloads = Arc::new(DownloadManager::new(
            index,
            Arc::clone(&live),
            source,
            self.sink.clone(),
            expected,
            versions,
        ));
        let evictor = Evictor::new(DEFAULT_EVICTION_THRESHOLD_SECS, live, remover);
        let tree = RommTree::new(cat);
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        Ok(Arc::new(RommFs::new(tree, downloads, evictor, clock)))
    }

    fn evict_once(&mut self) {
        let Some(fs) = self.fs.clone() else { return };
        match fs.evict_stale() {
            Ok(outcome) => {
                for key in outcome.evicted {
                    let name = self
                        .names
                        .get(&key)
                        .cloned()
                        .unwrap_or_else(|| key.cache_stem());
                    self.sink.emit(AppEvent::Evicted {
                        rom_id: key.rom_id.max(0) as u64,
                        file_name: name,
                    });
                }
                for key in outcome.deferred_failed {
                    self.sink.emit(AppEvent::EvictFailed {
                        rom_id: key.rom_id.max(0) as u64,
                        reason: "cleanup deferred (busy or removal failed)".into(),
                    });
                }
                if !outcome.retained_active.is_empty() {
                    self.log(
                        Level::Info,
                        "evict",
                        format!(
                            "eviction retained {} in-use entr{}",
                            outcome.retained_active.len(),
                            if outcome.retained_active.len() == 1 {
                                "y"
                            } else {
                                "ies"
                            }
                        ),
                    );
                }
            }
            Err(e) => {
                self.log(Level::Error, "evict", e.to_string());
            }
        }
    }

    fn stop_mount(&mut self) {
        self.sink.emit(AppEvent::MountStopping);
        if let Some(mount) = self.mount.take() {
            mount.stop();
            self.log(Level::Info, "mount", "mount stopped".to_string());
        }
        self.fs = None;
        self.sink.emit(AppEvent::MountStopped);
    }
}

fn worker_loop(
    rx: mpsc::Receiver<CommandEnvelope>,
    sink: EventSink,
    save_sync_gate: SaveSyncCommandGate,
) {
    worker_loop_with_interval_from(
        rx,
        Worker::with_persistent_save_sync_settings(sink, save_sync_gate),
        EVICTION_INTERVAL,
    );
}

fn worker_loop_with_interval_from(
    rx: mpsc::Receiver<CommandEnvelope>,
    mut worker: Worker,
    eviction_interval: Duration,
) {
    let mut next_eviction = None;
    loop {
        let received = if worker.mount.is_some() {
            let deadline = *next_eviction.get_or_insert_with(|| Instant::now() + eviction_interval);
            if Instant::now() >= deadline {
                worker.evict_once();
                next_eviction = Some(Instant::now() + eviction_interval);
                continue;
            }
            rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        } else {
            next_eviction = None;
            rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
        };
        let envelope = match received {
            Ok(envelope) => envelope,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                worker.evict_once();
                next_eviction = Some(Instant::now() + eviction_interval);
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let CommandEnvelope {
            command: cmd,
            save_sync_epoch,
        } = envelope;
        worker.current_save_sync_epoch = save_sync_epoch;
        let shutdown = matches!(cmd, Command::Shutdown);
        // Commands run one at a time on the worker thread; UI stays
        // responsive while network/filesystem work blocks here.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match &cmd {
            Command::Connect {
                url,
                username,
                password,
            } => worker.connect(url, username, password),
            Command::StartMount { path } => worker.start_mount(path),
            Command::StopMount => worker.stop_mount(),
            Command::RefreshSaveSync => worker.refresh_save_sync(),
            Command::SelectSaveSyncInstallation { path } => {
                worker.select_save_sync_installation(path)
            }
            Command::SetSaveSyncEnabled { enabled } => worker.set_save_sync_enabled(*enabled),
            Command::SetSaveSyncDebounce { seconds } => worker.set_save_sync_debounce(*seconds),
            Command::ExportSaveSyncIncoming {
                session_id,
                incoming_id,
                destination,
            } => worker.export_save_sync_incoming(*session_id, incoming_id, destination),
            Command::Shutdown => {
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
                Command::Connect { .. } => worker.sink.emit(AppEvent::ConnectFailed {
                    reason: format!("internal error: {msg}"),
                }),
                Command::StartMount { .. } => worker.sink.emit(AppEvent::MountFailed {
                    reason: format!("internal error: {msg}"),
                }),
                Command::StopMount => {}
                Command::RefreshSaveSync
                | Command::SelectSaveSyncInstallation { .. }
                | Command::SetSaveSyncEnabled { .. }
                | Command::SetSaveSyncDebounce { .. }
                | Command::ExportSaveSyncIncoming { .. } => {}
                Command::Shutdown => {}
            }
        }
        if shutdown {
            break;
        }
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
            .download_file(key.rom_id, name, writer, progress)
    }
}

// ---------------------------------------------------------------------------
// Mount root + backend seam.
// ---------------------------------------------------------------------------

fn check_mount_root(root: &Path, server_id: &str) -> Result<()> {
    let parent = root
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| {
            Error::Unsupported("choose a mount directory below an existing parent directory".into())
        })?;
    let _guard = rommfs_core::save_sync::hold_save_directory_chain(parent).map_err(|e| {
        Error::Unsupported(format!("protect mount parent {}: {e}", parent.display()))
    })?;
    match std::fs::create_dir(root) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => {
            return Err(Error::Unsupported(format!(
                "cannot create mount directory {}: {e}",
                root.display()
            )))
        }
    }
    #[cfg(windows)]
    rommfs_cfapi::check_prerequisites(root).map_err(|e| Error::Unsupported(format!("{e:#}")))?;
    rommfs_cfapi::check_mount_root(root, server_id)
        .map(|_| ())
        .map_err(|e| Error::Unsupported(format!("{e:#}")))
}

fn claim_mount_root(root: &Path, server_id: &str) -> Result<()> {
    rommfs_cfapi::claim_mount_root(root, server_id)
        .map_err(|e| Error::Unsupported(format!("{e:#}")))
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
    let mount = rommfs_cfapi::WindowsMount::mount(fs, root)
        .map_err(|e| Error::Unsupported(format!("{e:#}")))?;
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
        crate::save_sync_native::discovery_input()
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
mod tests {
    use super::*;
    use rommfs_core::cache::{CacheIndex, FakeClock, LiveState, NoopHydratedRemover};
    use rommfs_core::download::ContentSource;
    use rommfs_core::romm::{PlatformDto, RomDto, RomFileDto, SaveSyncIdentity};
    use rommfs_core::save_sync::RETROBAT_GB_SRM_PROFILE;
    use rommfs_fixture::{FixtureBodyBarrier, FixtureSaveRecord, FixtureServer, ResponseSpec};
    use std::io::Write;
    use std::time::Duration;

    #[test]
    fn mount_root_is_created_and_existing_user_files_are_preserved() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("roms");
        check_mount_root(&root, "server").unwrap();
        assert!(root.is_dir());
        check_mount_root(&root, "server").unwrap();
        std::fs::write(root.join("save.srm"), b"user data").unwrap();
        assert!(check_mount_root(&root, "server").is_err());
        assert_eq!(std::fs::read(root.join("save.srm")).unwrap(), b"user data");
    }

    #[test]
    fn failed_mount_logs_its_stage_and_path_before_and_after_validation() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("roms");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("save.srm"), b"user data").unwrap();
        let (sink, events) = rommfs_core::events::channel();
        let mut worker = Worker::new(sink);
        worker.client = Some(Arc::new(RommClient::new("http://unused.invalid").unwrap()));
        worker.server_id = Some("server".into());
        worker.start_mount(root.to_str().unwrap());
        let events: Vec<_> = events.try_iter().collect();
        let checking = events
            .iter()
            .position(|event| {
                matches!(event,
            AppEvent::Log(line) if line.message.contains("checking and preparing")
                && line.message.contains("roms"))
            })
            .unwrap();
        let failed = events
            .iter()
            .position(|event| {
                matches!(event,
            AppEvent::MountFailed { reason } if reason.contains("prepare and validate directory")
                && reason.contains("roms") && reason.contains("not empty"))
            })
            .unwrap();
        assert!(checking < failed);
        assert!(events.iter().any(|event| matches!(event,
            AppEvent::Log(line) if line.level == Level::Error
                && line.message.contains("prepare and validate directory")
                && line.message.contains("not empty"))));
        assert_eq!(std::fs::read(root.join("save.srm")).unwrap(), b"user data");
    }

    #[cfg(unix)]
    #[test]
    fn mount_root_creation_rejects_linked_ancestors() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        let link = tmp.path().join("link");
        std::fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(check_mount_root(&link.join("roms"), "server").is_err());
        assert!(!target.join("roms").exists());
    }

    struct UnusedSource;

    impl ContentSource for UnusedSource {
        fn fetch(
            &self,
            _key: &RomKey,
            _writer: &mut dyn Write,
            _progress: &mut dyn FnMut(u64, Option<u64>),
        ) -> Result<u64> {
            unreachable!("the eviction test never reads ROM content")
        }
    }

    fn create_test_retrobat_install(root: &Path, create_saves: bool) {
        let launcher_home = root.join("emulationstation/.emulationstation");
        std::fs::create_dir_all(root.join("system")).unwrap();
        std::fs::create_dir_all(root.join("emulators/retroarch")).unwrap();
        std::fs::create_dir_all(&launcher_home).unwrap();
        if create_saves {
            std::fs::create_dir_all(root.join("saves")).unwrap();
        }
        std::fs::write(root.join("RetroBat.exe"), b"exe").unwrap();
        std::fs::write(root.join("system/version.info"), "8.2.1\n").unwrap();
        std::fs::write(
            root.join("emulators/retroarch/retroarch.cfg"),
            "savefile_directory = \":\\saves\"\nsavefiles_in_content_dir = \"false\"\nsort_savefiles_enable = \"false\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("emulationstation/emulatorLauncher.cfg"),
            "home=.\\.emulationstation\nsaves=.\\..\\saves\n",
        )
        .unwrap();
        std::fs::write(launcher_home.join("es_settings.cfg"), "<config/>\n").unwrap();
        std::fs::write(
            launcher_home.join("es_systems.cfg"),
            r#"<systemList><system><name>gb</name><command>"%HOME%\emulatorLauncher.exe" -gameinfo %GAMEINFOXML% %CONTROLLERSCONFIG% -system %SYSTEM% -emulator %EMULATOR% -core %CORE% -rom %ROM%</command><emulators><emulator name="libretro"><cores><core>gambatte</core></cores></emulator></emulators></system></systemList>"#,
        )
        .unwrap();
    }

    #[test]
    fn failed_reconnect_discards_previous_worker_session() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0; 2048];
            let _ = std::io::Read::read(&mut stream, &mut request).unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
        });
        let (sink, events) = rommfs_core::events::channel();
        let mut worker = Worker::new(sink);
        worker.client = Some(Arc::new(
            RommClient::new("http://previous.invalid").unwrap(),
        ));
        worker.server_id = Some("http://previous.invalid".into());
        worker.names.insert(
            RomKey {
                server_id: "http://previous.invalid".into(),
                rom_id: 1,
                file_id: 2,
            },
            "old.nes".into(),
        );

        worker.connect(&format!("http://{address}"), "user", "password");
        server.join().unwrap();

        assert!(worker.client.is_none());
        assert!(worker.server_id.is_none());
        assert!(worker.catalogue.is_none());
        assert!(worker.names.is_empty());
        assert!(matches!(
            events.try_recv(),
            Ok(AppEvent::SaveSyncSessionChanged { session_id: 1 })
        ));
        assert!(matches!(events.try_recv(), Ok(AppEvent::Connecting)));
        assert!(matches!(events.try_recv(), Ok(AppEvent::SignInRequired)));
    }

    #[test]
    fn duplicate_mount_start_preserves_the_active_mount() {
        let (sink, events) = rommfs_core::events::channel();
        let mut worker = Worker::new(sink);
        worker.mount = Some(ActiveMount {
            root: PathBuf::from("active-root"),
            stop_fn: Box::new(|| {}),
        });

        worker.start_mount("another-root");

        assert_eq!(
            worker.mount.as_ref().unwrap().root,
            PathBuf::from("active-root")
        );
        assert!(
            events.try_recv().is_err(),
            "duplicate start must not emit failure"
        );
    }

    #[test]
    fn missing_previous_save_sync_selection_stays_selected_and_paused() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("previous-retrobat");
        let newly_found = dir.path().join("new-install");
        std::fs::create_dir_all(newly_found.join("emulationstation/config")).unwrap();
        std::fs::create_dir_all(newly_found.join("emulators/retroarch")).unwrap();
        std::fs::create_dir(newly_found.join("saves")).unwrap();
        std::fs::write(newly_found.join("RetroBat.exe"), b"exe").unwrap();
        std::fs::write(
            newly_found.join("emulationstation/config/es_systems.cfg"),
            "<systemList/>",
        )
        .unwrap();
        std::fs::write(
            newly_found.join("emulators/retroarch/retroarch.cfg"),
            "# config",
        )
        .unwrap();
        let (sink, events) = rommfs_core::events::channel();
        let mut worker = Worker::new(sink);
        worker.save_sync_selected_root = Some(missing.clone());

        worker.refresh_save_sync_from(DiscoveryInput {
            drive_roots: vec![(
                newly_found.clone(),
                rommfs_core::save_sync::InstallationSource::FixedDrive,
            )],
            process_images: Vec::new(),
        });

        assert!(matches!(
            events.try_recv(),
            Ok(AppEvent::SaveSyncSessionChanged { session_id: 1 })
        ));
        let event = events.try_recv().unwrap();
        match event {
            AppEvent::SaveSyncUpdated {
                candidates,
                selected_root,
                selected_problem,
                ..
            } => {
                assert_eq!(candidates.len(), 1);
                assert_eq!(candidates[0].info.install_root, newly_found);
                assert_eq!(selected_root, Some(missing.display().to_string()));
                assert!(selected_problem
                    .unwrap()
                    .contains("previously selected RetroBat installation is missing"));
            }
            other => panic!("expected save-sync state event, got {other:?}"),
        }
    }

    #[test]
    fn manual_save_sync_selection_is_validated_and_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("chosen-retrobat");
        std::fs::create_dir_all(root.join("emulationstation/config")).unwrap();
        std::fs::create_dir_all(root.join("emulators/retroarch")).unwrap();
        std::fs::create_dir(root.join("saves")).unwrap();
        std::fs::write(root.join("RetroBat.exe"), b"exe").unwrap();
        std::fs::write(
            root.join("emulationstation/config/es_systems.cfg"),
            "<systemList/>",
        )
        .unwrap();
        std::fs::write(root.join("emulators/retroarch/retroarch.cfg"), "# config").unwrap();

        let (sink, events) = rommfs_core::events::channel();
        let mut worker = Worker::new(sink);
        worker.save_sync_settings =
            Some(SaveSyncSettingsStore::open(dir.path().join("settings/save-sync.db")).unwrap());
        worker.save_sync_settings_problem =
            Some("Selection was not saved: transient failure".into());
        worker.select_save_sync_installation(&root);
        assert!(worker.save_sync_settings_problem.is_none());

        assert!(matches!(
            events.try_recv(),
            Ok(AppEvent::SaveSyncSessionChanged { session_id: 1 })
        ));
        let event = events.try_recv().unwrap();
        match event {
            AppEvent::SaveSyncUpdated {
                selected_root,
                documented_saves_root,
                effective_saves_root,
                enabled,
                debounce_secs,
                selected_problem,
                ..
            } => {
                assert_eq!(selected_root, Some(root.display().to_string()));
                assert_eq!(
                    documented_saves_root,
                    Some(root.join("saves").display().to_string())
                );
                assert!(!enabled);
                assert_eq!(debounce_secs, rommfs_core::save_sync::DEFAULT_DEBOUNCE_SECS);
                assert!(selected_problem.is_some());
                assert!(effective_saves_root.is_none());
            }
            other => panic!("expected save-sync state event, got {other:?}"),
        }
        assert_eq!(
            worker
                .save_sync_settings
                .as_ref()
                .unwrap()
                .selected_installation()
                .unwrap(),
            Some(root)
        );
    }

    #[test]
    fn save_sync_enable_request_is_refused_without_verified_account_and_save_root() {
        let (sink, events) = rommfs_core::events::channel();
        let mut worker = Worker::new(sink);

        worker.set_save_sync_enabled(true);

        let mut enabled = true;
        let mut debounce_secs = 0;
        let mut problem = None;
        for event in events.try_iter() {
            if let AppEvent::SaveSyncUpdated {
                enabled: event_enabled,
                debounce_secs: event_debounce,
                selected_problem,
                ..
            } = event
            {
                enabled = event_enabled;
                debounce_secs = event_debounce;
                problem = selected_problem;
            }
        }
        assert!(!enabled);
        assert_eq!(debounce_secs, rommfs_core::save_sync::DEFAULT_DEBOUNCE_SECS);
        assert!(problem.unwrap().contains(
            "account, RetroBat 8.2.1 profile, and visible save mapping are not verified"
        ));
    }

    #[test]
    fn stale_save_sync_status_and_transfer_events_are_rejected_after_scope_change() {
        let mut state = UiState::new(32);
        state.apply(&AppEvent::SaveSyncSessionChanged { session_id: 8 });
        state.apply(&AppEvent::SaveSyncAuthenticationRequired { session_id: 7 });
        assert_ne!(state.conn, ConnState::SignInRequired);
        state.apply(&AppEvent::SaveSyncQueueUpdated(SaveSyncQueueStatus {
            session_id: 7,
            mapped_games: 1,
            reconciled_games: 1,
            pending_outbound: 0,
            pending_incoming: 0,
            attention_games: 0,
            network_paused: false,
            authentication_required: false,
            actor_failed: false,
            failure: None,
            games: Vec::new(),
            incoming: Vec::new(),
        }));
        state.apply(&AppEvent::SaveSyncTransferProgress {
            session_id: 7,
            rom_id: 3,
            revision: "stale-revision".into(),
            phase: "verified".into(),
            detail: None,
        });
        assert!(state.save_sync_queue.is_none());
        assert!(state.save_sync_transfers.is_empty());

        state.apply(&AppEvent::SaveSyncAuthenticationRequired { session_id: 8 });
        assert!(state.save_sync_authentication_required);
        assert_eq!(state.conn, ConnState::SignInRequired);

        state.apply(&AppEvent::SaveSyncQueueUpdated(SaveSyncQueueStatus {
            session_id: 8,
            mapped_games: 1,
            reconciled_games: 0,
            pending_outbound: 2,
            pending_incoming: 1,
            attention_games: 1,
            network_paused: false,
            authentication_required: false,
            actor_failed: false,
            failure: Some("network retry".into()),
            games: Vec::new(),
            incoming: Vec::new(),
        }));
        let queue = state.save_sync_queue.as_ref().unwrap();
        assert_eq!(queue.pending_outbound, 2);
        assert_eq!(queue.reconciled_games, 0);
        assert_eq!(state.save_sync_pending_incoming, 1);
        assert_eq!(state.save_sync_attention_games, 1);
    }

    #[test]
    fn invalid_debounce_values_do_not_change_persisted_consent() {
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("save-sync.db");
        let scope = SaveSyncScope {
            server_id: "fixture-server".into(),
            account_id: "42".into(),
            installation_root: dir.path().join("RetroBat"),
            effective_saves_root: dir.path().join("RetroBat/saves"),
        };
        let settings = SaveSyncSettingsStore::open(&database).unwrap();
        settings
            .save(
                &scope,
                ConsentSettings {
                    enabled: false,
                    debounce_secs: 17,
                },
            )
            .unwrap();
        drop(settings);

        let (sink, events) = rommfs_core::events::channel();
        let mut worker = Worker::new(sink);
        worker.save_sync_settings = Some(SaveSyncSettingsStore::open(&database).unwrap());
        worker.save_sync_scope = Some(scope.clone());
        worker.save_sync_debounce_secs = 17;
        worker.set_save_sync_debounce(0);
        worker.set_save_sync_debounce(3601);

        assert_eq!(
            worker
                .save_sync_settings
                .as_ref()
                .unwrap()
                .load(&scope)
                .unwrap()
                .debounce_secs,
            17
        );
        assert!(events.try_iter().any(|event| matches!(
            event,
            AppEvent::SaveSyncUpdated {
                selected_problem: Some(problem),
                ..
            } if problem.contains("1 to 3600 seconds")
        )));
    }

    #[test]
    fn preview_reports_mapped_targets_separately_from_empty_existing_saves_before_consent() {
        let directory = tempfile::tempdir().unwrap();
        let install_root = directory.path().join("RetroBat");
        create_test_retrobat_install(&install_root, false);
        let settings_dir = directory.path().join("settings");
        let settings = SaveSyncSettingsStore::open(settings_dir.join("save-sync.db")).unwrap();
        let fixture = FixtureServer::start();
        let server_id = server_id_of(fixture.url());
        let client = Arc::new(RommClient::new(fixture.url()).unwrap());
        let rom = RomDto {
            id: 7,
            platform_fs_slug: "gb".into(),
            platform_slug: "gb".into(),
            fs_name: "Game.gb".into(),
            fs_size_bytes: 1,
            has_simple_single_file: true,
            has_nested_single_file: false,
            has_multiple_files: false,
            missing_from_fs: false,
            is_physical: false,
            updated_at: String::new(),
            files: vec![RomFileDto {
                id: 70,
                file_name: "Game.gb".into(),
                file_size_bytes: 1,
                last_modified: None,
                crc_hash: None,
                md5_hash: None,
                sha1_hash: None,
                is_top_level: true,
            }],
        };
        let catalogue = build_catalogue(
            &server_id,
            &[PlatformDto {
                id: 1,
                slug: "gb".into(),
                fs_slug: "gb".into(),
                name: "Game Boy".into(),
                custom_name: None,
                rom_count: 1,
            }],
            &[rom],
            |_| {},
        )
        .unwrap();
        let (sink, events) = rommfs_core::events::channel();
        let mut worker = Worker::new(sink);
        worker.save_sync_settings = Some(settings);
        worker.save_sync_storage_dir = Some(settings_dir);
        worker.save_sync_selected_root = Some(install_root.clone());
        worker.server_id = Some(server_id);
        worker.client = Some(client);
        worker.save_sync_identity = Some(SaveSyncIdentity {
            account_id: 42,
            scopes: vec!["assets.read".into(), "assets.write".into()],
        });
        worker.save_sync_catalogue = Some(catalogue);

        worker.refresh_save_sync_from(DiscoveryInput::default());

        let update = events
            .try_iter()
            .find_map(|event| match event {
                AppEvent::SaveSyncUpdated {
                    mapped_targets,
                    catalogue_unmapped,
                    existing_saves,
                    available,
                    enabled,
                    ..
                } => Some((
                    mapped_targets,
                    catalogue_unmapped,
                    existing_saves.unwrap(),
                    available,
                    enabled,
                )),
                _ => None,
            })
            .expect("worker preview update");
        assert_eq!(update.0, 1);
        assert_eq!(update.1, 0);
        assert!(
            update.3,
            "an absent local save must not block incoming sync"
        );
        assert!(!update.4, "consent remains off by default");
        assert_eq!(
            update.2.status,
            rommfs_core::save_sync::ExistingSaveScanStatus::Complete
        );
        assert_eq!(update.2.supported_files, 0);
        assert_eq!(update.2.skipped_files, 0);
        assert!(!install_root.join("saves").exists());
        assert!(
            fixture.requests().is_empty(),
            "preview is local-only before consent"
        );
    }

    #[test]
    fn disable_during_connect_prevents_the_older_scope_refresh_from_starting_sync() {
        let directory = tempfile::tempdir().unwrap();
        let install_root = directory.path().join("RetroBat");
        let save_root = install_root.join("saves");
        create_test_retrobat_install(&install_root, true);

        let fixture = FixtureServer::start();
        let account_barrier = FixtureBodyBarrier::new();
        let inventory_barrier = FixtureBodyBarrier::new();
        fixture.on(
            "POST",
            "/api/token",
            ResponseSpec::Json {
                status: 200,
                body: rommfs_fixture::contract::token_ok(),
            },
        );
        fixture.on(
            "GET",
            "/api/users/me",
            ResponseSpec::HeldBytes {
                status: 200,
                bytes: br#"{"id":42,"oauth_scopes":["me.read","assets.read","assets.write"]}"#
                    .to_vec(),
                barrier: account_barrier.clone(),
            },
        );
        fixture.on(
            "GET",
            "/api/platforms",
            ResponseSpec::Json {
                status: 200,
                body: rommfs_fixture::contract::platforms(&[(1, "gb", "gb", "Game Boy")]),
            },
        );
        let rom = rommfs_fixture::contract::rom(7, "gb", "Game.gb", 1, "abc");
        fixture.on(
            "GET",
            "/api/roms?",
            ResponseSpec::Json {
                status: 200,
                body: rommfs_fixture::contract::roms_page(&[rom], 1, 100, 0),
            },
        );
        fixture.use_romm_save_store(42, 201);
        fixture.seed_save(FixtureSaveRecord {
            id: 31,
            rom_id: 7,
            user_id: 42,
            file_name: "rommfs-550e8400-e29b-41d4-a716-446655440000 [2026-10-05_12-34-56].srm"
                .into(),
            file_size_bytes: 19,
            slot: RETROBAT_GB_SRM_PROFILE.into(),
            bytes: b"incoming save bytes".to_vec(),
            created_at: "2026-10-05T12:34:56Z".into(),
            updated_at: "2026-10-05T12:34:56Z".into(),
        });
        fixture.on(
            "GET",
            "/api/saves?",
            ResponseSpec::HeldBytes {
                status: 200,
                bytes: serde_json::json!([{
                    "id": 31,
                    "rom_id": 7,
                    "user_id": 42,
                    "file_name": "rommfs-550e8400-e29b-41d4-a716-446655440000 [2026-10-05_12-34-56].srm",
                    "file_size_bytes": 19,
                    "missing_from_fs": false,
                    "created_at": "2026-10-05T12:34:56Z",
                    "updated_at": "2026-10-05T12:34:56Z",
                    "emulator": "retroarch-gambatte",
                    "slot": RETROBAT_GB_SRM_PROFILE,
                }])
                .to_string()
                .into_bytes(),
                barrier: inventory_barrier.clone(),
            },
        );

        let settings_dir = directory.path().join("settings");
        let settings = SaveSyncSettingsStore::open(settings_dir.join("save-sync.db")).unwrap();
        let scope = SaveSyncScope {
            server_id: server_id_of(fixture.url()),
            account_id: "42".into(),
            installation_root: install_root.clone(),
            effective_saves_root: save_root,
        };
        settings
            .save(
                &scope,
                ConsentSettings {
                    enabled: true,
                    debounce_secs: 1,
                },
            )
            .unwrap();

        let (sink, events) = rommfs_core::events::channel();
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let gate = SaveSyncCommandGate::new();
        let worker_gate = gate.clone();
        let worker_sink = sink.clone();
        let worker_settings_dir = settings_dir.clone();
        let worker_install_root = install_root.clone();
        let worker = std::thread::spawn(move || {
            let mut worker = Worker::new_with_gate(worker_sink, worker_gate);
            worker.save_sync_settings = Some(settings);
            worker.save_sync_storage_dir = Some(worker_settings_dir);
            worker.save_sync_selected_root = Some(worker_install_root);
            worker_loop_with_interval_from(cmd_rx, worker, Duration::from_secs(3600));
        });
        let controller = Controller {
            sink,
            cmd_tx,
            worker: Some(worker),
            save_sync_gate: gate,
        };

        controller.send(Command::Connect {
            url: fixture.url().into(),
            username: "user".into(),
            password: "password".into(),
        });
        assert!(account_barrier.wait_until_blocked(Duration::from_secs(5)));
        controller.send(Command::SetSaveSyncEnabled { enabled: false });
        let disabled_epoch = controller.save_sync_gate.current_epoch();
        account_barrier.release();

        let inventory_started = inventory_barrier.wait_until_blocked(Duration::from_secs(3));
        if inventory_started {
            inventory_barrier.release();
        }
        let mut saw_disabled_status = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !saw_disabled_status && std::time::Instant::now() < deadline {
            if let Ok(AppEvent::SaveSyncUpdated {
                enabled,
                available,
                mapped_targets,
                account_id,
                ..
            }) = events.recv_timeout(Duration::from_millis(100))
            {
                assert!(!enabled, "an older connection refresh reopened save sync");
                assert!(available, "test account/profile was not save-sync ready");
                assert_eq!(mapped_targets, 1);
                assert_eq!(account_id, Some(42));
                saw_disabled_status = true;
            }
        }

        assert!(
            saw_disabled_status,
            "worker did not report the disabled scope"
        );
        assert!(
            !inventory_started,
            "the stale refresh started an inventory request"
        );
        assert!(!controller.save_sync_gate.enabled_for(disabled_epoch));
        assert_eq!(fixture.count_requests("POST", "/api/saves?"), 0);
        assert_eq!(fixture.count_requests("GET", "/api/saves/31/content"), 0);
        assert!(!install_root.join("saves/gb/Game.srm").exists());
    }

    #[test]
    fn worker_loop_sweeps_again_after_the_mount_start_sweep() {
        let dir = tempfile::tempdir().unwrap();
        let key = RomKey {
            server_id: "fixture".into(),
            rom_id: 12,
            file_id: 34,
        };
        let cache_dir = dir.path().to_path_buf();
        let bin = cache_dir.join(format!("{}.bin", key.cache_stem()));
        let worker_bin = bin.clone();
        let (sink, events) = rommfs_core::events::channel();
        let (cmd_tx, cmd_rx) = mpsc::channel::<CommandEnvelope>();
        let worker_thread = std::thread::spawn(move || {
            // Construct Worker inside its owning thread: ActiveMount may
            // contain a platform mount handle that is intentionally !Send.
            let mut index = CacheIndex::open(&cache_dir).unwrap();
            std::fs::write(&worker_bin, b"cached bytes").unwrap();
            index.mark_ready(&key, 12, None, "nes/game.nes").unwrap();
            index.touch(&key, 0).unwrap();

            let live = Arc::new(LiveState::default());
            let downloads = Arc::new(DownloadManager::new(
                index,
                Arc::clone(&live),
                Arc::new(UnusedSource),
                rommfs_core::events::channel().0,
                HashMap::new(),
                HashMap::new(),
            ));
            let catalogue = build_catalogue(
                "fixture",
                &[PlatformDto {
                    id: 1,
                    slug: "nes".into(),
                    fs_slug: "nes".into(),
                    name: "Nintendo".into(),
                    custom_name: None,
                    rom_count: 0,
                }],
                &[],
                |_| {},
            )
            .unwrap();
            let fs = Arc::new(RommFs::new(
                RommTree::new(catalogue),
                downloads,
                Evictor::new(1, live, Arc::new(NoopHydratedRemover)),
                Arc::new(FakeClock::new(100)),
            ));
            let worker = Worker {
                sink,
                client: None,
                server_id: None,
                catalogue: None,
                names: HashMap::from([(key, "game.nes".into())]),
                fs: Some(fs),
                mount: Some(ActiveMount {
                    root: cache_dir,
                    stop_fn: Box::new(|| {}),
                }),
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
                save_sync_gate: SaveSyncCommandGate::new(),
                current_save_sync_epoch: 0,
                save_sync_runtime_problem: None,
                save_sync_session_id: 0,
            };
            worker_loop_with_interval_from(cmd_rx, worker, Duration::from_millis(10));
        });

        let event = events.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(event, AppEvent::Evicted { rom_id: 12, .. }));
        assert!(
            !bin.exists(),
            "the periodic sweep removes stale cache bytes"
        );

        cmd_tx
            .send(CommandEnvelope {
                command: Command::Shutdown,
                save_sync_epoch: 0,
            })
            .unwrap();
        worker_thread.join().unwrap();
    }
}
