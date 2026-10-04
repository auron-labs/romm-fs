//! Worker -> UI event channel and the bounded diagnostic log.
//! All events are real worker facts; nothing is fabricated by timers.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;

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

/// Events emitted by workers. UI renders these; tests assert on them.
#[derive(Clone, Debug)]
pub enum AppEvent {
    Connecting,
    Connected,
    ConnectFailed { reason: String },
    SignInRequired,

    CatalogueLoading,
    CatalogueLoaded { platforms: usize, roms: usize, skipped_unsupported: usize },
    CatalogueFailed { reason: String },

    MountStarting { path: String },
    MountStarted { path: String },
    MountFailed { reason: String },
    MountStopping,
    MountStopped,

    DownloadStarted { rom_id: u64, file_name: String, total: Option<u64> },
    DownloadProgress { rom_id: u64, received: u64, total: Option<u64> },
    DownloadFinished { rom_id: u64, file_name: String },
    DownloadFailed { rom_id: u64, file_name: String, reason: String },

    Evicted { rom_id: u64, file_name: String },
    EvictFailed { rom_id: u64, reason: String },

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
}

impl EventSink {
    pub fn emit(&self, event: AppEvent) {
        // Never block or panic a worker on UI backpressure.
        let _ = self.tx.send(event);
    }
}

/// Create the (sink, receiver) pair. The app consumes `receiver`.
pub fn channel() -> (EventSink, mpsc::Receiver<AppEvent>) {
    let (tx, rx) = mpsc::channel();
    (EventSink { tx }, rx)
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
        Self { cap: cap.max(16), lines: VecDeque::new() }
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
