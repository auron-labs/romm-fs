//! Behavioral tests for the real UI transition table (`UiState::apply`).
//! These drive the same enum the GPUI window consumes — no GPUI, no network,
//! and no duplicate state machine (PRD §6 "UI behavior" row).

use rommfs_app::controller::{ConnState, FolderSummary, MountState, SaveSyncSummary, UiState};
use rommfs_core::events::{AppEvent, Level, LogLine, SaveSyncQueueStatus};

#[path = "controller/auxiliary_setup.rs"]
mod auxiliary_setup;
#[path = "controller/retrobat_setup.rs"]
mod retrobat_setup;
#[path = "controller/save_sync_and_folders.rs"]
mod save_sync_and_folders;
#[path = "controller/session_and_downloads.rs"]
mod session_and_downloads;

fn log(message: &str) -> AppEvent {
    AppEvent::Log(LogLine {
        unix_secs: 0,
        level: Level::Info,
        op: "test",
        message: message.to_string(),
    })
}

// ---- UI-PLAN task 1: derived status can never imply another state's success ----

fn queue(
    mapped: usize,
    reconciled: usize,
    pending_out: usize,
    pending_in: usize,
    attention: usize,
) -> SaveSyncQueueStatus {
    SaveSyncQueueStatus {
        session_id: 1,
        mapped_games: mapped,
        reconciled_games: reconciled,
        pending_outbound: pending_out,
        pending_incoming: pending_in,
        attention_games: attention,
        network_paused: false,
        authentication_required: false,
        actor_failed: false,
        failure: None,
        games: Vec::new(),
        incoming: Vec::new(),
    }
}

fn enabled_ui() -> UiState {
    let mut ui = UiState::new(64);
    ui.apply(&AppEvent::Connecting);
    ui.apply(&AppEvent::Connected);
    // Queue/session events are gated on the actor's session id.
    ui.apply(&AppEvent::SaveSyncSessionChanged { session_id: 1 });
    ui.save_sync_enabled = true;
    ui.save_sync_available = true;
    ui
}

use rommfs_core::connection_settings::IntegrationMode;

fn linked_ui() -> UiState {
    let mut ui = UiState::new(64);
    ui.apply(&AppEvent::Connected);
    ui
}
