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

/// A durable per-game reconciliation fact for the opt-in save handoff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveSyncGameStatus {
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

/// Events emitted by workers. UI renders these; tests assert on them.
#[derive(Clone, Debug)]
pub enum AppEvent {
    AuxiliaryStatus {
        running: bool,
        detail: String,
        error: Option<String>,
    },
    StartupPreference {
        enabled: bool,
        error: Option<String>,
    },
    SaveFileMappings {
        session_id: u64,
        games: Vec<(i64, String)>,
        unmatched: Vec<String>,
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
        pending: bool,
        detail: String,
        error: Option<String>,
    },
    RetroBatSetup {
        running: bool,
        detail: String,
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
    IntegrationUpdated {
        mode: crate::connection_settings::IntegrationMode,
        retrobat_root: Option<String>,
        error: Option<String>,
    },
    SignedOut,
    Connecting,
    Connected,
    ConnectFailed {
        reason: String,
    },
    SignInRequired,

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
        roms: usize,
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
        let diagnostic_line = match &event {
            AppEvent::Log(line) => Some(line.clone()),
            AppEvent::DownloadStarted { .. }
            | AppEvent::DownloadFinished { .. }
            | AppEvent::DownloadFailed { .. }
            | AppEvent::MountStarted { .. }
            | AppEvent::MountFailed { .. }
            | AppEvent::MountStopped
            | AppEvent::MountProgress { .. }
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
}

/// Create the (sink, receiver) pair. The app consumes `receiver`.
pub fn channel() -> (EventSink, mpsc::Receiver<AppEvent>) {
    let (tx, rx) = mpsc::channel();
    (
        EventSink {
            tx,
            active: None,
            diagnostic_log: None,
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
