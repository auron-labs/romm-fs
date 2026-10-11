//! Headless app controller: the real state machine the GPUI window renders.
//! Drives workers via commands, consumes `AppEvent`s into `UiState`.
//! Tests exercise this path — no duplicate state machine in the UI (PRD §6).

use crate::save_sync_agent::{SaveSyncAgent, SaveSyncCommandGate};
use anyhow::Context as _;
use rommfs_core::cache::clock::{Clock, SystemClock, DEFAULT_EVICTION_THRESHOLD_SECS};
#[cfg(not(windows))]
use rommfs_core::cache::NoopHydratedRemover;
use rommfs_core::cache::{CacheIndex, Evictor, HydratedRemover, LiveState};
#[cfg(test)]
use rommfs_core::catalog::build_catalogue;
use rommfs_core::catalog::{server_id_of, Catalogue, RomKey};
use rommfs_core::catalogue_store::{CatalogueSnapshot, CatalogueStore};
use rommfs_core::connection_settings::{
    ConnectionPreferences, ConnectionSettings, IntegrationMode,
};
use rommfs_core::download::{ContentSource, DownloadManager};
use rommfs_core::error::{Error, Result};
use rommfs_core::events::{
    AppEvent, EventSink, ForeignEntry, ForeignKind, Level, LogBuffer, SaveSyncGameStatus,
    SaveSyncQueueStatus,
};
use rommfs_core::fscore::RommFs;
use rommfs_core::romm::{Credentials, PairedSession, PairingPoll, PairingRequest, RommClient};
use rommfs_core::save_sync::{
    discover_installations, map_catalogue, resolve_retrobat_gb_profile, validate_installation,
    ConsentSettings, DiscoveryInput, InstallationCandidate, InstallationProblem, MappingReport,
    RetroBatGbProfile, SaveSyncJournal, SaveSyncScope, SaveSyncSettingsStore, SnapshotState,
    UnmatchedFile,
};
use rommfs_core::tree::RommTree;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

mod auxiliary;
mod installer;
mod save_files;

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

/// The derived games-folder summary the whole UI shares (UI-PLAN §3): a folder
/// is "ready" only after a real MountStarted — never inferred from the
/// persisted enabled setting or the existence of a directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderSummary {
    /// No completed folder setup for this server yet.
    SetupNeeded,
    /// A folder is configured but the provider is stopped.
    Stopped,
    /// A start operation is running.
    Starting,
    /// Provider active and folder usable.
    Ready,
    /// The last start failed (reason in `mount_error`).
    Failed,
}

impl FolderSummary {
    pub fn label(self) -> &'static str {
        match self {
            Self::SetupNeeded => "Set up your games folder",
            Self::Stopped => "Games folder stopped",
            Self::Starting => "Starting games folder…",
            Self::Ready => "Games folder ready",
            Self::Failed => "Couldn't start the games folder",
        }
    }
}

/// The derived save-sync summary the whole UI shares (UI-PLAN §5): configured,
/// enabled, running, and successfully checked stay distinct — unknown state
/// and zero linked files are never presented as "up to date".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveSyncSummary {
    /// Signed out or the server rejected the session.
    AuthRequired,
    /// The RomM connection itself failed.
    Offline,
    /// The user has not enabled sync — no implied protection.
    Off,
    /// The source configuration is invalid or incomplete.
    NeedsSetup,
    /// Enabled but no completed check exists yet.
    Checking,
    /// A check ran but no eligible files are linked to games.
    NoLinkedFiles,
    /// Still reconciling known links.
    Reconciling,
    /// Transfers are pending or running.
    Syncing,
    /// The current operation failed or items need review.
    NeedsAttention,
    /// The worker paused syncing (for example on network errors).
    Paused,
    /// Linked files checked with no work pending. Unlinked files keep a
    /// separate count — they do not downgrade the summary.
    UpToDate,
}

impl SaveSyncSummary {
    pub fn label(self) -> &'static str {
        match self {
            Self::AuthRequired => "Sign-in required",
            Self::Offline => "RomM unreachable",
            Self::Off => "Save sync off",
            Self::NeedsSetup => "Save sync needs setup",
            Self::Checking => "Checking saves…",
            Self::NoLinkedFiles => "No files ready to sync",
            Self::Reconciling => "Checking saves…",
            Self::Syncing => "Syncing saves…",
            Self::NeedsAttention => "Save sync needs attention",
            Self::Paused => "Save sync paused",
            Self::UpToDate => "Linked saves are up to date",
        }
    }
}

#[derive(Clone, Debug)]
pub struct DownloadView {
    pub file_id: u64,
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
    pub auxiliary_running: bool,
    pub start_at_login: bool,
    pub startup_error: Option<String>,
    pub save_file_games: Vec<(i64, String)>,
    pub save_file_unmatched: Vec<UnmatchedFile>,
    pub auxiliary_detail: Option<String>,
    pub auxiliary_error: Option<String>,
    pub installer_running: bool,
    pub installer_ready: bool,
    pub installer_progress: (u64, Option<u64>),
    pub installer_detail: Option<String>,
    pub installer_error: Option<String>,
    pub integration: IntegrationMode,
    pub retrobat_root: Option<String>,
    pub integration_error: Option<String>,
    /// Confirmation/next-step shown after a layout change succeeds.
    pub integration_notice: Option<String>,
    pub retrobat_setup_running: bool,
    pub retrobat_setup_detail: Option<String>,
    pub retrobat_setup_error: Option<String>,
    pub frontend_metadata_pending: bool,
    pub frontend_metadata_detail: Option<String>,
    pub frontend_metadata_error: Option<String>,
    pub pairing_code: Option<String>,
    pub conn: ConnState,
    pub conn_error: Option<String>,
    pub mount: MountState,
    pub mount_path: Option<String>,
    pub mount_error: Option<String>,
    /// Classified foreign entries blocking a mount/setup, for remediation UI.
    pub foreign_root: Option<String>,
    pub foreign_entries: Vec<ForeignEntry>,
    /// Set after a "remove leftover links" pass ran.
    pub foreign_removal: Option<std::result::Result<usize, String>>,
    /// How many known stock/OS files and links the last RetroBat setup
    /// refusal reported as removable with consent.
    pub retrobat_setup_removable: usize,
    pub downloads: Vec<DownloadView>,
    pub catalogue_progress: Option<(String, u64, Option<u64>, u64)>,
    pub mount_progress: Option<(String, u64, Option<u64>, u64)>,
    pub catalogue_refreshed_at: Option<u64>,
    pub catalogue_pending_remount: bool,
    catalogue_operation_id: u64,
    pub catalogue: Option<(usize, usize, usize)>, // platforms, roms, skipped
    pub save_sync_candidates: Vec<InstallationCandidate>,
    pub save_sync_skipped: usize,
    pub save_sync_selected_root: Option<String>,
    pub save_sync_documented_saves_root: Option<String>,
    pub save_sync_effective_saves_root: Option<String>,
    pub save_sync_profile_version: Option<String>,
    /// Server the active session connected to (from ConnectionRestored).
    pub connected_server: Option<String>,
    /// The persisted games-folder root for this server, when known —
    /// distinguishes "never set up" from "configured but stopped".
    pub configured_mount_root: Option<String>,
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
            start_at_login: false,
            auxiliary_running: false,
            auxiliary_detail: None,
            auxiliary_error: None,
            startup_error: None,
            save_file_games: Vec::new(),
            save_file_unmatched: Vec::new(),
            installer_running: false,
            installer_ready: false,
            installer_progress: (0, None),
            installer_detail: None,
            installer_error: None,
            integration: IntegrationMode::Generic,
            retrobat_root: None,
            integration_error: None,
            integration_notice: None,
            retrobat_setup_running: false,
            retrobat_setup_detail: None,
            retrobat_setup_error: None,
            frontend_metadata_pending: false,
            frontend_metadata_detail: None,
            frontend_metadata_error: None,
            pairing_code: None,
            conn: ConnState::Idle,
            conn_error: None,
            mount: MountState::NotMounted,
            mount_path: None,
            mount_error: None,
            foreign_root: None,
            foreign_entries: Vec::new(),
            foreign_removal: None,
            retrobat_setup_removable: 0,
            downloads: Vec::new(),
            catalogue_progress: None,
            mount_progress: None,
            catalogue_refreshed_at: None,
            catalogue_pending_remount: false,
            catalogue_operation_id: 0,
            catalogue: None,
            connected_server: None,
            configured_mount_root: None,
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
            AppEvent::ForeignRootEntries {
                root,
                entries,
                removal,
            } => {
                self.foreign_root = Some(root.clone());
                self.foreign_entries = entries.clone();
                if removal.is_some() {
                    self.foreign_removal = removal.clone();
                }
            }
            AppEvent::AuxiliaryStatus {
                running,
                detail,
                error,
            } => {
                // Auxiliary support-file downloads are their own operation:
                // their status must not masquerade as (or overwrite) RetroBat
                // setup state.
                self.auxiliary_running = *running;
                self.auxiliary_detail = Some(detail.clone());
                self.auxiliary_error = error.clone();
            }
            AppEvent::StartupPreference { enabled, error } => {
                self.start_at_login = *enabled;
                self.startup_error = error.clone();
            }
            AppEvent::SaveFileMappings {
                session_id,
                games,
                unmatched,
            } => {
                if *session_id == self.save_sync_session_id {
                    self.save_file_games = games.clone();
                    self.save_file_unmatched = unmatched.clone();
                }
            }
            AppEvent::RetroBatInstaller {
                running,
                ready,
                received,
                total,
                detail,
                error,
            } => {
                self.installer_running = *running;
                self.installer_ready = *ready;
                self.installer_progress = (*received, *total);
                self.installer_detail = Some(detail.clone());
                self.installer_error = error.clone();
            }
            AppEvent::FrontendMetadata {
                pending,
                detail,
                error,
            } => {
                self.frontend_metadata_pending = *pending;
                self.frontend_metadata_detail = Some(detail.clone());
                self.frontend_metadata_error = error.clone();
            }
            AppEvent::RetroBatSetup {
                running,
                detail,
                error,
                removable_conflicts,
            } => {
                self.retrobat_setup_running = *running;
                self.retrobat_setup_detail = Some(detail.clone());
                self.retrobat_setup_error = error.clone();
                self.retrobat_setup_removable = *removable_conflicts;
                if *running {
                    self.foreign_root = None;
                    self.foreign_entries.clear();
                    self.foreign_removal = None;
                } else if error.is_none() {
                    // Setup finished — the "Next: run Set up RetroBat" hint
                    // from the layout change is done.
                    self.integration_notice = None;
                }
            }
            AppEvent::IntegrationUpdated {
                mode,
                retrobat_root,
                error,
            } => {
                let changed = self.integration != *mode;
                self.integration = *mode;
                self.retrobat_root = retrobat_root.clone();
                self.integration_error = error.clone();
                self.integration_notice = if error.is_none() && changed {
                    Some(match mode {
                        IntegrationMode::RetroBat => {
                            "RetroBat layout is active. Next: run \"Set up RetroBat\" below.".into()
                        }
                        IntegrationMode::Generic => "Generic layout is active.".into(),
                    })
                } else {
                    None
                };
            }
            AppEvent::ConnectionRestored {
                server, mount_root, ..
            } => {
                self.connected_server = Some(server.clone());
                if mount_root.is_some() {
                    self.configured_mount_root = mount_root.clone();
                }
            }
            AppEvent::SignedOut => {
                self.conn = ConnState::Idle;
                self.conn_error = None;
                self.catalogue = None;
                self.catalogue_refreshed_at = None;
                self.catalogue_pending_remount = false;
                self.catalogue_progress = None;
                self.downloads.clear();
                self.pairing_code = None;
                self.connected_server = None;
                self.configured_mount_root = None;
            }
            AppEvent::PairingReady { user_code, .. } => {
                self.pairing_code = Some(user_code.clone());
            }
            AppEvent::PairingEnded { reason } => {
                self.pairing_code = None;
                if let Some(reason) = reason {
                    self.conn = ConnState::Failed;
                    self.conn_error = Some(reason.clone());
                }
            }
            AppEvent::Connecting => {
                self.conn = ConnState::Connecting;
                self.conn_error = None;
                // A new connection attempt supersedes any prior session's
                // catalogue/downloads — never show stale data for a server
                // we may no longer be talking to.
                self.catalogue = None;
                self.catalogue_progress = None;
                self.catalogue_refreshed_at = None;
                self.catalogue_pending_remount = false;
                self.downloads.clear();
                self.save_sync_games.clear();
                self.save_sync_reconciled_games = 0;
                self.save_sync_pending_incoming = 0;
                self.save_sync_attention_games = 0;
                self.save_sync_failure = None;
                self.save_sync_transfers.clear();
                self.save_sync_queue = None;
                self.save_sync_authentication_required = false;
                // A new connection attempt may target a different server —
                // the old session's host and folder config are not evidence
                // of anything until ConnectionRestored confirms them.
                self.connected_server = None;
                self.configured_mount_root = None;
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
                // A failed mount attempt belongs to the previous connection;
                // keep it from rendering as a stale banner after reconnect.
                self.mount = MountState::NotMounted;
                self.mount_error = None;
            }
            AppEvent::ConnectFailed { reason } => {
                self.conn = ConnState::Failed;
                self.conn_error = Some(reason.clone());
            }
            AppEvent::SignInRequired { reason } => {
                self.conn = ConnState::SignInRequired;
                self.conn_error = Some(
                    reason
                        .clone()
                        .unwrap_or_else(|| "sign-in required".to_string()),
                );
            }

