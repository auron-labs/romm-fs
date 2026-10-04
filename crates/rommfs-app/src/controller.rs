//! Headless app controller: the real state machine the GPUI window renders.
//! Drives workers via commands, consumes `AppEvent`s into `UiState`.
//! Tests exercise this path — no duplicate state machine in the UI (PRD §6).

use rommfs_core::events::{AppEvent, EventSink, LogBuffer};
use std::sync::mpsc;

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
    pub finished: Option<Result<(), String>>,
}

/// Everything the window draws — derived ONLY from real worker events.
pub struct UiState {
    pub conn: ConnState,
    pub conn_error: Option<String>,
    pub mount: MountState,
    pub mount_path: Option<String>,
    pub mount_error: Option<String>,
    pub downloads: Vec<DownloadView>,
    pub catalogue: Option<(usize, usize, usize)>, // platforms, roms, skipped
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
            log: LogBuffer::new(log_cap),
        }
    }

    /// Apply one worker event. This is THE transition table the UI uses.
    pub fn apply(&mut self, event: &AppEvent) {
        todo!("every AppEvent variant -> state fields incl. terminal DownloadFailed clears progress; MountFailed cannot show Mounted")
    }
}

/// Commands the window sends to the controller.
#[derive(Debug)]
pub enum Command {
    Connect { url: String, username: String, password: String },
    StartMount { path: String },
    StopMount,
    Shutdown,
}

/// Owns the worker session (client + catalogue + mount + evict timer).
/// Created on Connect; events flow back over the `EventSink` channel.
pub struct Controller {
    sink: EventSink,
    cmd_tx: mpsc::Sender<Command>,
    // worker thread handle held for shutdown joining
}

impl Controller {
    /// Spawn the controller + worker loop. Returns (controller, event rx).
    pub fn spawn() -> (Self, mpsc::Receiver<AppEvent>) {
        todo!("channel(); thread: recv Command -> connect/catalogue/mount/evict-tick; Shutdown -> stop mount, release pending, exit")
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.cmd_tx.send(cmd);
    }

    pub fn sink(&self) -> EventSink {
        self.sink.clone()
    }
}
