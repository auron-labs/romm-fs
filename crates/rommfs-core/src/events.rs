//! Worker -> UI event channel and the bounded diagnostic log.
//! All events are real worker facts; nothing is fabricated by timers.

use std::collections::VecDeque;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

/// One diagnostic log line: timestamped, severity, operation, context.
#[derive(Clone, Debug)]
pub struct LogLine {
    pub unix_secs: u64,
    pub level: Level,
    /// Short operation tag, e.g. "catalogue", "download", "mount", "evict".
    pub op: &'static str,
    /// Human-readable detail incl. path/rom id/status code/cause. Never
    /// contains passwords, tokens, cookies, or auth headers.
    pub message: String,
}

/// Current work for one save identity, independent of retained history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveSyncWork {
    NotChecked,
    Checking,
    WaitingToUpload,
    Uploading,
    WaitingForRomm,
    NeedsReview,
    UpToDate,
}

/// A durable per-game reconciliation fact for the opt-in save handoff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveSyncGameStatus {
    pub work: SaveSyncWork,
    pub local_path: String,
    pub rom_id: i64,
    pub file_id: i64,
    pub rom_name: String,
    pub local_hash: Option<String>,
    pub remote_id: Option<String>,
    pub remote_hash: Option<String>,
    pub incoming_ids: Vec<String>,
    pub issue: Option<String>,
    pub installed_incoming: bool,
}

/// A verified incoming save kept in the scoped journal for explicit review.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveSyncIncomingStatus {
    pub incoming_id: String,
    pub rom_id: i64,
    pub rom_name: String,
    pub remote_id: String,
    pub content_hash: String,
    pub reason: String,
    pub state: String,
}

/// One discovered emulator memory-card folder with its per-folder consent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryCardFolderStatus {
    pub emulator: String,
    pub label: String,
    pub relative_dir: String,
    pub cards: usize,
    pub enabled: bool,
}

/// A card whose local and remote copies both changed: the local card is kept
/// and the remote copy is written beside it as a `.rommfs-conflict` sibling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryCardConflictStatus {
    pub emulator: String,
    pub card_name: String,
    pub relative_path: String,
    pub remote_version_id: i64,
}

/// An event-backed snapshot of the current save handoff queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveSyncQueueStatus {
    pub session_id: u64,
    pub mapped_games: usize,
    pub reconciled_games: usize,
    pub pending_outbound: usize,
    pub pending_incoming: usize,
    pub attention_games: usize,
    pub network_paused: bool,
    pub authentication_required: bool,
    pub actor_failed: bool,
    pub failure: Option<String>,
    pub games: Vec<SaveSyncGameStatus>,
    pub incoming: Vec<SaveSyncIncomingStatus>,
}

/// A classified on-disk entry inside a target directory that the app does not
/// own, surfaced for remediation UI. `Link` means a symlink/junction whose
/// removal leaves the target intact, not a data-bearing cloud reparse point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForeignKind {
    File,
    Directory,
    Link,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForeignEntry {
    /// Root-relative, '/'-separated path under the affected directory.
    pub path: String,
    pub kind: ForeignKind,
}

/// Why a support file was not installed, for plain-language reporting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuxiliaryWarningKind {
    /// The server lists the file but it is missing on its disk; needs a rescan.
    MissingOnServer,
    /// Switch firmware uploaded as an archive instead of unpacked files.
    SwitchArchive,
    /// Same content published once from several sources. Informational.
    Duplicate,
    Other,
}

/// One structured note from a support-file install.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuxiliaryWarning {
    pub kind: AuxiliaryWarningKind,
    /// File name (or path) the note is about.
    pub subject: String,
    /// Full diagnostic text, as logged.
    pub message: String,
}

impl AuxiliaryWarning {
    /// Informational notes are not problems and are not counted as issues.
    pub fn is_issue(&self) -> bool {
        self.kind != AuxiliaryWarningKind::Duplicate
    }
}

impl std::fmt::Display for AuxiliaryWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Outcome of a finished support-file install.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuxiliarySummary {
    pub published: usize,
    pub unchanged: usize,
    pub warnings: Vec<AuxiliaryWarning>,
}