            AppEvent::CatalogueProgress {
                operation_id,
                stage,
                completed,
                total,
                elapsed_ms,
            } => {
                if *operation_id >= self.catalogue_operation_id {
                    self.catalogue_operation_id = *operation_id;
                    self.catalogue_progress =
                        Some((stage.clone(), *completed, *total, *elapsed_ms));
                }
            }
            AppEvent::CatalogueSnapshot {
                refreshed_at,
                pending_remount,
                ..
            } => {
                self.catalogue_refreshed_at = Some(*refreshed_at);
                self.catalogue_pending_remount = *pending_remount;
            }
            AppEvent::MountProgress {
                stage,
                completed,
                total,
                elapsed_ms,
            } => {
                self.mount_progress = Some((stage.clone(), *completed, *total, *elapsed_ms));
            }
            AppEvent::CatalogueLoading => {
                self.conn_error = None;
            }
            AppEvent::CatalogueLoaded {
                platforms,
                roms,
                skipped_unsupported,
            } => {
                self.catalogue = Some((*platforms, *roms, *skipped_unsupported));
                self.catalogue_progress = None;
                self.conn_error = None;
            }
            AppEvent::CatalogueFailed { reason } => {
                // Auth may have succeeded while the catalogue fetch failed:
                // stay Connected but surface the error — a failed catalogue
                // must never render as a successful (empty) library (R1).
                self.conn_error = Some(reason.clone());
                self.catalogue_progress = None;
            }

