//! Event-derived UI state and the summaries rendered by the window.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
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
            Self::SetupNeeded => "No games folder chosen yet",
            Self::Stopped => "Games folder stopped",
            Self::Starting => "Preparing games…",
            Self::Ready => "Available",
            Self::Failed => "Couldn't start the games folder",
        }
    }
}

/// Artwork checking is only the first stage; RetroBat must also receive the update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrontendMetadataSummary {
    NotStarted,
    CheckingArtwork,
    Publishing,
    Waiting,
    UpdatedWithIssues,
    Failed,
    UpToDate,
}

/// Where one step of the RetroBat flow stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepStatus {
    Done,
    /// Actionable now.
    Ready,
    Running,
    /// The last attempt failed; the action is a retry.
    Failed,
    /// Unavailable until something else happens (the reason).
    Blocked(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetroBatStep {
    Install,
    Link,
    Support,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetroBatSteps {
    pub install: StepStatus,
    pub link: StepStatus,
    pub support: StepStatus,
}

impl RetroBatSteps {
    /// The one step whose action is highlighted: the first that is neither
    /// done nor blocked. Nothing is highlighted while an operation runs.
    pub fn primary(&self) -> Option<RetroBatStep> {
        [
            (RetroBatStep::Install, self.install),
            (RetroBatStep::Link, self.link),
            (RetroBatStep::Support, self.support),
        ]
        .into_iter()
        .find_map(|(step, status)| match status {
            StepStatus::Running => Some(None),
            StepStatus::Ready | StepStatus::Failed => Some(Some(step)),
            _ => None,
        })
        .flatten()
    }
}

/// Parses a trailing "N / M" count out of a status line, for determinate
/// progress ("Auxiliary files: 32 / 146").
pub fn parse_progress_counts(detail: &str) -> Option<(u64, u64)> {
    let (left, right) = detail.split_once('/')?;
    let done = left
        .trim_end()
        .rsplit(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()?;
    let total = right
        .trim_start()
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()?;
    (total > 0).then_some((done, total))
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
            Self::Off => "Automatic save sync is off",
            Self::NeedsSetup => "Save sync needs setup",
            Self::Checking => "Checking for changes…",
            Self::NoLinkedFiles => "Nothing to sync yet",
            Self::Reconciling => "Checking for changes…",
            Self::Syncing => "Syncing saves…",
            Self::NeedsAttention => "Save sync needs attention",
            Self::Paused => "Save sync paused",
            Self::UpToDate => "Tracked saves are up to date",
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
    pub updated_at: u64,
    pub phase: String,
    pub detail: Option<String>,
}

/// How many download rows are kept on screen; oldest finished rows are
/// dropped first (the log carries the durable record anyway).
const MAX_DOWNLOAD_ROWS: usize = 128;

#[derive(PartialEq, Eq)]
struct SaveScanScope {
    server: Option<String>,
    account: Option<i64>,
    root: Option<String>,
}

/// Everything the window draws — derived ONLY from real worker events.
pub struct UiState {
    pub closing: bool,
    pub auxiliary_running: bool,
    pub start_at_login: bool,
    pub startup_error: Option<String>,
    pub save_file_games: Vec<(i64, String)>,
    pub save_file_unmatched: Vec<UnmatchedFile>,
    pub auxiliary_detail: Option<String>,
    pub auxiliary_error: Option<String>,
    /// Result of the last completed support-file install.
    pub auxiliary_summary: Option<rommfs_core::events::AuxiliarySummary>,
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
    /// True once setup for the selected installation has actually succeeded
    /// (this session, or a verified setup record found on disk).
    pub retrobat_setup_ok: bool,
    /// An earlier build's setup record must be migrated by running setup.
    pub retrobat_setup_migration: bool,
    pub retrobat_setup_issues: Vec<rommfs_core::retrobat::SkippedSystem>,
    pub retrobat_setup_detail: Option<String>,
    pub retrobat_setup_error: Option<String>,
    /// Systems the last setup run could not link, each with its reason.
    pub retrobat_setup_skipped: Vec<rommfs_core::retrobat::SkippedSystem>,
    pub frontend_metadata_pending: bool,
    pub frontend_metadata_detail: Option<String>,
    pub frontend_metadata_error: Option<String>,
    /// Live counts of the current metadata pass, and the pass identity that
    /// produced them (a stale pass is ignored).
    pub frontend_metadata_progress: Option<(usize, usize)>,
    frontend_metadata_job_id: u64,
    /// When the last metadata pass finished cleanly, for "up to date · N ago".
    pub frontend_metadata_updated_at: Option<u64>,
    /// The session holds the scopes play-data sync needs.
    pub play_data_available: bool,
    pub play_data_permission_denied: bool,
    /// Consent is on for the connected server and account.
    pub play_data_enabled: bool,
    pub play_data_problem: Option<String>,
    /// "Hide RomM games you already have" is on (default), and how many entries
    /// the last metadata pass hid.
    pub hide_copies_enabled: bool,
    pub hide_copies_count: usize,
    pub pairing_code: Option<String>,
    pub conn: ConnectionState,
    pub conn_error: Option<String>,
    pub mount: MountState,
    pub mount_stopping: bool,
    pub save_sync_checking: bool,
    pub save_scan_at: Option<u64>,
    save_scan_scope: Option<SaveScanScope>,
    pub folder_open_error: Option<String>,
    pub mount_path: Option<String>,
    pub mount_error: Option<String>,
    /// Classified foreign entries blocking a mount/setup, for remediation UI.
    pub foreign_root: Option<String>,
    pub foreign_entries: Vec<ForeignEntry>,
    /// Set after a "remove leftover links" pass ran.
    pub foreign_removal: Option<std::result::Result<usize, String>>,
    pub downloads: Vec<DownloadView>,
    pub catalogue_progress: Option<(String, u64, Option<u64>, u64)>,
    pub mount_progress: Option<(String, u64, Option<u64>, u64)>,
    pub catalogue_refreshed_at: Option<u64>,
    pub catalogue_pending_remount: bool,
    catalogue_operation_id: u64,
    pub catalogue: Option<(usize, usize, usize, usize)>, // platforms, games, files, skipped
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
    requested_save_sync_consent: Option<(u64, bool)>,
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
    /// Discovered memory-card folders, their per-folder consent, and any card
    /// whose local and remote copies both changed.
    pub memory_card_folders: Vec<MemoryCardFolderStatus>,
    pub memory_card_conflicts: Vec<MemoryCardConflictStatus>,
    pub memory_cards_available: bool,
    pub memory_cards_problem: Option<String>,
    pub log: LogBuffer,
}

impl UiState {
    pub fn retrobat_ready_for(
        &self,
        installation: Option<&std::path::Path>,
        games: &std::path::Path,
    ) -> bool {
        self.retrobat_setup_ok
            && installation.is_some_and(|path| {
                self.retrobat_root
                    .as_deref()
                    .is_some_and(|root| std::path::Path::new(root) == path)
            })
            && self
                .configured_mount_root
                .as_deref()
                .is_some_and(|root| std::path::Path::new(root) == games)
    }

    pub fn new(log_cap: usize) -> Self {
        Self {
            closing: false,
            start_at_login: false,
            auxiliary_running: false,
            auxiliary_detail: None,
            auxiliary_error: None,
            auxiliary_summary: None,
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
            retrobat_setup_ok: false,
            retrobat_setup_migration: false,
            retrobat_setup_issues: Vec::new(),
            retrobat_setup_detail: None,
            retrobat_setup_error: None,
            retrobat_setup_skipped: Vec::new(),
            frontend_metadata_pending: false,
            frontend_metadata_detail: None,
            frontend_metadata_error: None,
            frontend_metadata_progress: None,
            frontend_metadata_job_id: 0,
            frontend_metadata_updated_at: None,
            play_data_available: false,
            play_data_permission_denied: false,
            play_data_enabled: false,
            play_data_problem: None,
            hide_copies_enabled: true,
            hide_copies_count: 0,
            pairing_code: None,
            conn: ConnectionState::Idle,
            conn_error: None,
            mount: MountState::NotMounted,
            mount_stopping: false,
            save_sync_checking: false,
            save_scan_at: None,
            save_scan_scope: None,
            folder_open_error: None,
            mount_path: None,
            mount_error: None,
            foreign_root: None,
            foreign_entries: Vec::new(),
            foreign_removal: None,
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
            requested_save_sync_consent: None,
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
            memory_card_folders: Vec::new(),
            memory_card_conflicts: Vec::new(),
            memory_cards_available: false,
            memory_cards_problem: None,
            log: LogBuffer::new(log_cap),
        }
    }

    /// Apply one worker event. This is THE transition table the UI uses.
    pub fn apply_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::ShutdownStarting => self.closing = true,
            AppEvent::SaveSyncCheckStarted { session_id } => {
                if *session_id > self.save_sync_session_id {
                    self.save_sync_session_id = *session_id;
                    self.save_sync_checking = true;
                    self.save_sync_transfers
                        .retain(|transfer| transfer.phase == "verified");
                }
            }
            AppEvent::SaveSyncScanFinished {
                session_id,
                completed_at,
            } => {
                if *session_id == self.save_sync_session_id {
                    self.save_scan_at = Some(*completed_at);
                }
            }
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
                summary,
            } => {
                // Auxiliary support-file downloads are their own operation:
                // their status must not masquerade as (or overwrite) RetroBat
                // setup state.
                self.auxiliary_running = *running;
                self.auxiliary_detail = Some(detail.clone());
                self.auxiliary_error = error.clone();
                self.auxiliary_summary = summary.clone();
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
                job_id,
                pending,
                detail,
                error,
            } => {
                let accepted = if *pending {
                    // Same active pass, or a strictly newer pass after the last
                    // one finished. An older pass is stale.
                    (self.frontend_metadata_pending && *job_id == self.frontend_metadata_job_id)
                        || (!self.frontend_metadata_pending
                            && *job_id > self.frontend_metadata_job_id)
                } else {
                    // A finish/stop belongs to the newest pass only. It is
                    // accepted even right after an invalidation, so the closing
                    // detail line still lands.
                    *job_id == self.frontend_metadata_job_id
                };
                if accepted {
                    self.frontend_metadata_job_id = (*job_id).max(self.frontend_metadata_job_id);
                    self.frontend_metadata_pending = *pending;
                    self.frontend_metadata_detail = Some(detail.clone());
                    self.frontend_metadata_error = error.clone();
                    if !pending {
                        self.frontend_metadata_progress = None;
                        if error.is_none() || detail.starts_with("Updated with issues") {
                            self.frontend_metadata_updated_at =
                                Some(rommfs_core::cache::now_unix_secs());
                        }
                    }
                }
            }
            AppEvent::FrontendMetadataProgress {
                job_id,
                completed,
                total,
            } => {
                // Progress is only trusted for the active pass, and the count
                // only moves forward within it.
                if self.frontend_metadata_pending
                    && *job_id == self.frontend_metadata_job_id
                    && self
                        .frontend_metadata_progress
                        .is_none_or(|(previous, _)| *completed >= previous)
                {
                    self.frontend_metadata_progress = Some((*completed, *total));
                }
            }
            AppEvent::FrontendMetadataInvalidated { job_id } => {
                // The pass was cancelled/stopped: drop it so a late callback or
                // publication status for the same id cannot revive it.
                if *job_id == self.frontend_metadata_job_id {
                    self.frontend_metadata_pending = false;
                    self.frontend_metadata_progress = None;
                    self.frontend_metadata_error = None;
                    self.frontend_metadata_detail = None;
                }
            }
            AppEvent::PlayDataUpdated {
                available,
                enabled,
                permission_denied,
                problem,
            } => {
                self.play_data_available = *available;
                self.play_data_permission_denied = *permission_denied;
                self.play_data_enabled = *enabled;
                self.play_data_problem = problem.clone();
            }
            AppEvent::HideCopies { enabled, hidden } => {
                self.hide_copies_enabled = *enabled;
                self.hide_copies_count = *hidden;
            }
            AppEvent::RetroBatSetup {
                running,
                detail,
                skipped,
                error,
            } => {
                self.retrobat_setup_running = *running;
                self.retrobat_setup_detail = Some(detail.clone());
                self.retrobat_setup_error = error.clone();
                self.retrobat_setup_skipped = skipped.clone();
                if *running || error.is_some() {
                    self.retrobat_setup_ok = false;
                }
                if *running {
                    // A new setup attempt supersedes earlier messages.
                    self.installer_error = None;
                    self.foreign_root = None;
                    self.foreign_entries.clear();
                    self.foreign_removal = None;
                } else if error.is_none() {
                    self.integration_notice = None;
                }
            }
            AppEvent::IntegrationUpdated {
                mode,
                retrobat_root,
                setup_complete,
                setup_migration,
                setup_issues,
                error,
            } => {
                let changed = self.integration != *mode;
                let root_changed = self.retrobat_root != *retrobat_root;
                self.integration = *mode;
                self.retrobat_root = retrobat_root.clone();
                self.retrobat_setup_migration = *setup_migration;
                self.retrobat_setup_issues = setup_issues.clone();
                if error.is_none() && retrobat_root.is_some() {
                    // A usable installation is selected: the installer's
                    // download/launch problems no longer apply.
                    self.installer_error = None;
                }
                self.retrobat_setup_ok = !*setup_migration
                    && *mode == IntegrationMode::RetroBat
                    && retrobat_root.is_some()
                    && *setup_complete;
                if root_changed {
                    // Results belong to the installation that produced them.
                    self.retrobat_setup_error = None;
                    self.retrobat_setup_detail = None;
                    self.auxiliary_error = None;
                    self.auxiliary_detail = None;
                    self.auxiliary_summary = None;
                }
                self.integration_error = error.clone();
                self.integration_notice = if error.is_none() && changed {
                    match mode {
                        // The layout is only reported as active once setup
                        // has linked it; the Settings steps say what is next.
                        IntegrationMode::RetroBat => None,
                        IntegrationMode::Generic => Some("Generic layout is active.".into()),
                    }
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
            AppEvent::MountRootConfigured { server, mount_root } => {
                self.connected_server = Some(server.clone());
                self.configured_mount_root = Some(mount_root.clone());
                // Without a live mount the applied path is the current one:
                // the folder display, Start and Retry must all follow the
                // edit. A running mount keeps its real path until it stops.
                if !matches!(self.mount, MountState::Mounted | MountState::Mounting) {
                    self.mount_path = Some(mount_root.clone());
                }
            }
            AppEvent::SignedOut => {
                self.save_sync_enabled = false;
                self.requested_save_sync_consent = None;
                self.save_sync_checking = false;
                self.save_scan_at = None;
                self.save_scan_scope = None;
                self.mount_stopping = false;
                self.conn = ConnectionState::Idle;
                self.conn_error = None;
                self.catalogue = None;
                self.catalogue_refreshed_at = None;
                self.catalogue_pending_remount = false;
                self.catalogue_progress = None;
                self.downloads.clear();
                self.pairing_code = None;
                self.connected_server = None;
                self.configured_mount_root = None;
                // The mount belonged to the session that just ended — its
                // path must never present as the next session's folder.
                self.mount = MountState::NotMounted;
                self.mount_path = None;
                self.mount_error = None;
                self.mount_progress = None;
                self.play_data_available = false;
                self.play_data_permission_denied = false;
                self.play_data_enabled = false;
                self.play_data_problem = None;
                // Metadata belongs to the session that just ended. The job
                // high-water mark is kept so a stale pass cannot revive.
                self.frontend_metadata_pending = false;
                self.frontend_metadata_progress = None;
                self.frontend_metadata_updated_at = None;
            }
            AppEvent::PairingReady { user_code, .. } => {
                self.pairing_code = Some(user_code.clone());
            }
            AppEvent::PairingEnded { reason } => {
                self.pairing_code = None;
                if let Some(reason) = reason {
                    self.conn = ConnectionState::Failed;
                    self.conn_error = Some(reason.clone());
                }
            }
            AppEvent::Connecting => {
                self.save_scan_at = None;
                self.save_scan_scope = None;
                self.folder_open_error = None;
                self.conn = ConnectionState::Connecting;
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
                self.mount_path = None;
                self.save_sync_export_errors.clear();
                self.save_sync_export_feedback.clear();
                self.save_sync_enabled = false;
                self.requested_save_sync_consent = None;
                self.save_sync_available = false;
                self.save_sync_account_id = None;
                self.save_sync_server_id = None;
                self.save_sync_effective_saves_root = None;
                self.save_sync_mapped_targets = 0;
                self.save_sync_catalogue_unmapped = 0;
                self.save_sync_existing_saves = None;
                self.memory_card_folders.clear();
                self.memory_card_conflicts.clear();
                self.memory_cards_available = false;
                self.memory_cards_problem = None;
                // A new connection attempt is a new scope: drop the previous
                // metadata stage/age (the job high-water mark stays).
                self.frontend_metadata_pending = false;
                self.frontend_metadata_progress = None;
                self.frontend_metadata_updated_at = None;
            }
            AppEvent::Connected => {
                self.conn = ConnectionState::Connected;
                self.conn_error = None;
                // A failed mount attempt belongs to the previous connection;
                // keep it from rendering as a stale banner after reconnect.
                self.mount = MountState::NotMounted;
                self.mount_error = None;
            }
            AppEvent::ConnectFailed { reason } => {
                self.conn = ConnectionState::Failed;
                self.conn_error = Some(reason.clone());
            }
            AppEvent::SignInRequired { reason } => {
                self.conn = ConnectionState::SignInRequired;
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
                games,
                files,
                skipped_unsupported,
            } => {
                self.catalogue = Some((*platforms, *games, *files, *skipped_unsupported));
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
                self.mount_stopping = false;
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
            AppEvent::MountStopping => self.mount_stopping = true,
            AppEvent::MountStopped => {
                self.mount_stopping = false;
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
                self.save_sync_checking = false;
                let scan_scope = SaveScanScope {
                    server: server_id.clone(),
                    account: *account_id,
                    root: effective_saves_root.clone(),
                };
                if self.save_scan_scope.as_ref() != Some(&scan_scope) {
                    self.save_scan_at = None;
                    self.save_scan_scope = Some(scan_scope);
                }
                let scan_at = existing_saves
                    .as_ref()
                    .filter(|preview| {
                        matches!(
                            preview.status,
                            rommfs_core::save_sync::ExistingSaveScanStatus::Complete
                        )
                    })
                    .map(|_| rommfs_core::cache::now_unix_secs());
                if scan_at.is_some() {
                    self.save_scan_at = scan_at;
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
                self.save_sync_problem = selected_problem
                    .as_deref()
                    .map(|raw| crate::problem::UserProblem::from_text("Save sync", raw))
                    .and_then(|problem| problem.cause);
                if self
                    .requested_save_sync_consent
                    .is_none_or(|(requested_in, _)| *session_id > requested_in)
                {
                    self.save_sync_enabled = *enabled;
                    self.requested_save_sync_consent = None;
                }
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
                self.save_sync_transfers
                    .retain(|transfer| transfer.revision != *revision);
                self.save_sync_transfers.push(SaveSyncTransferView {
                    rom_id: *rom_id,
                    revision: revision.clone(),
                    updated_at: rommfs_core::cache::now_unix_secs(),
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
                    self.save_sync_checking = true;
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
                    self.memory_card_folders.clear();
                    self.memory_card_conflicts.clear();
                    self.memory_cards_available = false;
                    self.memory_cards_problem = None;
                }
            }
            AppEvent::SaveSyncAuthenticationRequired { session_id } => {
                if *session_id == self.save_sync_session_id {
                    self.save_sync_authentication_required = true;
                    self.conn = ConnectionState::SignInRequired;
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
            AppEvent::MemoryCardsUpdated {
                session_id,
                available,
                folders,
                conflicts,
                problem,
            } => {
                if *session_id == self.save_sync_session_id {
                    self.memory_cards_available = *available;
                    self.memory_card_folders = folders.clone();
                    self.memory_card_conflicts = conflicts.clone();
                    self.memory_cards_problem = problem.clone();
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

    /// The three-step RetroBat flow derived from real state (install →
    /// connect to the games folder → optional support files).
    pub fn retrobat_steps(&self) -> RetroBatSteps {
        let folder_running = matches!(self.mount, MountState::Mounted | MountState::Mounting);
        let connected = self.conn == ConnectionState::Connected;
        let install = if self.installer_running {
            StepStatus::Running
        } else if self.retrobat_root.is_some() {
            StepStatus::Done
        } else {
            StepStatus::Ready
        };
        let link = if self.retrobat_root.is_none() {
            StepStatus::Blocked("Install RetroBat first.")
        } else if self.retrobat_setup_running {
            StepStatus::Running
        } else if self.retrobat_setup_ok {
            StepStatus::Done
        } else if !connected {
            StepStatus::Blocked("Connect to RomM first.")
        } else if folder_running {
            StepStatus::Blocked("Stop the games folder first.")
        } else if self.retrobat_setup_error.is_some() {
            StepStatus::Failed
        } else {
            StepStatus::Ready
        };
        let support = if self.auxiliary_running {
            StepStatus::Running
        } else if self.retrobat_root.is_none() {
            StepStatus::Blocked("Install RetroBat first.")
        } else if !connected {
            StepStatus::Blocked("Connect to RomM first.")
        } else if self.catalogue.is_none() {
            StepStatus::Blocked("Wait for your game library to load.")
        } else if folder_running {
            StepStatus::Blocked("Stop the games folder first.")
        } else if self.auxiliary_error.is_some() {
            StepStatus::Failed
        } else if self.auxiliary_summary.is_some() {
            StepStatus::Done
        } else {
            StepStatus::Ready
        };
        RetroBatSteps {
            install,
            link,
            support,
        }
    }

    /// Whether the games folder is usable right now, derived only from real
    /// mount events — never from connection, configuration, or sync state.
    pub fn folder_summary(&self) -> FolderSummary {
        match self.mount {
            MountState::Mounted => FolderSummary::Ready,
            MountState::Mounting => FolderSummary::Starting,
            MountState::Failed => FolderSummary::Failed,
            MountState::NotMounted => {
                // A loaded catalogue means a folder was already set up, even
                // when its path hasn't been reported to this session.
                if self.mount_path.is_some()
                    || self.configured_mount_root.is_some()
                    || self.catalogue.is_some()
                {
                    FolderSummary::Stopped
                } else {
                    FolderSummary::SetupNeeded
                }
            }
        }
    }

    /// Structural exclusions are skipped; eligible unlinked saves stay actionable.
    pub fn save_files_ignored(&self) -> usize {
        self.save_file_unmatched
            .iter()
            .filter(|file| !file.mappable)
            .count()
    }

    pub fn frontend_metadata_summary(&self) -> FrontendMetadataSummary {
        if self.frontend_metadata_pending {
            if self.frontend_metadata_error.is_some()
                || self
                    .frontend_metadata_detail
                    .as_deref()
                    .is_some_and(|detail| detail.starts_with("Waiting for "))
            {
                FrontendMetadataSummary::Waiting
            } else if self
                .frontend_metadata_progress
                .is_some_and(|(completed, total)| completed >= total)
            {
                FrontendMetadataSummary::Publishing
            } else {
                FrontendMetadataSummary::CheckingArtwork
            }
        } else if self
            .frontend_metadata_detail
            .as_deref()
            .is_some_and(|detail| detail.starts_with("Updated with issues"))
        {
            FrontendMetadataSummary::UpdatedWithIssues
        } else if self.frontend_metadata_error.is_some() {
            FrontendMetadataSummary::Failed
        } else if self.frontend_metadata_updated_at.is_some() {
            FrontendMetadataSummary::UpToDate
        } else {
            FrontendMetadataSummary::NotStarted
        }
    }

    /// The save-sync problem worth interrupting the user for. Save sync is
    /// silent until the user enables it: a missing folder or unverified
    /// account is a setup hint then, not something needing attention.
    pub fn save_sync_attention(&self) -> Option<String> {
        self.save_sync_failure
            .clone()
            .or_else(|| {
                self.save_sync_enabled
                    .then(|| self.save_sync_problem.clone())
                    .flatten()
            })
            .or_else(|| {
                self.save_sync_queue
                    .as_ref()
                    .and_then(|queue| queue.failure.clone())
            })
    }

    /// One-line hint shown while save sync is off and cannot be enabled yet.
    pub fn save_sync_off_hint(&self) -> Option<&'static str> {
        (!self.save_sync_enabled && !self.save_sync_available && self.save_sync_problem.is_some())
            .then_some("Save sync is off. Choose a save folder or RetroBat installation in Settings to use it.")
    }

    /// The one derived save-sync status (UI-PLAN §5 table). Zero eligible
    /// files is not "up to date"; an unlinked-file count does not downgrade
    /// an otherwise healthy linked set.
    pub fn save_sync_summary(&self) -> SaveSyncSummary {
        if self.conn == ConnectionState::SignInRequired || self.save_sync_authentication_required {
            return SaveSyncSummary::AuthRequired;
        }
        if self.conn == ConnectionState::Failed {
            return SaveSyncSummary::Offline;
        }
        if !self.save_sync_enabled {
            return SaveSyncSummary::Off;
        }
        if self.save_sync_checking {
            return if self.save_sync_attention_games > 0 || self.save_sync_attention().is_some() {
                SaveSyncSummary::NeedsAttention
            } else {
                SaveSyncSummary::Checking
            };
        }
        if !self.memory_card_conflicts.is_empty() {
            return SaveSyncSummary::NeedsAttention;
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
        if queue.attention_games > 0 {
            return SaveSyncSummary::NeedsAttention;
        }
        if queue.network_paused {
            return SaveSyncSummary::Paused;
        }
        if queue.mapped_games == 0 {
            return SaveSyncSummary::NoLinkedFiles;
        }
        if queue.pending_outbound > 0 {
            return SaveSyncSummary::Syncing;
        }
        if queue.attention_games > 0 || queue.pending_incoming > 0 {
            return SaveSyncSummary::NeedsAttention;
        }
        if queue.reconciled_games < queue.mapped_games {
            return SaveSyncSummary::Reconciling;
        }
        if queue.games.len() != queue.mapped_games
            || queue
                .games
                .iter()
                .any(|game| game.work != rommfs_core::events::SaveSyncWork::UpToDate)
        {
            return SaveSyncSummary::Reconciling;
        }
        SaveSyncSummary::UpToDate
    }

    /// Review versions belong to the existing save identity, never to a
    /// game title or to a count of queued downloads.
    pub fn save_sync_review_versions(
        &self,
        rom_id: i64,
        file_id: i64,
    ) -> impl Iterator<Item = &rommfs_core::events::SaveSyncIncomingStatus> {
        let game = self
            .save_sync_games
            .iter()
            .find(|game| (game.rom_id, game.file_id) == (rom_id, file_id));
        self.save_sync_queue
            .iter()
            .flat_map(|queue| queue.incoming.iter())
            .filter(move |incoming| {
                incoming.rom_id == rom_id
                    && game.is_some_and(|game| game.incoming_ids.contains(&incoming.incoming_id))
            })
    }

    /// Keep a deliberate consent change visible until a newer worker session
    /// acknowledges it. Old discovery results cannot undo the user's choice.
    pub fn request_save_sync_enabled(&mut self, enabled: bool) {
        self.save_sync_enabled = enabled;
        self.requested_save_sync_consent = Some((self.save_sync_session_id, enabled));
    }

    pub fn save_sync_review_count(&self) -> usize {
        self.save_sync_games
            .iter()
            .filter(|game| {
                game.work == rommfs_core::events::SaveSyncWork::NeedsReview
                    || !game.incoming_ids.is_empty()
            })
            .count()
            + self.memory_card_conflicts.len()
    }

    pub fn save_sync_summary_text(&self) -> String {
        use rommfs_core::events::SaveSyncWork;
        let review = self.save_sync_review_count();
        let active = self
            .save_sync_games
            .iter()
            .filter(|game| game.work == SaveSyncWork::Uploading)
            .count();
        let waiting = self
            .save_sync_games
            .iter()
            .filter(|game| game.work == SaveSyncWork::WaitingToUpload)
            .count();
        let mut parts = Vec::new();
        if !self.save_sync_enabled {
            parts.push("Automatic save sync is off".to_owned());
        }
        let summary = self.save_sync_summary();
        if matches!(
            summary,
            SaveSyncSummary::AuthRequired
                | SaveSyncSummary::Offline
                | SaveSyncSummary::NeedsSetup
                | SaveSyncSummary::Paused
        ) || (summary == SaveSyncSummary::NeedsAttention && review == 0)
        {
            parts.push(summary.label().to_owned());
        }
        if review > 0 {
            parts.push(format!(
                "{review} {} review",
                if review == 1 {
                    "save needs"
                } else {
                    "saves need"
                }
            ));
        }
        if self.save_sync_checking && review == 0 {
            parts.push(SaveSyncSummary::Checking.label().to_owned());
        }
        if active > 0 && !self.save_sync_checking {
            parts.push(format!(
                "Uploading {active} {}",
                if active == 1 { "save" } else { "saves" }
            ));
        }
        if waiting > 0 && !self.save_sync_checking {
            parts.push(format!(
                "{waiting} {} waiting to upload",
                if waiting == 1 { "save" } else { "saves" }
            ));
        }
        if parts.is_empty() {
            if self.save_sync_summary() == SaveSyncSummary::UpToDate {
                parts.push(format!(
                    "{} tracked {} up to date",
                    self.save_sync_games.len(),
                    if self.save_sync_games.len() == 1 {
                        "save"
                    } else {
                        "saves"
                    }
                ));
            } else {
                parts.push(self.save_sync_summary().label().to_owned());
            }
        }
        parts.join(" · ")
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