/// Events emitted by workers. UI renders these; tests assert on them.
#[derive(Clone, Debug)]
pub enum AppEvent {
    ShutdownStarting,
    SaveSyncCheckStarted {
        session_id: u64,
    },
    SaveSyncScanFinished {
        session_id: u64,
        completed_at: u64,
    },
    /// The foreign entries currently blocking a mount/setup, plus which of
    /// them are links the user can safely remove. Emitted alongside a failure
    /// event and cleared by the next attempt.
    ForeignRootEntries {
        /// Absolute path of the directory that was scanned.
        root: String,
        entries: Vec<ForeignEntry>,
        /// Set after a cleanup pass: how many links were just removed, or the
        /// cleanup error.
        removal: Option<Result<usize, String>>,
    },
    AuxiliaryStatus {
        running: bool,
        detail: String,
        error: Option<String>,
        /// Present only on the final event of a completed install.
        summary: Option<AuxiliarySummary>,
    },
    StartupPreference {
        enabled: bool,
        error: Option<String>,
    },
    SaveFileMappings {
        session_id: u64,
        games: Vec<(i64, String)>,
        unmatched: Vec<crate::save_sync::UnmatchedFile>,
    },
    RetroBatInstaller {
        running: bool,
        ready: bool,
        received: u64,
        total: Option<u64>,
        detail: String,
        error: Option<String>,
    },
    FrontendMetadata {
        /// The metadata pass this status belongs to.
        job_id: u64,
        pending: bool,
        detail: String,
        error: Option<String>,
    },
    /// Live progress of the frontend metadata pass. `job_id` identifies the
    /// pass; progress from an earlier, cancelled pass is dropped by the UI.
    FrontendMetadataProgress {
        job_id: u64,
        completed: usize,
        total: usize,
    },
    /// The active metadata pass ended without publishing (cancelled, stopped,
    /// signed out). The UI drops the pass and its progress; later events for
    /// `job_id` are ignored.
    FrontendMetadataInvalidated {
        job_id: u64,
    },
    /// Play-data sync (favourites and play history) availability and consent.
    PlayDataUpdated {
        /// The session holds all four play-data scopes.
        available: bool,
        /// Consent is on for this server and account.
        enabled: bool,
        problem: Option<String>,
    },
    /// "Hide RomM games you already have": the switch state and how many
    /// entries RomMFS hid in the last metadata pass.
    HideCopies {
        enabled: bool,
        hidden: usize,
    },
    RetroBatSetup {
        running: bool,
        detail: String,
        /// Systems the last run could not link, each with its reason (WS6.10).
        skipped: Vec<crate::retrobat::SkippedSystem>,
        error: Option<String>,
    },
    PairingReady {
        user_code: String,
        url: String,
    },
    PairingEnded {
        reason: Option<String>,
    },
    ConnectionRestored {
        server: String,
        mount_root: Option<String>,
    },
    /// The games folder configured in Settings was applied to the active
    /// session — every dependent screen uses this path until a live mount
    /// or a later apply says otherwise.
    MountRootConfigured {
        server: String,
        mount_root: String,
    },
    IntegrationUpdated {
        mode: crate::connection_settings::IntegrationMode,
        retrobat_root: Option<String>,
        /// A verified RetroBat setup record exists for this installation and
        /// games folder.
        setup_complete: bool,
        /// An earlier build's setup record is present: setup must run to
        /// migrate its links before the integration can mount.
        setup_migration: bool,
        error: Option<String>,
    },
    SignedOut,
    Connecting,
    Connected,
    ConnectFailed {
        reason: String,
    },
    /// Authentication is required (again). `reason` preserves the real cause —
    /// rejected credentials, an expired/rejected token, or a revoked pairing —
    /// so the UI shows why, never a bare "sign-in required" placeholder.
    SignInRequired {
        reason: Option<String>,
    },

    /// Actual counts and timings for one catalogue operation.
    CatalogueProgress {
        operation_id: u64,
        stage: String,
        completed: u64,
        total: Option<u64>,
        elapsed_ms: u64,
    },
    CatalogueSnapshot {
        refreshed_at: u64,
        cached: bool,
        pending_remount: bool,
    },
    MountProgress {
        stage: String,
        completed: u64,
        total: Option<u64>,
        elapsed_ms: u64,
    },
    CatalogueLoading,
    CatalogueLoaded {
        platforms: usize,
        /// Distinct ROM identities with at least one visible file.
        games: usize,
        /// Visible files (a game with several files counts each one).
        files: usize,
        skipped_unsupported: usize,
    },
    CatalogueFailed {
        reason: String,
    },

    MountStarting {
        path: String,
    },
    MountStarted {
        path: String,
    },
    MountFailed {
        reason: String,
    },
    MountStopping,
    MountStopped,