            AppEvent::MountStarting { path } => {
                self.mount = MountState::Mounting;
                self.mount_path = Some(path.clone());
                self.mount_error = None;
                self.foreign_root = None;
                self.foreign_entries.clear();
                self.foreign_removal = None;
            }
            AppEvent::MountStarted { path } => {
                self.mount = MountState::Mounted;
                self.catalogue_pending_remount = false;
                self.mount_progress = None;
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
                if let Some(existing) = self.save_sync_games.iter_mut().find(|existing| {
                    (existing.rom_id, existing.file_id) == (game.rom_id, game.file_id)
                }) {
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
                    self.save_file_games.clear();
                    self.save_file_unmatched.clear();
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
                file_id,
                rom_id,
                file_name,
                total,
            } => {
                let row = self.download_row(*rom_id, *file_id, file_name);
                row.received = 0;
                row.total = *total;
                row.finished = None;
            }
            AppEvent::DownloadProgress {
                file_id,
                rom_id,
                received,
                total,
            } => {
                let file_name = String::new();
                let row = self.download_row(*rom_id, *file_id, &file_name);
                row.received = *received;
                if total.is_some() {
                    row.total = *total;
                }
            }
            AppEvent::DownloadFinished {
                rom_id,
                file_id,
                file_name,
            } => {
                let row = self.download_row(*rom_id, *file_id, file_name);
                row.finished = Some(Ok(()));
            }
            AppEvent::DownloadFailed {
                file_id,
                rom_id,
                file_name,
                reason,
            } => {
                let row = self.download_row(*rom_id, *file_id, file_name);
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

    /// Whether the games folder is usable right now, derived only from real
    /// mount events — never from connection, configuration, or sync state.
    pub fn folder_summary(&self) -> FolderSummary {
        match self.mount {
            MountState::Mounted => FolderSummary::Ready,
            MountState::Mounting => FolderSummary::Starting,
            MountState::Failed => FolderSummary::Failed,
            MountState::NotMounted => {
                if self.mount_path.is_some() || self.configured_mount_root.is_some() {
                    FolderSummary::Stopped
                } else {
                    FolderSummary::SetupNeeded
                }
            }
        }
    }

    /// The one derived save-sync status (UI-PLAN §5 table). Zero eligible
    /// files is not "up to date"; an unlinked-file count does not downgrade
    /// an otherwise healthy linked set.
    pub fn save_sync_summary(&self) -> SaveSyncSummary {
        if self.conn == ConnState::SignInRequired || self.save_sync_authentication_required {
            return SaveSyncSummary::AuthRequired;
        }
        if self.conn == ConnState::Failed {
            return SaveSyncSummary::Offline;
        }
        if !self.save_sync_enabled {
            return SaveSyncSummary::Off;
        }
        if !self.save_sync_available {
            return SaveSyncSummary::NeedsSetup;
        }
        let Some(queue) = self.save_sync_queue.as_ref() else {
            return SaveSyncSummary::Checking;
        };
        if queue.actor_failed || queue.failure.is_some() || self.save_sync_failure.is_some() {
            return SaveSyncSummary::NeedsAttention;
        }
        if queue.network_paused {
            return SaveSyncSummary::Paused;
        }
        if queue.mapped_games == 0 {
            return SaveSyncSummary::NoLinkedFiles;
        }
        if queue.reconciled_games < queue.mapped_games {
            return SaveSyncSummary::Reconciling;
        }
        if queue.pending_outbound > 0 {
            return SaveSyncSummary::Syncing;
        }
        if queue.attention_games > 0 || queue.pending_incoming > 0 {
            return SaveSyncSummary::NeedsAttention;
        }
        SaveSyncSummary::UpToDate
    }

    /// Mark a mount request pending immediately so a second UI click cannot
    /// enqueue another start before the worker's MountStarting event arrives.
    pub fn request_mount(&mut self, path: String) {
        self.mount = MountState::Mounting;
        self.mount_path = Some(path);
        self.mount_error = None;
    }

    fn download_row(&mut self, rom_id: u64, file_id: u64, file_name: &str) -> &mut DownloadView {
        if let Some(pos) = self
            .downloads
            .iter()
            .position(|d| d.rom_id == rom_id && d.file_id == file_id)
        {
            let row = &mut self.downloads[pos];
            if !file_name.is_empty() {
                row.file_name = file_name.to_string();
            }
            return row;
        }
        self.downloads.push(DownloadView {
            rom_id,
            file_id,
            file_name: file_name.to_string(),
            received: 0,
            total: None,
            finished: None,
        });
        self.downloads.last_mut().unwrap()
    }

    pub fn clear_completed_downloads(&mut self) {
        self.downloads
            .retain(|d| !matches!(d.finished, Some(Ok(()))));
    }

    fn prune_downloads(&mut self) {
        while self
            .downloads
            .iter()
            .filter(|d| matches!(d.finished, Some(Ok(()))))
            .count()
            > MAX_DOWNLOAD_ROWS
        {
            let drop_at = self
                .downloads
                .iter()
                .position(|d| matches!(d.finished, Some(Ok(()))));
            let Some(drop_at) = drop_at else {
                break;
            };
            self.downloads.remove(drop_at);
        }
    }
}

/// Commands the window sends to the controller.
pub enum Command {
    CancelAuxiliary,
    MaterializeAuxiliary,
    SetStartAtLogin {
        enabled: bool,
    },
    SelectSaveFolder {
        path: PathBuf,
    },
    AssociateSaveFile {
        relative_path: PathBuf,
        rom_id: i64,
    },
    /// Associate every eligible unmatched save file under a save-root-relative
    /// directory group with one game (not its descendant groups).
    AssociateSaveFolder {
        prefix: PathBuf,
        rom_id: i64,
    },
    DownloadRetroBatInstaller,
    CancelRetroBatInstaller,
    LaunchRetroBatInstaller,
    SetupRetroBat {
        installation: PathBuf,
        managed_root: PathBuf,
        /// User consented to deleting only the classified stock/OS files and
        /// leftover links inside RetroBat's roms directory.
        remove_known: bool,
    },
    /// Delete foreign symlinks/junctions (never data-bearing reparse points) under a refused
    /// mount root, then republish the remaining foreign list.
    RemoveForeignLinks {
        root: PathBuf,
    },
    SetIntegration {
        mode: IntegrationMode,
        retrobat_root: Option<PathBuf>,
    },
    RestoreConnection,
    BeginPairing {
        url: String,
    },
    CancelPairing,
    Logout,
    Connect {
        url: String,
        username: String,
        password: String,
    },
    StartMount {
        path: String,
    },
    StopMount,
    RefreshCatalogue,
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
    connection_epoch: u64,
}

fn invalidates_save_sync_scope(command: &Command) -> bool {
    matches!(
        command,
        Command::SetIntegration { .. }
            | Command::SelectSaveFolder { .. }
            | Command::AssociateSaveFile { .. }
            | Command::AssociateSaveFolder { .. }
            | Command::SetupRetroBat { .. }
            | Command::BeginPairing { .. }
            | Command::CancelPairing
            | Command::Logout
            | Command::RestoreConnection
            | Command::Connect { .. }
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
    pub fn shutdown(&self) {
        self.send(Command::Shutdown);
        if let Some(worker) = self
            .worker
            .lock()
            .expect("controller worker mutex poisoned")
            .take()
        {
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
    metadata_active: Option<Arc<std::sync::atomic::AtomicBool>>,
    metadata_source: Option<(Catalogue, CatalogueSnapshot)>,
    metadata_ready: Option<rommfs_core::retrobat::ArtworkBatch>,
    metadata_wait_reported: bool,
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
    refresh_result: Option<mpsc::Receiver<Result<CatalogueSnapshot>>>,
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
            metadata_active: None,
            metadata_source: None,
            metadata_ready: None,
            metadata_wait_reported: false,
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

    fn refresh_save_sync(&mut self) {
        if self.startup_restoring || (self.startup_requires_mount && self.mount.is_none()) {
            return;
        }
        self.refresh_save_sync_from(save_sync_discovery_input());
    }

    fn refresh_save_sync_from(&mut self, input: DiscoveryInput) {
        self.advance_save_sync_session();
        self.stop_save_sync_agent();
        if self.preferences.is_some() {
            self.refresh_save_files(input);
            return;
        }
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
        if let Some(prefs) = self.preferences.as_mut() {
            prefs.save_root = None;
            // Selecting a save installation must not change the mounted
            // library's integration capabilities or metadata destination.
            if let Some(settings) = &self.connection_settings {
                if let Err(e) = settings.save(prefs) {
                    self.save_sync_runtime_problem = Some(e.to_string());
                }
            }
        }
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
                    .is_some_and(|report| {
                        report.supported_count() > 0 || self.save_file_rules.is_some()
                    });
            if !ready {
                self.save_sync_runtime_problem = Some(
                    "Enable refused: verify the account, save folder, and game mappings first."
                        .into(),
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
            let saved = if self.save_file_rules.is_some() {
                settings.save_files(scope, settings_value)
            } else {
                settings.save(scope, settings_value)
            };
            if let Err(error) = saved {
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
            let consent = ConsentSettings {
                enabled: self.save_sync_enabled,
                debounce_secs: seconds,
            };
            let saved = if self.save_file_rules.is_some() {
                settings.save_files(scope, consent)
            } else {
                settings.save(scope, consent)
            };
            if let Err(error) = saved {
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
        let result: Result<()> = (|| {
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
                file_id: mapping.rom_key.file_id,
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
        self.cancel_pairing();
        if self.mount.is_some() {
            self.stop_mount();
        }

        // Starting a new connection replaces the previous session, even when
        // URL validation or authentication fails. Keep the mounted-session
        // rejection above intact because that mount still owns its client.
        // Dropping the old result receiver prevents stale refreshes from publishing.
        self.cancel_catalogue_refresh();
        self.source_snapshot = None;
        self.catalogue_account = None;
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

        self.accept_connection(client, url, save_sync_identity);
    }

    fn forget_other_account(&mut self, url: &str, account_id: Option<i64>) {
        if self.preferences.as_ref().is_some_and(|p| {
            server_id_of(&p.server) != server_id_of(url) || account_id != Some(p.account_id)
        }) {
            if let Some(p) = self.preferences.take() {
                if let Err(error) =
                    rommfs_core::credential_store::delete(&server_id_of(&p.server), p.account_id)
                {
                    self.log(
                        Level::Error,
                        "credentials",
                        format!("Previous remembered account could not be removed: {error}"),
                    );
                }
            }
            if let Some(settings) = &self.connection_settings {
                if let Err(error) = settings.clear() {
                    self.log(Level::Error, "settings", error.to_string());
                }
            }
        }
    }

    fn accept_connection(
        &mut self,
        client: Arc<RommClient>,
        url: &str,
        identity: Option<rommfs_core::romm::SaveSyncIdentity>,
    ) {
        self.forget_other_account(url, identity.as_ref().map(|i| i.account_id));
        if self.preferences.is_none() {
            if let Some(identity) = &identity {
                self.preferences = Some(ConnectionPreferences {
                    server: url.into(),
                    account_id: identity.account_id,
                    device_identifier: uuid::Uuid::new_v4().to_string(),
                    mount_root: None,
                    integration: IntegrationMode::Generic,
                    retrobat_root: None,
                    start_at_login: false,
                    save_root: None,
                });
            }
        }
        self.emit_integration(None);
        self.client = Some(Arc::clone(&client));
        self.server_id = Some(server_id_of(url));
        self.catalogue_account = identity
            .as_ref()
            .map(|identity| identity.account_id.to_string());
        self.save_sync_identity = identity;
        self.sink.emit(AppEvent::Connected);
        self.log(
            Level::Info,
            "connect",
            "Authenticated connection established",
        );
        if self.restore_catalogue() {
            self.refresh_save_sync();
            self.refresh_catalogue();
            return;
        }
        match self.load_catalogue(&client) {
            Ok(cat) => {
                self.store_catalogue(cat);
                self.refresh_save_sync();
            }
            Err(error) => {
                if error.needs_sign_in() {
                    self.sink.emit(AppEvent::SignInRequired {
                        reason: Some(error.to_string()),
                    });
                } else {
                    self.sink.emit(AppEvent::CatalogueFailed {
                        reason: error.to_string(),
                    });
                }
                self.log(Level::Error, "catalogue", error.to_string());
                self.refresh_save_sync();
            }
        }
    }

    fn begin_pairing(&mut self, url: &str) {
        self.clear_active_connection();
        self.sink.emit(AppEvent::Connecting);
        let result: Result<()> = (|| {
            let client = Arc::new(RommClient::new(url)?);
            let identifier = self
                .preferences
                .as_ref()
                .map(|p| p.device_identifier.clone())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let request = client.begin_pairing(&identifier)?;
            let browser_url = client.pairing_url(&request)?;
            let interval = Duration::from_secs(request.interval.max(1));
            self.sink.emit(AppEvent::PairingReady {
                user_code: request.user_code.clone(),
                url: browser_url,
            });
            self.pairing = Some(PendingPairing {
                client,
                url: url.into(),
                identifier,
                expires: Instant::now() + Duration::from_secs(request.expires_in.min(600)),
                next_poll: Instant::now() + interval,
                interval,
                request,
                epoch: self.current_connection_epoch,
            });
            Ok(())
        })();
        if let Err(error) = result {
            self.fail_connect(&error);
        }
    }

    fn cancel_pairing(&mut self) {
        if self.pairing.take().is_some() {
            self.sink.emit(AppEvent::PairingEnded {
                reason: Some("Browser sign-in cancelled".into()),
            });
        }
    }

    fn poll_pairing(&mut self) {
        let Some(mut pairing) = self.pairing.take() else {
            return;
        };
        if pairing.epoch != self.connection_gate.current_epoch() {
            self.pairing = Some(pairing);
            return;
        }
        if Instant::now() >= pairing.expires {
            self.sink.emit(AppEvent::PairingEnded {
                reason: Some("Pairing expired; sign in again".into()),
            });
            return;
        }
        if Instant::now() < pairing.next_poll {
            self.pairing = Some(pairing);
            return;
        }
        let result = pairing.client.poll_pairing(&pairing.request.device_code);
        if pairing.epoch != self.connection_gate.current_epoch() {
            self.pairing = Some(pairing);
            return;
        }
        match result {
            Ok(PairingPoll::Pending | PairingPoll::SlowDown) => {
                if matches!(result, Ok(PairingPoll::SlowDown)) {
                    pairing.interval += Duration::from_secs(5);
                }
                pairing.next_poll = Instant::now() + pairing.interval;
                self.pairing = Some(pairing);
            }
            Ok(PairingPoll::Denied | PairingPoll::Expired) => {
                self.sink.emit(AppEvent::PairingEnded {
                    reason: Some("Browser sign-in denied or expired; sign in again".into()),
                });
            }
            Ok(PairingPoll::Approved(secret)) => {
                let result = self.finish_pairing(&pairing, &secret);
                if pairing.epoch != self.connection_gate.current_epoch() {
                    self.pairing = Some(pairing);
                    return;
                }
                self.sink.emit(AppEvent::PairingEnded {
                    reason: result.as_ref().err().map(ToString::to_string),
                });
                if let Err(error) = result {
                    self.fail_connect(&error);
                }
            }
            Err(error) => {
                self.sink.emit(AppEvent::PairingEnded {
                    reason: Some(error.to_string()),
                });
                self.fail_connect(&error);
            }
        }
    }

    fn finish_pairing(&mut self, pairing: &PendingPairing, secret: &PairedSession) -> Result<()> {
        pairing.client.restore_paired_session(secret)?;
        let identity = pairing.client.save_sync_identity()?.ok_or_else(|| {
            Error::Auth("pairing did not return a verified account; approve me.read".into())
        })?;
        if pairing.epoch != self.connection_gate.current_epoch() {
            return Err(Error::Cancelled);
        }
        self.forget_other_account(&pairing.url, Some(identity.account_id));
        let server = server_id_of(&pairing.url);
        let mut preferences = self
            .preferences
            .clone()
            .unwrap_or_else(|| ConnectionPreferences {
                server: pairing.url.clone(),
                account_id: identity.account_id,
                device_identifier: pairing.identifier.clone(),
                mount_root: None,
                integration: IntegrationMode::Generic,
                retrobat_root: None,
                start_at_login: false,
                save_root: None,
            });
        preferences.device_identifier = pairing.identifier.clone();
        // A credential-store failure leaves the authenticated in-memory session
        // usable, but is explicitly not represented as remembered sign-in.
        match rommfs_core::credential_store::save(&server, identity.account_id, secret) {
            Ok(()) => {
                if let Some(settings) = &self.connection_settings {
                    settings.save(&preferences)?;
                }
            }
            Err(error) => self.log(
                Level::Warn,
                "credentials",
                format!("Sign-in will not be remembered: {error}"),
            ),
        }
        self.preferences = Some(preferences);
        self.accept_connection(pairing.client.clone(), &pairing.url, Some(identity));
        Ok(())
    }

    fn restore_connection(&mut self) {
        let result: Result<()> = (|| {
            let Some(settings) = &self.connection_settings else {
                return Ok(());
            };
            let Some(preferences) = settings.load()? else {
                return Ok(());
            };
            self.sink.emit(AppEvent::ConnectionRestored {
                server: preferences.server.clone(),
                mount_root: preferences
                    .mount_root
                    .as_ref()
                    .map(|p| p.display().to_string()),
            });
            self.preferences = Some(preferences.clone());
            let Some(secret) = rommfs_core::credential_store::load(
                &server_id_of(&preferences.server),
                preferences.account_id,
            )?
            else {
                return Err(Error::Auth(
                    "remembered account needs browser sign-in".into(),
                ));
            };
            let client = Arc::new(RommClient::new(&preferences.server)?);
            client.restore_paired_session(&secret)?;
            // Authentication rejection is never hidden by a warm cache.
            let identity = client
                .save_sync_identity()?
                .ok_or_else(|| Error::Auth("stored session account unavailable".into()))?;
            if identity.account_id != preferences.account_id {
                return Err(Error::Auth(
                    "stored session belongs to another account".into(),
                ));
            }
            self.sink.emit(AppEvent::Connecting);
            self.startup_restoring =
                std::env::args().any(|arg| arg == "--startup") && preferences.start_at_login;
            self.startup_requires_mount = self.startup_restoring;
            self.sink.emit(AppEvent::StartupPreference {
                enabled: preferences.start_at_login,
                error: None,
            });
            self.accept_connection(client, &preferences.server, Some(identity));
            if self.startup_restoring {
                if let Some(root) = &preferences.mount_root {
                    self.start_mount(&root.display().to_string());
                }
                self.startup_restoring = false;
                if self.mount.is_some() {
                    self.refresh_save_sync();
                } else {
                    self.log(Level::Warn,"startup","Automatic mount did not complete; save sync remains stopped. Open RomMFS to inspect the error.");
                    self.sink.emit(AppEvent::StartupPreference { enabled:preferences.start_at_login,error:Some("Automatic mount did not complete. Start the mount before resuming save sync.".into()) });
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.startup_restoring = false;
            self.fail_connect(&error);
        }
        if self.client.is_none() {
            self.refresh_save_sync();
        }
    }

    fn clear_active_connection(&mut self) {
        self.startup_requires_mount = false;
        self.startup_restoring = false;
        self.cancel_auxiliary();
        self.save_file_rules = None;
        self.cancel_pairing();
        self.stop_save_sync_agent();
        self.stop_mount();
        self.advance_save_sync_session();
        self.cancel_catalogue_refresh();
        self.client = None;
        self.server_id = None;
        self.catalogue = None;
        self.source_snapshot = None;
        self.catalogue_account = None;
        self.names.clear();
        self.save_sync_catalogue = None;
        self.save_sync_identity = None;
        self.save_sync_scope = None;
        self.save_sync_enabled = false;
    }

    fn logout(&mut self) {
        self.clear_active_connection();
        let result: Result<()> = (|| {
            if let Some(preferences) = &self.preferences {
                rommfs_core::credential_store::delete(
                    &server_id_of(&preferences.server),
                    preferences.account_id,
                )?;
            }
            if let Some(settings) = &self.connection_settings {
                settings.clear()?;
            }
            Ok(())
        })();
        self.preferences = None;
        if cfg!(windows) {
            if let Err(e) = crate::desktop::set_start_at_login(false) {
                self.log(
                    Level::Warn,
                    "startup",
                    format!("Could not remove login startup entry: {e}"),
                );
            }
        }
        self.sink.emit(AppEvent::StartupPreference {
            enabled: false,
            error: None,
        });
        if let Err(error) = result {
            self.log(
                Level::Error,
                "logout",
                format!("Could not remove remembered sign-in: {error}"),
            );
        }
        self.sink.emit(AppEvent::SignedOut);
        self.refresh_save_sync();
    }

    fn emit_integration(&self, error: Option<String>) {
        let preferences = self.preferences.as_ref();
        self.sink.emit(AppEvent::IntegrationUpdated {
            mode: preferences.map(|p| p.integration).unwrap_or_default(),
            retrobat_root: preferences
                .and_then(|p| p.retrobat_root.as_ref())
                .map(|p| p.display().to_string()),
            error,
        });
    }

    fn metadata_status(&self, pending: bool, detail: impl Into<String>, error: Option<String>) {
        self.sink.emit(AppEvent::FrontendMetadata {
            pending,
            detail: detail.into(),
            error,
        });
    }

    fn cancel_metadata(&mut self) {
        if let Some(active) = self.metadata_active.take() {
            active.store(false, std::sync::atomic::Ordering::Release);
        }
        self.metadata_job = None;
        if let Some(handle) = self.metadata_handle.take() {
            let _ = handle.join();
        }
        self.metadata_ready = None;
        self.metadata_source = None;
        self.metadata_wait_reported = false;
    }

    fn queue_metadata(&mut self) {
        self.cancel_metadata();
        let Some(preferences) = self
            .preferences
            .as_ref()
            .filter(|p| p.integration == IntegrationMode::RetroBat)
        else {
            return;
        };
        let (Some(fs), Some(snapshot), Some(client)) =
            (&self.fs, &self.source_snapshot, &self.client)
        else {
            return;
        };
        let catalogue = fs.catalogue().clone();
        // A refreshed structure is staged for the next mount. Never generate
        // launch paths for files that are absent from this immutable native tree.
        if self.catalogue.as_ref().is_some_and(|next| {
            next.entries.len() != catalogue.entries.len()
                || next.entries.iter().zip(&catalogue.entries).any(|(a, b)| {
                    a.key != b.key
                        || a.platform_dir != b.platform_dir
                        || a.file_name != b.file_name
                        || next.is_launchable(&a.key) != catalogue.is_launchable(&b.key)
                })
        }) {
            self.metadata_status(
                true,
                "Frontend metadata retained; library changes apply on the next remount",
                None,
            );
            return;
        }
        let snapshot = snapshot.clone();
        let roms = snapshot.roms.clone();
        let client = client.clone();
        let cache = cache_dir_for(self.server_id.as_deref().unwrap_or_default())
            .join("artwork")
            .join(preferences.account_id.to_string());
        let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
        self.metadata_active = Some(active.clone());
        self.metadata_source = Some((catalogue.clone(), snapshot));
        let (tx, rx) = mpsc::channel();
        self.metadata_job = Some(rx);
        self.metadata_status(
            true,
            "Caching frontend artwork without downloading ROMs",
            None,
        );
        self.metadata_handle = Some(std::thread::spawn(move || {
            let _ = tx.send(rommfs_core::retrobat::cache_artwork_cancellable(
                &client, &catalogue, &roms, &cache, &active,
            ));
        }));
    }

    fn poll_metadata(&mut self) {
        if let Some(rx) = &self.metadata_job {
            let result = match rx.try_recv() {
                Ok(result) => result,
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Err(Error::Transport("artwork worker stopped".into()))
                }
            };
            self.metadata_job = None;
            if let Some(handle) = self.metadata_handle.take() {
                let _ = handle.join();
            }
            match result {
                Ok(artwork) => self.metadata_ready = Some(artwork),
                Err(error) => {
                    self.cancel_metadata();
                    if error.needs_sign_in() {
                        self.stop_save_sync_agent();
                        self.stop_mount();
                        self.sink.emit(AppEvent::SignInRequired {
                            reason: Some(error.to_string()),
                        });
                    }
                    self.log(Level::Error, "metadata", error.to_string());
                    self.metadata_status(
                        false,
                        "Frontend metadata was not published",
                        Some(error.to_string()),
                    );
                    return;
                }
            }
        }
        if self.metadata_ready.is_none() {
            return;
        }
        let Some(installation) = self
            .preferences
            .as_ref()
            .and_then(|p| p.retrobat_root.as_deref())
        else {
            self.cancel_metadata();
            return;
        };
        let guard = match stopped_retrobat_guard(installation) {
            Ok(guard) => guard,
            Err(error)
                if !error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|io| matches!(io.raw_os_error(), Some(5 | 32 | 33))) =>
            {
                self.cancel_metadata();
                self.log(Level::Error, "metadata", format!("{error:#}"));
                self.metadata_status(
                    false,
                    "Frontend metadata cannot be published safely",
                    Some(format!("{error:#}")),
                );
                return;
            }
            Err(error) => {
                if !self.metadata_wait_reported {
                    self.metadata_status(true, "Frontend metadata is ready; waiting for exclusive frontend/emulator access. Stop RetroBat and check the reported access error", Some(format!("{error:#}")));
                    self.metadata_wait_reported = true;
                }
                return;
            }
        };
        let result = (|| -> anyhow::Result<()> {
            let root = &self
                .mount
                .as_ref()
                .context("mount stopped before metadata publication")?
                .root;
            let server = self.server_id.as_deref().context("connection ended")?;
            let account = self
                .preferences
                .as_ref()
                .context("account changed")?
                .account_id;
            let (catalogue, snapshot) = self
                .metadata_source
                .as_ref()
                .context("metadata source unavailable")?;
            let artwork = self
                .metadata_ready
                .as_ref()
                .context("artwork unavailable")?;
            for warning in artwork.warnings() {
                self.log(Level::Warn, "artwork", warning.clone());
            }
            let (index, existing) = rommfs_cfapi::read_frontend_metadata(root, server)?;
            let publication = rommfs_core::retrobat::generate_gamelists(
                catalogue,
                &snapshot.roms,
                server,
                account,
                &index,
                &existing,
                artwork.references(),
            )?;
            rommfs_cfapi::publish_frontend_metadata(root, server, &publication, artwork.files())?;
            self.log(
                Level::Info,
                "metadata",
                format!(
                    "Published {} gamelists; frontend favourites and history preserved",
                    publication.gamelists().len()
                ),
            );
            Ok(())
        })();
        drop(guard);
        self.cancel_metadata();
        match result {
            Ok(()) => self.metadata_status(false, "Frontend metadata is up to date", None),
            Err(error) => {
                self.log(Level::Error, "metadata", format!("{error:#}"));
                self.metadata_status(
                    false,
                    "Frontend metadata was not fully published; existing state was retained",
                    Some(format!("{error:#}")),
                );
            }
        }
    }

    fn setup_retrobat(&mut self, installation: &Path, managed_root: &Path, remove_known: bool) {
        self.cancel_auxiliary();
        self.sink.emit(AppEvent::RetroBatSetup {
            running: true,
            detail: "Checking installation and ROM ownership".into(),
            error: None,
            removable_conflicts: 0,
        });
        let result = (|| -> anyhow::Result<Vec<String>> {
            anyhow::ensure!(self.mount.is_none(), "stop the mount before RetroBat setup");
            anyhow::ensure!(
                installation.is_absolute() && managed_root.is_absolute(),
                "choose absolute RetroBat and managed ROM folder paths"
            );
            let server = self
                .server_id
                .as_deref()
                .context("connect before setting up RetroBat")?;
            let _stopped = stopped_retrobat_guard(installation)?;
            if let Err(e) = check_mount_root(managed_root, server) {
                self.emit_foreign_entries(managed_root);
                return Err(e.into());
            }
            if let Err(e) = claim_mount_root(managed_root, server) {
                self.emit_foreign_entries(managed_root);
                return Err(e.into());
            }
            // Validate the proposed settings before linking so an override error
            // never leaves an unexpectedly reconfigured ROM directory.
            let (home, _) = rommfs_core::retrobat::launcher_paths(installation)?;
            rommfs_core::retrobat::switch_save_setting(&std::fs::read_to_string(
                home.join("es_settings.cfg"),
            )?)?;
            rommfs_core::retrobat::setup_roms_link(
                installation,
                managed_root,
                server,
                remove_known,
            )?;
            rommfs_core::retrobat::configure_switch_saves(installation)
        })();
        match result {
            Ok(messages) => {
                let detail = format!(
                    "RetroBat ROM directory linked to {}. {}",
                    managed_root.display(),
                    messages.join(". ")
                );
                self.log(Level::Info, "retrobat-setup", detail.clone());
                self.set_integration(IntegrationMode::RetroBat, Some(installation.to_path_buf()));
                self.sink.emit(AppEvent::RetroBatSetup {
                    running: false,
                    detail,
                    error: None,
                    removable_conflicts: 0,
                });
            }
            Err(error) => {
                let removable = error
                    .downcast_ref::<rommfs_core::retrobat::RomsConflicts>()
                    .map_or(0, |c| c.removable.len());
                let error = format!("{error:#}");
                self.log(Level::Error, "retrobat-setup", error.clone());
                self.sink.emit(AppEvent::RetroBatSetup {
                    running: false,
                    detail: "RetroBat setup did not complete".into(),
                    error: Some(error),
                    removable_conflicts: removable,
                });
            }
        }
    }

    fn set_integration(&mut self, mode: IntegrationMode, root: Option<PathBuf>) {
        self.cancel_auxiliary();
        let previous = self.preferences.clone();
        let result: Result<()> = (|| {
            if self.mount.is_some() {
                return Err(Error::Unsupported(
                    "stop the mount before changing its layout".into(),
                ));
            }
            let preferences = self.preferences.as_mut().ok_or_else(|| {
                Error::Auth("connect and verify an account before choosing the layout".into())
            })?;
            if mode == IntegrationMode::RetroBat {
                let root = root
                    .as_deref()
                    .ok_or_else(|| Error::Unsupported("select a RetroBat installation".into()))?;
                rommfs_core::layout::RetroBatSystems::from_installation(root)?;
            }
            preferences.integration = mode;
            preferences.retrobat_root = root;
            let catalogue = self
                .source_snapshot
                .as_ref()
                .map(|s| self.project_snapshot(s))
                .transpose()?;
            if let (Some(settings), Some(preferences)) =
                (&self.connection_settings, &self.preferences)
            {
                settings.save(preferences)?;
            }
            self.stop_save_sync_agent();
            self.advance_save_sync_session();
            if let Some(catalogue) = catalogue {
                self.sink.emit(AppEvent::CatalogueLoaded {
                    platforms: catalogue.platforms.len(),
                    roms: catalogue.entries.len(),
                    skipped_unsupported: catalogue.skipped_unsupported,
                });
                self.store_catalogue(catalogue);
            }
            self.refresh_save_sync();
            Ok(())
        })();
        if let Err(error) = result {
            self.preferences = previous;
            self.log(Level::Error, "integration", error.to_string());
            self.emit_integration(Some(error.to_string()));
        } else {
            self.emit_integration(None);
        }
    }

    fn project_snapshot(&self, snapshot: &CatalogueSnapshot) -> Result<Catalogue> {
        let systems = match self.preferences.as_ref() {
            Some(preferences) if preferences.integration == IntegrationMode::RetroBat => {
                let root = preferences
                    .retrobat_root
                    .as_deref()
                    .ok_or_else(|| Error::Unsupported("select a RetroBat installation".into()))?;
                Some(rommfs_core::layout::RetroBatSystems::from_installation(
                    root,
                )?)
            }
            _ => None,
        };
        let mut warnings = Vec::new();
        let result = rommfs_core::catalog::build_catalogue_for_layout(
            self.server_id.as_deref().unwrap_or_default(),
            &snapshot.platforms,
            &snapshot.roms,
            systems.as_ref(),
            |warning| warnings.push(warning),
        );
        emit_catalogue_warnings(&self.sink, warnings);
        result
    }

    fn publish_snapshot(&mut self, snapshot: CatalogueSnapshot, cached: bool) -> Result<Catalogue> {
        let cat = self.project_snapshot(&snapshot)?;
        self.sink.emit(AppEvent::CatalogueLoaded {
            platforms: cat.platforms.len(),
            roms: cat.entries.len(),
            skipped_unsupported: cat.skipped_unsupported,
        });
        self.sink.emit(AppEvent::CatalogueSnapshot {
            refreshed_at: snapshot.refreshed_at,
            cached,
            pending_remount: self.mount.is_some(),
        });
        self.source_snapshot = Some(snapshot);
        Ok(cat)
    }

    fn persist_snapshot(&self, snapshot: &CatalogueSnapshot) {
        if let (Some(store), Some(server), Some(account)) = (
            &self.catalogue_store,
            &self.server_id,
            &self.catalogue_account,
        ) {
            if let Err(error) = store.save(server, account, snapshot) {
                self.log(
                    Level::Warn,
                    "catalogue",
                    format!("Catalogue could not be persisted: {error}"),
                );
            }
        }
    }

    fn restore_catalogue(&mut self) -> bool {
        let snapshot = match (
            &self.catalogue_store,
            &self.server_id,
            &self.catalogue_account,
        ) {
            (Some(store), Some(server), Some(account)) => store.load(server, account),
            _ => return false,
        };
        match snapshot {
            Ok(Some(snapshot)) => match self.publish_snapshot(snapshot, true) {
                Ok(cat) => {
                    self.store_catalogue(cat);
                    true
                }
                Err(error) => {
                    self.log(Level::Warn, "catalogue", error.to_string());
                    false
                }
            },
            Err(error) => {
                self.log(Level::Warn, "catalogue", error.to_string());
                false
            }
            Ok(None) => false,
        }
    }

    fn load_catalogue(&mut self, client: &RommClient) -> Result<Catalogue> {
        self.sink.emit(AppEvent::CatalogueLoading);
        self.catalogue_operation_id += 1;
        let snapshot = fetch_catalogue(client, &self.sink, self.catalogue_operation_id)?;
        let cat = self.publish_snapshot(snapshot.clone(), false)?;
        self.persist_snapshot(&snapshot);
        Ok(cat)
    }

    /// At most one refresh in flight. The mounted tree remains immutable; the
    /// complete replacement projection is used only by the next mount.
    fn refresh_catalogue(&mut self) {
        if self.refresh_result.is_some() {
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        self.catalogue_operation_id += 1;
        let operation_id = self.catalogue_operation_id;
        let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let sink = self.sink.clone().scoped(active.clone());
        self.refresh_active = Some(active);
        let (tx, rx) = mpsc::channel();
        self.refresh_result = Some(rx);
        self.refresh_handle = Some(std::thread::spawn(move || {
            let _ = tx.send(fetch_catalogue(&client, &sink, operation_id));
        }));
    }

    fn poll_catalogue_refresh(&mut self) {
        let Some(rx) = &self.refresh_result else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err(Error::Transport("catalogue refresh worker stopped".into()))
            }
        };
        self.refresh_result = None;
        self.refresh_active = None;
        if let Some(handle) = self.refresh_handle.take() {
            let _ = handle.join();
        }
        self.finish_catalogue_refresh(result);
    }

    fn cancel_catalogue_refresh(&mut self) {
        if let Some(active) = self.refresh_active.take() {
            active.store(false, std::sync::atomic::Ordering::Release);
        }
        self.refresh_result = None;
        if let Some(handle) = self.refresh_handle.take() {
            let _ = handle.join();
        }
    }

    fn finish_catalogue_refresh(&mut self, result: Result<CatalogueSnapshot>) {
        match result {
            Ok(snapshot) => match self.publish_snapshot(snapshot.clone(), false) {
                Ok(cat) => {
                    self.persist_snapshot(&snapshot);
                    self.store_catalogue(cat);
                    if self.mount.is_none() {
                        self.refresh_save_sync();
                    } else {
                        self.queue_metadata();
                    }
                }
                Err(error) => self.catalogue_refresh_failed(error),
            },
            Err(error) => self.catalogue_refresh_failed(error),
        }
    }

    fn catalogue_refresh_failed(&mut self, error: Error) {
        // Never replace the last successful catalogue with an empty library.
        if error.needs_sign_in() {
            self.stop_save_sync_agent();
            self.stop_mount();
            self.sink.emit(AppEvent::SignInRequired {
                reason: Some(error.to_string()),
            });
        }
        self.sink.emit(AppEvent::CatalogueFailed {
            reason: error.to_string(),
        });
        self.log(
            Level::Error,
            "catalogue",
            format!("Refresh failed; retaining last complete snapshot: {error}"),
        );
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
            self.sink.emit(AppEvent::SignInRequired {
                reason: Some(e.to_string()),
            });
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

    /// The classified foreign entries under a directory the app does not own —
    /// the remediation list the UI renders with per-kind actions.
    fn foreign_entry_list(root: &Path) -> Vec<ForeignEntry> {
        rommfs_cfapi::foreign_entries(root)
            .into_iter()
            .map(|e| ForeignEntry {
                path: e.path,
                kind: match e.kind {
                    rommfs_cfapi::UnownedKind::File => ForeignKind::File,
                    rommfs_cfapi::UnownedKind::Directory => ForeignKind::Directory,
                    rommfs_cfapi::UnownedKind::Link => ForeignKind::Link,
                },
            })
            .collect()
    }

    /// After a mount/setup refusal, publish what's actually in the way so the
    /// UI can offer "remove leftover links" / "open folder" instead of a bare
    /// path dump.
    fn emit_foreign_entries(&self, root: &Path) {
        let entries = Self::foreign_entry_list(root);
        if !entries.is_empty() {
            self.sink.emit(AppEvent::ForeignRootEntries {
                root: root.display().to_string(),
                entries,
                removal: None,
            });
        }
    }

    /// Remove only foreign symlinks/junctions under `root` — never real
    /// files or directories — then republish the remaining list.
    fn remove_foreign_links(&mut self, root: &Path) {
        let removal = rommfs_cfapi::remove_foreign_links(root).map_err(|e| format!("{e:#}"));
        match &removal {
            Ok(0) => self.log(Level::Info, "cleanup", "no leftover links found"),
            Ok(n) => self.log(
                Level::Info,
                "cleanup",
                format!("removed {n} leftover links under {}", root.display()),
            ),
            Err(e) => self.log(Level::Error, "cleanup", format!("link cleanup failed: {e}")),
        }
        self.sink.emit(AppEvent::ForeignRootEntries {
            root: root.display().to_string(),
            entries: Self::foreign_entry_list(root),
            removal: Some(removal),
        });
    }

    fn start_mount(&mut self, path: &str) {
        let path = path.trim();
        if self.auxiliary_job.is_some() {
            self.fail_mount(
                "wait for auxiliary downloads",
                path,
                &Error::Unsupported(
                    "Finish or cancel auxiliary materialization before mounting".into(),
                ),
            );
            return;
        }
        if self.save_sync_scope.as_ref().is_some_and(|scope| {
            rommfs_core::save_sync::paths_overlap(Path::new(path), &scope.effective_saves_root)
        }) {
            self.fail_mount(
                "check save-folder separation",
                path,
                &Error::Unsupported("ROM and save folders must not overlap".into()),
            );
            return;
        }
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
        if !client.has_token() && self.catalogue.is_some() {
            self.sink.emit(AppEvent::SignInRequired {
                reason: Some("sign in before mounting the cached library".into()),
            });
            self.fail_mount(
                "check authentication",
                path,
                &Error::Auth("sign in before mounting cached metadata".into()),
            );
            return;
        }
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
        if let Some(preferences) = self
            .preferences
            .as_ref()
            .filter(|p| p.integration == IntegrationMode::RetroBat)
        {
            let verified = preferences
                .retrobat_root
                .as_deref()
                .context("select a RetroBat installation")
                .and_then(|install| {
                    rommfs_core::retrobat::verify_setup_link(install, &root, &server_id)
                });
            if let Err(error) = verified {
                self.fail_mount(
                    "verify RetroBat setup",
                    path,
                    &Error::Unsupported(format!("{error:#}")),
                );
                return;
            }
        }
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
            self.emit_foreign_entries(&root);
            self.fail_mount("prepare and validate directory", path, &e);
            self.refresh_save_sync();
            return;
        }

        if self.catalogue.is_none() {
            // Reuse Connect's stable snapshot; failed cold loads may be retried.
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
                    self.sink.emit(AppEvent::SignInRequired {
                        reason: Some(e.to_string()),
                    });
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
        }
        self.log(
            Level::Info,
            "mount",
            format!("claiming ownership of {path:?}"),
        );
        if let Err(e) = claim_mount_root(&root, &server_id) {
            self.emit_foreign_entries(&root);
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
                if self.startup_requires_mount {
                    self.startup_requires_mount = false;
                    self.sink.emit(AppEvent::StartupPreference {
                        enabled: self.preferences.as_ref().is_some_and(|p| p.start_at_login),
                        error: None,
                    });
                }
                self.fs = Some(fs);
                if let Some(preferences) = &mut self.preferences {
                    preferences.mount_root = Some(root.clone());
                    if let Some(settings) = &self.connection_settings {
                        if let Err(error) = settings.save(preferences) {
                            self.log(Level::Warn, "settings", error.to_string());
                        }
                    }
                }
                self.sink
                    .emit(AppEvent::MountStarted { path: mounted_path });
                self.queue_metadata();
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
            Err(e) => {
                // Backend-start failures can carry UnownedEntries (foreign
                // files under the mount root); publish the remediation list.
                self.emit_foreign_entries(&root);
                self.fail_mount("start filesystem backend", path, &e)
            }
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
        let cache_directory = cache_dir_for(&server_id);
        #[cfg(test)]
        let cache_directory = self.rom_cache_directory.clone().unwrap_or(cache_directory);
        let index = CacheIndex::open(cache_directory)?;
        let live = Arc::new(LiveState::default());
        let source = Arc::new(ClientSource {
            client: Arc::clone(client),
            names: self.names.clone(),
        });
        let cat = self.catalogue.clone();
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
        let had_metadata = self.metadata_active.is_some()
            || self
                .preferences
                .as_ref()
                .is_some_and(|p| p.integration == IntegrationMode::RetroBat);
        self.cancel_metadata();
        if had_metadata {
            self.metadata_status(
                false,
                "Frontend files are retained while the mount is stopped",
                None,
            );
        }
        self.sink.emit(AppEvent::MountStopping);
        if let Some(fs) = &self.fs {
            fs.cancel_transfers();
        }
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
        worker.poll_pairing();
        worker.poll_catalogue_refresh();
        worker.poll_metadata();
        worker.installer.poll(&worker.sink);
        worker.poll_auxiliary();
        let received = if worker.refresh_result.is_some()
            || worker.auxiliary_job.is_some()
            || worker.installer.is_running()
            || worker.pairing.is_some()
            || worker.metadata_job.is_some()
            || worker.metadata_ready.is_some()
        {
            rx.recv_timeout(Duration::from_millis(100))
        } else if worker.mount.is_some() {
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
            Err(mpsc::RecvTimeoutError::Timeout)
                if worker.refresh_result.is_some()
                    || worker.auxiliary_job.is_some()
                    || worker.installer.is_running()
                    || worker.pairing.is_some()
                    || worker.metadata_job.is_some()
                    || worker.metadata_ready.is_some() =>
            {
                continue
            }
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
            Command::SetupRetroBat {
                installation,
                managed_root,
                remove_known,
            } => worker.setup_retrobat(installation, managed_root, *remove_known),
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
            Command::StartMount { path } => worker.start_mount(path),
            Command::StopMount => worker.stop_mount(),
            Command::RefreshCatalogue => worker.refresh_catalogue(),
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
                Command::StartMount { .. } => worker.sink.emit(AppEvent::MountFailed {
                    reason: format!("internal error: {msg}"),
                }),
                Command::StopMount => {}
                Command::SetupRetroBat { .. } => worker.sink.emit(AppEvent::RetroBatSetup {
                    running: false,
                    detail: "RetroBat setup stopped".into(),
                    error: Some(format!("internal error: {msg}")),
                    removable_conflicts: 0,
                }),
                Command::RemoveForeignLinks { .. } => {}
                Command::SetIntegration { .. }
                | Command::RefreshCatalogue
                | Command::RefreshSaveSync
                | Command::SelectSaveSyncInstallation { .. }
                | Command::SetSaveSyncEnabled { .. }
                | Command::SetSaveSyncDebounce { .. }
                | Command::ExportSaveSyncIncoming { .. } => {}
                Command::SelectSaveFolder { .. }
                | Command::AssociateSaveFile { .. }
                | Command::AssociateSaveFolder { .. } => {}
                Command::Shutdown => {}
                Command::MaterializeAuxiliary => {}
                Command::CancelAuxiliary => {}
                Command::SetStartAtLogin { .. } => {}
                Command::DownloadRetroBatInstaller
                | Command::CancelRetroBatInstaller
                | Command::LaunchRetroBatInstaller => {}
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
}

fn fetch_catalogue(
    client: &RommClient,
    sink: &EventSink,
    operation_id: u64,
) -> Result<CatalogueSnapshot> {
    let started = Instant::now();
    let request_started = Instant::now();
    if !sink.is_active() {
        return Err(Error::Cancelled);
    }
    let platforms = client.platforms();
    sink.emit(AppEvent::log(
        Level::Info,
        "http",
        format!(
            "operation={operation_id} GET /api/platforms elapsed_ms={} result={}",
            request_started.elapsed().as_millis(),
            if platforms.is_ok() {
                "ok".into()
            } else {
                platforms.as_ref().expect_err("failed result").to_string()
            }
        ),
    ));
    let platforms = platforms?;
    let mut roms = Vec::new();
    loop {
        if !sink.is_active() {
            return Err(Error::Cancelled);
        }
        let offset = roms.len() as u64;
        let request_started = Instant::now();
        let page = client.roms_page(200, offset);
        sink.emit(AppEvent::log(Level::Info, "http", format!("operation={operation_id} GET /api/roms limit=200 offset={offset} elapsed_ms={} result={}", request_started.elapsed().as_millis(), if page.is_ok() { "ok".into() } else { page.as_ref().expect_err("failed result").to_string() })));
        let page = page?;
        let count = page.items.len();
        let total = page.total;
        if page.offset != offset {
            return Err(Error::InvalidCatalogue(
                "catalogue pagination offset did not advance correctly".into(),
            ));
        }
        roms.extend(page.items);
        sink.emit(AppEvent::CatalogueProgress {
            operation_id,
            stage: "Catalogue pages".into(),
            completed: roms.len() as u64,
            total,
            elapsed_ms: started.elapsed().as_millis() as u64,
        });
        match total {
            Some(total) if roms.len() as u64 >= total => break,
            Some(_) if count == 0 => {
                return Err(Error::InvalidCatalogue(
                    "catalogue ended before declared total".into(),
                ))
            }
            None if count < 200 => break,
            _ => {}
        }
    }
    sink.emit(AppEvent::log(
        Level::Info,
        "catalogue",
        format!(
            "operation={operation_id} fetched {} platforms and {} ROMs in {}ms",
            platforms.len(),
            roms.len(),
            started.elapsed().as_millis()
        ),
    ));
    Ok(CatalogueSnapshot {
        platforms,
        roms,
        refreshed_at: rommfs_core::cache::now_unix_secs(),
    })
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
fn stopped_retrobat_guard(installation: &Path) -> anyhow::Result<Vec<std::fs::File>> {
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

fn check_mount_root(root: &Path, server_id: &str) -> Result<()> {
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

fn claim_mount_root(root: &Path, server_id: &str) -> Result<()> {
    rommfs_cfapi::claim_mount_root(root, server_id).map_err(mount_root_error)
}

/// Collapse the per-file repeats a catalogue build produces ("rom 824 file
/// 5425: …") into one Activity line per (rom, reason). The complete detail is
/// written to the diagnostic file, where bulk output belongs.
fn emit_catalogue_warnings(sink: &EventSink, warnings: Vec<String>) {
    let mut grouped: std::collections::BTreeMap<String, usize> = Default::default();
    for warning in &warnings {
        sink.log_detail(Level::Warn, "catalogue", warning.clone());
        *grouped.entry(summarize_warning(warning)).or_default() += 1;
    }
    for (message, count) in grouped {
        sink.emit(AppEvent::log(
            Level::Warn,
            "catalogue",
            if count > 1 {
                format!("{message} (×{count})")
            } else {
                message
            },
        ));
    }
}

/// "rom 824 file 5425: reason" → "rom 824: reason"; "file 228: invalid
/// category" → "invalid category". Anything already aggregate passes through.
fn summarize_warning(warning: &str) -> String {
    if let Some(rest) = warning.strip_prefix("rom ") {
        if let Some((rom, rest)) = rest.split_once(' ') {
            if let Some(rest) = rest.strip_prefix("file ") {
                if let Some((_, reason)) = rest.split_once(": ") {
                    return format!("rom {rom}: {reason}");
                }
            }
        }
        return warning.to_string();
    }
    if let Some((_, reason)) = warning
        .strip_prefix("file ")
        .and_then(|r| r.split_once(": "))
    {
        return reason.to_string();
    }
    warning.to_string()
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
    #[test]
    fn application_owner_keeps_worker_alive_after_view_handle_drops_and_quit_joins_once() {
        let (sink, _events) = rommfs_core::events::channel();
        let (cmd_tx, commands) = mpsc::channel::<CommandEnvelope>();
        let (facts, observed) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            for envelope in commands {
                let stopping = matches!(envelope.command, Command::Shutdown);
                facts.send(stopping).unwrap();
                if stopping {
                    break;
                }
            }
        });
        let application = std::rc::Rc::new(Controller {
            sink,
            cmd_tx,
            worker: std::sync::Mutex::new(Some(worker)),
            save_sync_gate: SaveSyncCommandGate::new(),
            connection_gate: SaveSyncCommandGate::new(),
        });
        let view = application.clone();
        drop(view);
        application.send(Command::StopMount);
        assert!(!observed.recv_timeout(Duration::from_secs(2)).unwrap());
        application.shutdown();
        assert!(observed.recv_timeout(Duration::from_secs(2)).unwrap());
        assert!(application.worker.lock().unwrap().is_none());
        application.shutdown();
    }
    use super::*;
    use rommfs_core::cache::{CacheIndex, FakeClock, LiveState, NoopHydratedRemover};
    use rommfs_core::download::ContentSource;
    use rommfs_core::romm::{PlatformDto, RomDto, RomFileDto, SaveSyncIdentity};
    use rommfs_core::save_sync::RETROBAT_GB_SRM_PROFILE;
    use rommfs_fixture::{FixtureBodyBarrier, FixtureSaveRecord, FixtureServer, ResponseSpec};
    use std::io::Write;
    use std::time::Duration;

    #[test]
    fn mount_root_errors_preserve_io_kind_context_and_permission_guidance() {
        for kind in [
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::NotFound,
        ] {
            let error = mount_root_error(
                anyhow::Error::new(std::io::Error::new(kind, "filesystem failure"))
                    .context("create ownership manifest C:\\RomM.rommfs-root"),
            );
            let Error::Io(io) = error else {
                panic!("filesystem failures must remain I/O errors");
            };
            assert_eq!(io.kind(), kind);
            let message = io.to_string();
            assert!(message.contains("C:\\RomM.rommfs-root: filesystem failure"));
            assert_eq!(
                message.contains("%USERPROFILE%\\RomM"),
                kind == std::io::ErrorKind::PermissionDenied
            );
        }
        assert!(matches!(
            mount_root_error(anyhow::anyhow!("root is not empty")),
            Error::Unsupported(_)
        ));
    }

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
    fn foreign_entries_populate_remediation_state_and_clear_on_retry() {
        let mut state = UiState::new(16);
        state.apply(&AppEvent::ForeignRootEntries {
            root: "C:\\RomM".into(),
            entries: vec![
                ForeignEntry {
                    path: "gb/gamelist.xml".into(),
                    kind: ForeignKind::File,
                },
                ForeignEntry {
                    path: "gb".into(),
                    kind: ForeignKind::Link,
                },
            ],
            removal: None,
        });
        assert_eq!(state.foreign_root.as_deref(), Some("C:\\RomM"));
        assert_eq!(state.foreign_entries.len(), 2);
        assert!(state.foreign_removal.is_none());
        state.apply(&AppEvent::ForeignRootEntries {
            root: "C:\\RomM".into(),
            entries: vec![ForeignEntry {
                path: "gb/gamelist.xml".into(),
                kind: ForeignKind::File,
            }],
            removal: Some(Ok(1)),
        });
        assert_eq!(state.foreign_removal, Some(Ok(1)));
        assert_eq!(state.foreign_entries.len(), 1);
        state.apply(&AppEvent::MountStarting {
            path: "C:\\RomM".into(),
        });
        assert!(state.foreign_entries.is_empty());
        assert!(state.foreign_root.is_none());
        assert!(state.foreign_removal.is_none());
    }

    /// The cleanup command deletes links only and republishes what remains.
    #[cfg(unix)]
    #[test]
    fn remove_foreign_links_reports_and_preserves_real_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("mount");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("user.srm"), b"keep").unwrap();
        std::os::unix::fs::symlink(&root, root.join("leftover")).unwrap();
        let (sink, events) = rommfs_core::events::channel();
        let mut worker = Worker::new(sink);
        worker.remove_foreign_links(&root);
        assert_eq!(std::fs::read(root.join("user.srm")).unwrap(), b"keep");
        assert!(std::fs::symlink_metadata(root.join("leftover")).is_err());
        let events: Vec<_> = events.try_iter().collect();
        let report = events
            .iter()
            .find_map(|e| match e {
                AppEvent::ForeignRootEntries {
                    entries, removal, ..
                } => Some((entries, removal)),
                _ => None,
            })
            .expect("cleanup must republish the foreign entry list");
        assert_eq!(report.1, &Some(Ok(1)));
        assert_eq!(report.0.len(), 1);
        assert_eq!(report.0[0].kind, ForeignKind::File);
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

    #[test]
    fn connect_clears_a_stale_mount_failure() {
        let mut state = UiState::new(32);
        state.apply(&AppEvent::MountFailed {
            reason: "connect to a RomM server before mounting".into(),
        });
        assert_eq!(state.mount, MountState::Failed);
        assert!(state.mount_error.is_some());

        state.apply(&AppEvent::Connected);
        assert_eq!(state.mount, MountState::NotMounted);
        assert!(state.mount_error.is_none());
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

    #[cfg(windows)]
    #[test]
    fn mount_root_creation_rejects_junction_ancestors() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        let link = tmp.path().join("link");
        std::fs::create_dir(&target).unwrap();
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .output()
            .expect("mklink junction");
        assert!(
            output.status.success(),
            "junction creation failed: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(check_mount_root(&link.join("roms"), "server").is_err());
        assert!(!target.join("roms").exists());
    }

    struct UnusedSource;

    #[test]
    fn stopping_cancels_artwork_and_discards_late_metadata_results() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let (sink, events) = rommfs_core::events::channel();
        let mut worker = Worker::new(sink);
        let active = Arc::new(AtomicBool::new(true));
        let (tx, rx) = mpsc::channel();
        worker.metadata_active = Some(active.clone());
        worker.metadata_job = Some(rx);
        worker.stop_mount();
        assert!(!active.load(Ordering::Acquire));
        assert!(tx
            .send(Err(Error::Transport("old session failure".into())))
            .is_err());
        worker.poll_metadata();
        let mut state = UiState::new(32);
        while let Ok(event) = events.try_recv() {
            state.apply(&event);
        }
        assert!(!state.frontend_metadata_pending);
        assert!(state.frontend_metadata_error.is_none());
    }

    #[test]
    fn changing_accounts_drains_catalogue_and_artwork_threads_before_returning() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let (sink, _events) = rommfs_core::events::channel();
        let mut worker = Worker::new(sink);
        let completed = Arc::new(AtomicUsize::new(0));
        for artwork in [true, false] {
            let active = Arc::new(AtomicBool::new(true));
            let flag = active.clone();
            let done = completed.clone();
            let handle = std::thread::spawn(move || {
                while flag.load(Ordering::Acquire) {
                    std::thread::yield_now();
                }
                done.fetch_add(1, Ordering::SeqCst);
            });
            if artwork {
                worker.metadata_active = Some(active);
                worker.metadata_handle = Some(handle);
            } else {
                worker.refresh_active = Some(active);
                worker.refresh_handle = Some(handle);
            }
        }
        worker.clear_active_connection();
        assert_eq!(completed.load(Ordering::SeqCst), 2);
        assert!(worker.metadata_handle.is_none());
        assert!(worker.refresh_handle.is_none());
    }

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
        assert!(matches!(
            events.try_recv(),
            Ok(AppEvent::SignInRequired { .. })
        ));
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
        assert!(problem
            .unwrap()
            .contains("verify the account, save folder, and game mappings first"));
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
            metadata: Default::default(),
            fs_path: String::new(),
            files: vec![RomFileDto {
                id: 70,
                file_name: "Game.gb".into(),
                file_size_bytes: 1,
                last_modified: None,
                crc_hash: None,
                md5_hash: None,
                sha1_hash: None,
                is_top_level: true,
                file_path: String::new(),
                category: None,
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
            .save_files(
                &scope,
                ConsentSettings {
                    enabled: true,
                    debounce_secs: 1,
                },
            )
            .unwrap();

        let path = PathBuf::from("gb/Game.srm");
        let mapping = rommfs_core::save_sync::SaveMapping {
            rom_key: rommfs_core::save_sync::save_file_key(&scope.server_id, 7, &path).unwrap(),
            profile: rommfs_core::save_sync::SaveProfile::ordinary_files(),
            target_path: scope.effective_saves_root.join(&path),
            relative_path: path,
            visible_rom_name: "Game.gb".into(),
            legacy_compatible: true,
        };
        let mut journal = SaveSyncJournal::open(
            settings_dir.join("save-sync-journal.db"),
            settings_dir.join("save-sync-snapshots"),
            scope.clone(),
        )
        .unwrap();
        journal
            .reconcile_mappings(&rommfs_core::save_sync::MappingReport {
                mappings: vec![mapping.clone()],
                ..Default::default()
            })
            .unwrap();
        journal
            .confirm_mapping(&mapping.rom_key, &mapping.relative_path)
            .unwrap();
        drop(journal);

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
            worker: std::sync::Mutex::new(Some(worker)),
            save_sync_gate: gate,
            connection_gate: SaveSyncCommandGate::new(),
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
                auxiliary_job: None,
                startup_restoring: false,
                startup_requires_mount: false,
                save_file_rules: None,
                installer: installer::InstallerJob::default(),
                metadata_job: None,
                metadata_handle: None,
                metadata_active: None,
                metadata_source: None,
                metadata_ready: None,
                metadata_wait_reported: false,
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
                connection_gate: SaveSyncCommandGate::new(),
                current_connection_epoch: 0,
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
                connection_epoch: 0,
            })
            .unwrap();
        worker_thread.join().unwrap();
    }
    #[test]
    fn catalogue_pages_are_fetched_once_and_warm_remounts_make_no_http_requests() {
        use rommfs_fixture::contract;
        let tmp = tempfile::tempdir().unwrap();
        let fx = FixtureServer::start();
        fx.on(
            "POST",
            "/api/token",
            ResponseSpec::Json {
                status: 200,
                body: contract::token_ok(),
            },
        );
        fx.on("GET", "/api/users/me", ResponseSpec::Json { status: 200, body: serde_json::json!({"id":7,"oauth_scopes":["me.read","assets.read","assets.write"]}).to_string() });
        let slugs: Vec<_> = (0..25).map(|n| format!("platform{n}")).collect();
        let platforms: Vec<_> = slugs
            .iter()
            .enumerate()
            .map(|(n, slug)| (n as i64, slug.as_str(), slug.as_str(), slug.as_str()))
            .collect();
        fx.on(
            "GET",
            "/api/platforms",
            ResponseSpec::Json {
                status: 200,
                body: contract::platforms(&platforms),
            },
        );
        let items: Vec<_> = (0..2448)
            .map(|n| {
                contract::rom(
                    n + 1,
                    &slugs[n as usize % 25],
                    &format!("game{n}.bin"),
                    10,
                    "hash",
                )
            })
            .collect();
        for offset in (0..2448).step_by(200) {
            fx.on(
                "GET",
                &format!("/api/roms?limit=200&offset={offset}&"),
                ResponseSpec::Json {
                    status: 200,
                    body: contract::roms_page(
                        &items[offset..(offset + 200).min(items.len())],
                        items.len(),
                        200,
                        offset,
                    ),
                },
            );
        }
        let (sink, events) = rommfs_core::events::channel();
        let mut worker = Worker::new(sink);
        worker.rom_cache_directory = Some(tmp.path().join("rom-cache"));
        worker.catalogue_store =
            Some(CatalogueStore::open(&tmp.path().join("catalogue.db")).unwrap());
        worker.connect(fx.url(), "user", "password");
        assert_eq!(worker.catalogue.as_ref().unwrap().entries.len(), 2448);
        assert_eq!(fx.count("GET", "/api/platforms"), 1);
        for offset in (0..2448).step_by(200) {
            assert_eq!(
                fx.count("GET", &format!("/api/roms?limit=200&offset={offset}&")),
                1
            );
        }
        let root = tmp.path().join("roms");
        for _ in 0..2 {
            worker.start_mount(root.to_str().unwrap());
            worker.stop_mount();
        }
        assert_eq!(fx.count("GET", "/api/platforms"), 1);
        assert_eq!(fx.count("GET", "/api/roms/"), 0);
        assert!(
            worker.catalogue.is_some(),
            "mount construction retains the reusable snapshot"
        );
        let snapshot = worker.source_snapshot.clone().unwrap();
        worker.catalogue = None;
        assert!(worker.restore_catalogue());
        assert_eq!(
            worker.source_snapshot.as_ref().unwrap().refreshed_at,
            snapshot.refreshed_at
        );
        // A failed later page retains the whole preceding complete snapshot on
        // disk and in memory, rather than publishing the first successful page.
        fx.on(
            "GET",
            "/api/roms?limit=200&offset=200&",
            ResponseSpec::Json {
                status: 500,
                body: "{}".into(),
            },
        );
        worker.refresh_catalogue();
        let result = worker
            .refresh_result
            .take()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        worker.finish_catalogue_refresh(result);
        assert_eq!(worker.catalogue.as_ref().unwrap().entries.len(), 2448);
        assert_eq!(
            worker
                .catalogue_store
                .as_ref()
                .unwrap()
                .load(worker.server_id.as_deref().unwrap(), "7")
                .unwrap()
                .unwrap()
                .roms
                .len(),
            2448
        );
        assert!(events
            .try_iter()
            .any(|e| matches!(e,AppEvent::CatalogueFailed{reason} if reason.contains("500"))));
    }

    #[test]
    fn browser_pairing_cancelled_during_account_verification_never_connects_or_remembers() {
        let fixture = FixtureServer::start();
        fixture.on("POST", "/api/auth/device/init", ResponseSpec::Json { status: 201, body: serde_json::json!({
            "device_code":"private-code", "user_code":"ABCD", "verification_path":"/pair/device",
            "verification_path_complete":"/pair/device?user_code=ABCD", "expires_in":600, "interval":1
        }).to_string() });
        fixture.on("POST", "/api/auth/device/token", ResponseSpec::Json { status: 200, body: serde_json::json!({
            "access_token":"secret-token", "device_id":"device", "scopes":["me.read"], "expires_at":null
        }).to_string() });
        let barrier = FixtureBodyBarrier::new();
        fixture.on(
            "GET",
            "/api/users/me",
            ResponseSpec::HeldBytes {
                status: 200,
                bytes: br#"{"id":42,"oauth_scopes":["me.read"]}"#.to_vec(),
                barrier: barrier.clone(),
            },
        );
        let directory = tempfile::tempdir().unwrap();
        let settings_path = directory.path().join("connection.db");
        let gate = SaveSyncCommandGate::new();
        let worker_gate = gate.clone();
        let worker_path = settings_path.clone();
        let url = fixture.url().to_owned();
        let (sink, events) = rommfs_core::events::channel();
        let worker = std::thread::spawn(move || {
            let mut worker = Worker::new(sink);
            worker.connection_gate = worker_gate;
            worker.connection_settings = Some(ConnectionSettings::open(&worker_path).unwrap());
            worker.begin_pairing(&url);
            worker.pairing.as_mut().unwrap().next_poll = Instant::now();
            worker.poll_pairing();
            assert!(worker.client.is_none());
            assert!(worker.preferences.is_none());
            assert!(worker
                .connection_settings
                .as_ref()
                .unwrap()
                .load()
                .unwrap()
                .is_none());
            worker.cancel_pairing();
        });
        assert!(barrier.wait_until_blocked(Duration::from_secs(5)));
        gate.invalidate();
        barrier.release();
        worker.join().unwrap();
        let events: Vec<_> = events.try_iter().collect();
        assert!(events.iter().any(
            |e| matches!(e, AppEvent::PairingEnded { reason: Some(r) } if r.contains("cancelled"))
        ));
        assert!(!events.iter().any(|e| matches!(
            e,
            AppEvent::Connected | AppEvent::ConnectFailed { .. } | AppEvent::CatalogueLoaded { .. }
        )));
        assert_eq!(fixture.count_requests("GET", "/api/platforms"), 0);
        assert!(!format!("{events:?}").contains("secret-token"));
    }

    #[test]
    fn save_sync_changes_do_not_cancel_browser_pairing_and_denial_is_visible() {
        let fixture = FixtureServer::start();
        fixture.on("POST", "/api/auth/device/init", ResponseSpec::Json { status:201, body:serde_json::json!({
            "device_code":"private-code", "user_code":"ABCD", "verification_path":"/pair/device",
            "verification_path_complete":"/pair/device?user_code=ABCD", "expires_in":600, "interval":1
        }).to_string() });
        fixture.on(
            "POST",
            "/api/auth/device/token",
            ResponseSpec::Json {
                status: 400,
                body: r#"{"detail":"access_denied"}"#.into(),
            },
        );
        let (sink, events) = rommfs_core::events::channel();
        let mut worker = Worker::new(sink);
        worker.begin_pairing(fixture.url());
        worker.save_sync_gate.invalidate();
        worker.pairing.as_mut().unwrap().next_poll = Instant::now();
        worker.poll_pairing();
        assert_eq!(fixture.count_requests("POST", "/api/auth/device/token"), 1);
        assert!(worker.client.is_none());
        assert!(worker.pairing.is_none());
        assert!(events.try_iter().any(
            |e| matches!(e, AppEvent::PairingEnded { reason:Some(r) } if r.contains("denied"))
        ));
    }

    #[test]
    fn remembered_preferences_never_cross_accounts_or_servers() {
        let directory = tempfile::tempdir().unwrap();
        let (sink, _) = rommfs_core::events::channel();
        let mut worker = Worker::new(sink);
        worker.connection_settings =
            Some(ConnectionSettings::open(&directory.path().join("connection.db")).unwrap());
        let preferences = ConnectionPreferences {
            server: "https://romm.example".into(),
            account_id: 42,
            device_identifier: "id".into(),
            mount_root: Some(directory.path().join("roms")),
            integration: IntegrationMode::Generic,
            retrobat_root: None,
            start_at_login: false,
            save_root: None,
        };
        worker
            .connection_settings
            .as_ref()
            .unwrap()
            .save(&preferences)
            .unwrap();
        worker.preferences = Some(preferences.clone());
        worker.forget_other_account("https://romm.example/", Some(42));
        assert!(worker.preferences.is_some());
        worker.forget_other_account("https://romm.example", Some(43));
        assert!(worker.preferences.is_none());
        assert!(worker
            .connection_settings
            .as_ref()
            .unwrap()
            .load()
            .unwrap()
            .is_none());
        worker.preferences = Some(preferences);
        worker.forget_other_account("https://another.example", Some(42));
        assert!(worker.preferences.is_none());
    }

    #[test]
    fn invalid_layout_selection_keeps_the_previous_projection_and_preferences() {
        let directory = tempfile::tempdir().unwrap();
        let (sink, events) = rommfs_core::events::channel();
        let mut worker = Worker::new(sink);
        worker.preferences = Some(ConnectionPreferences {
            server: "s".into(),
            account_id: 42,
            device_identifier: "id".into(),
            mount_root: None,
            integration: IntegrationMode::Generic,
            retrobat_root: None,
            start_at_login: false,
            save_root: None,
        });
        worker.set_integration(IntegrationMode::RetroBat, Some(directory.path().into()));
        assert_eq!(
            worker.preferences.as_ref().unwrap().integration,
            IntegrationMode::Generic
        );
        assert!(events.try_iter().any(|e| matches!(
            e,
            AppEvent::IntegrationUpdated {
                mode: IntegrationMode::Generic,
                error: Some(_),
                ..
            }
        )));
    }

    #[test]
    fn cancelled_refresh_cannot_emit_late_events_or_replace_the_new_session() {
        let (sink, events) = rommfs_core::events::channel();
        let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let scoped = sink.clone().scoped(active.clone());
        let mut worker = Worker::new(sink);
        worker.refresh_active = Some(active);
        worker.cancel_catalogue_refresh();
        scoped.emit(AppEvent::CatalogueProgress {
            operation_id: 99,
            stage: "old account".into(),
            completed: 200,
            total: Some(2448),
            elapsed_ms: 100,
        });
        assert!(events.try_recv().is_err());
    }
}