    SaveSyncUpdated {
        session_id: u64,
        server_id: Option<String>,
        candidates: Vec<crate::save_sync::InstallationCandidate>,
        skipped: usize,
        selected_root: Option<String>,
        documented_saves_root: Option<String>,
        effective_saves_root: Option<String>,
        profile_version: Option<String>,
        account_id: Option<i64>,
        mapped_targets: usize,
        catalogue_unmapped: usize,
        existing_saves: Option<crate::save_sync::ExistingSavePreview>,
        preview_saves: Vec<String>,
        available: bool,
        selected_problem: Option<String>,
        enabled: bool,
        debounce_secs: u32,
    },
    SaveSyncReconciliation {
        session_id: u64,
        game: SaveSyncGameStatus,
        mapped_games: usize,
        reconciled_games: usize,
        pending_incoming: usize,
        attention_games: usize,
        failure: Option<String>,
    },
    SaveSyncTransferProgress {
        session_id: u64,
        rom_id: i64,
        revision: String,
        phase: String,
        detail: Option<String>,
    },
    /// Invalidates transient facts from an earlier account/root/server scope.
    SaveSyncSessionChanged {
        session_id: u64,
    },
    SaveSyncAuthenticationRequired {
        session_id: u64,
    },
    SaveSyncQueueUpdated(SaveSyncQueueStatus),
    SaveSyncExportFinished {
        session_id: u64,
        incoming_id: String,
        destination: Option<String>,
        error: Option<String>,
    },

    /// Memory-card discovery, per-folder consent, and conflict review.
    MemoryCardsUpdated {
        session_id: u64,
        available: bool,
        folders: Vec<MemoryCardFolderStatus>,
        conflicts: Vec<MemoryCardConflictStatus>,
        problem: Option<String>,
    },

    DownloadStarted {
        file_id: u64,
        rom_id: u64,
        file_name: String,
        total: Option<u64>,
    },
    DownloadProgress {
        file_id: u64,
        rom_id: u64,
        received: u64,
        total: Option<u64>,
    },
    DownloadFinished {
        file_id: u64,
        rom_id: u64,
        file_name: String,
    },
    DownloadFailed {
        file_id: u64,
        rom_id: u64,
        file_name: String,
        reason: String,
    },

    Evicted {
        rom_id: u64,
        file_name: String,
    },
    EvictFailed {
        rom_id: u64,
        reason: String,
    },

    /// A diagnostic line for the log view.
    Log(LogLine),
}

impl AppEvent {
    /// Convenience for emitting a `Log` event.
    pub fn log(level: Level, op: &'static str, message: impl Into<String>) -> Self {
        AppEvent::Log(LogLine {
            unix_secs: crate::cache::now_unix_secs(),
            level,
            op,
            message: message.into(),
        })
    }
}

/// Cheap multi-producer handle workers share. Clone freely.
#[derive(Clone)]
pub struct EventSink {
    tx: mpsc::Sender<AppEvent>,
    active: Option<Arc<std::sync::atomic::AtomicBool>>,
    diagnostic_log: Option<Arc<Mutex<crate::diagnostics::DiagnosticLog>>>,
    /// Shared by every clone so progress is throttled per kind, not per worker.
    progress_throttle: Arc<Mutex<crate::diagnostics::ProgressThrottle>>,
    started: std::time::Instant,
}

impl EventSink {
    pub fn scoped(mut self, active: Arc<std::sync::atomic::AtomicBool>) -> Self {
        self.active = Some(active);
        self
    }
    pub fn is_active(&self) -> bool {
        self.active
            .as_ref()
            .is_none_or(|active| active.load(std::sync::atomic::Ordering::Acquire))
    }

    pub fn with_log_directory(mut self, directory: &std::path::Path) -> std::io::Result<Self> {
        self.diagnostic_log = Some(Arc::new(Mutex::new(
            crate::diagnostics::DiagnosticLog::open(directory)?,
        )));
        Ok(self)
    }
    pub fn emit(&self, event: AppEvent) {
        if !self.is_active() {
            return;
        }
        // Do not Debug arbitrary events: browser approval URLs contain pairing
        // codes. Persist only allowlisted facts and explicit credential-free logs.
        let diagnostic_line = if self.throttled_progress(&event) {
            None
        } else {
            self.diagnostic_line(&event)
        };
        if let (Some(log), Some(line)) = (&self.diagnostic_log, &diagnostic_line) {
            if let Ok(mut log) = log.lock() {
                if let Err(error) = log.append(line) {
                    let _ = self.tx.send(AppEvent::log(
                        Level::Error,
                        "diagnostics",
                        format!("Diagnostic log write failed: {error}"),
                    ));
                }
            }
        }
        // Never block or panic a worker on UI backpressure.
        let _ = self.tx.send(event);
    }

    /// True for a progress tick that the durable log should skip. The UI
    /// still receives every event.
    fn throttled_progress(&self, event: &AppEvent) -> bool {
        use crate::diagnostics::ProgressSample;
        let (kind, sample) = match event {
            AppEvent::RetroBatInstaller {
                running,
                ready,
                received,
                total,
                detail,
                error,
            } => (
                "RetroBatInstaller",
                ProgressSample {
                    running: *running,
                    ready: *ready,
                    error: error.as_deref(),
                    detail,
                    fraction: total
                        .filter(|total| *total > 0)
                        .map(|total| *received as f64 / total as f64),
                },
            ),
            AppEvent::AuxiliaryStatus {
                running,
                detail,
                error,
                ..
            } => (
                "AuxiliaryStatus",
                ProgressSample {
                    running: *running,
                    ready: false,
                    error: error.as_deref(),
                    detail,
                    fraction: count_fraction(detail),
                },
            ),
            AppEvent::RetroBatSetup {
                running,
                detail,
                error,
                ..
            } => (
                "RetroBatSetup",
                ProgressSample {
                    running: *running,
                    ready: false,
                    error: error.as_deref(),
                    detail,
                    fraction: count_fraction(detail),
                },
            ),
            _ => return false,
        };
        let now_ms = self.started.elapsed().as_millis() as u64;
        self.progress_throttle
            .lock()
            .is_ok_and(|mut throttle| !throttle.admit(kind, &sample, now_ms))
    }

    fn diagnostic_line(&self, event: &AppEvent) -> Option<LogLine> {
        match event {
            // MountProgress is deliberately not persisted: it fires per small
            // batch and dominated the diagnostic log (2MB+ per session). Mount
            // start/finish/failure timestamps still bound every stage.
            AppEvent::Log(line) => Some(line.clone()),
            AppEvent::DownloadStarted { .. }
            | AppEvent::DownloadFinished { .. }
            | AppEvent::DownloadFailed { .. }
            | AppEvent::ForeignRootEntries { .. }
            | AppEvent::MountStarted { .. }
            | AppEvent::MountFailed { .. }
            | AppEvent::MountStopped
            | AppEvent::RetroBatSetup { .. }
            | AppEvent::AuxiliaryStatus { .. }
            | AppEvent::RetroBatInstaller { .. }
            | AppEvent::StartupPreference { .. }
            | AppEvent::Evicted { .. }
            | AppEvent::EvictFailed { .. } => Some(LogLine {
                unix_secs: crate::cache::now_unix_secs(),
                level: Level::Info,
                op: "event",
                message: format!("{event:?}"),
            }),
            _ => None,
        }
    }

    /// Append a line to the durable diagnostic file only — never the UI's
    /// activity stream. For high-volume per-item detail that must remain
    /// available for support without burying what the user sees.
    pub fn log_detail(&self, level: Level, op: &'static str, message: impl Into<String>) {
        let Some(log) = &self.diagnostic_log else {
            return;
        };
        if let Ok(mut log) = log.lock() {
            let _ = log.append(&LogLine {
                unix_secs: crate::cache::now_unix_secs(),
                level,
                op,
                message: message.into(),
            });
        }
    }
}

/// Create the (sink, receiver) pair. The app consumes `receiver`.
pub fn channel() -> (EventSink, mpsc::Receiver<AppEvent>) {
    let (tx, rx) = mpsc::channel();
    (
        EventSink {
            tx,
            active: None,
            diagnostic_log: None,
            progress_throttle: Default::default(),
            started: std::time::Instant::now(),
        },
        rx,
    )
}

/// Test helper: a sink that also records every event for later assertion.
#[derive(Clone, Default)]
pub struct EventRecorder {
    inner: Arc<Mutex<Vec<AppEvent>>>,
}

impl EventRecorder {
    pub fn events(&self) -> Vec<AppEvent> {
        self.inner.lock().unwrap().clone()
    }
    /// Attach to a channel: forwards each event into the recorder.
    pub fn spawn_tap(&self, rx: mpsc::Receiver<AppEvent>) -> std::thread::JoinHandle<()> {
        let inner = Arc::clone(&self.inner);
        std::thread::spawn(move || {
            while let Ok(ev) = rx.recv() {
                inner.lock().unwrap().push(ev);
            }
        })
    }
    /// All emitted log messages (for the no-credentials-in-logs assertions).
    pub fn log_messages(&self) -> Vec<String> {
        self.inner
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                AppEvent::Log(l) => Some(l.message.clone()),
                _ => None,
            })
            .collect()
    }
}

/// Bounded scrollback for the UI log pane (newest last, cap enforced).
pub struct LogBuffer {
    cap: usize,
    lines: VecDeque<LogLine>,
}

impl LogBuffer {
    pub fn new(cap: usize) -> Self {
        Self {
            cap: cap.max(16),
            lines: VecDeque::new(),
        }
    }
    pub fn push(&mut self, line: LogLine) {
        if self.lines.len() == self.cap {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }
    pub fn lines(&self) -> impl Iterator<Item = &LogLine> {
        self.lines.iter()
    }
}

/// "… 32 / 146" → 32/146 as a fraction, when the detail carries counts.
fn count_fraction(detail: &str) -> Option<f64> {
    let (left, right) = detail.split_once('/')?;
    let done: f64 = left
        .trim_end()
        .rsplit(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()?;
    let total: f64 = right
        .trim_start()
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()?;
    (total > 0.0).then_some(done / total)
}
