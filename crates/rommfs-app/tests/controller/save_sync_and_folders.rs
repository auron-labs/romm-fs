// Behavioral tests for public controller state transitions.
use super::*;

/// The old bug: invalid credentials collapsed to a bare "sign-in required"
/// with the real reason discarded. The event now carries it through.
#[test]
fn signin_required_preserves_the_real_reason() {
    let mut ui = UiState::new(64);
    ui.apply_event(&AppEvent::Connecting);
    ui.apply_event(&AppEvent::SignInRequired {
        reason: Some("credentials rejected by server".into()),
    });
    assert_eq!(ui.conn, ConnectionState::SignInRequired);
    assert_eq!(
        ui.conn_error.as_deref(),
        Some("credentials rejected by server"),
        "the user's actual failure reason must reach the UI"
    );
}

/// Zero linked files after a completed check was previously reported as
/// "Up to date". It is its own state now (UI-PLAN §5).
#[test]
fn save_sync_zero_linked_files_is_not_up_to_date() {
    let mut ui = enabled_ui();
    ui.apply_event(&AppEvent::SaveSyncQueueUpdated(queue(0, 0, 0, 0, 0)));
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::NoLinkedFiles);
    assert_ne!(
        ui.save_sync_summary(),
        SaveSyncSummary::UpToDate,
        "nothing linked can never mean everything synced"
    );
}

/// Every status combination actually reachable: off / needs setup / checking /
/// reconciling / syncing / attention / paused / clean.
#[test]
fn save_sync_summary_covers_the_status_table() {
    // Off — enabled=false, no implied protection.
    let mut ui = UiState::new(64);
    ui.apply_event(&AppEvent::Connected);
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::Off);

    // Needs setup — enabled but source config invalid.
    let mut ui = UiState::new(64);
    ui.apply_event(&AppEvent::Connected);
    ui.save_sync_enabled = true;
    ui.save_sync_available = false;
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::NeedsSetup);

    // Checking — enabled, available, no completed check yet.
    let ui = enabled_ui();
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::Checking);

    // Reconciling — links known, check incomplete.
    let mut ui = enabled_ui();
    ui.apply_event(&AppEvent::SaveSyncQueueUpdated(queue(4, 2, 0, 0, 0)));
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::Reconciling);

    // Syncing — outbound transfers pending.
    let mut ui = enabled_ui();
    ui.apply_event(&AppEvent::SaveSyncQueueUpdated(queue(4, 4, 2, 0, 0)));
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::Syncing);

    // Needs attention — a failed operation beats an otherwise-busy queue.
    let mut ui = enabled_ui();
    let mut failed = queue(4, 4, 2, 0, 0);
    failed.failure = Some("upload rejected".into());
    ui.apply_event(&AppEvent::SaveSyncQueueUpdated(failed));
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::NeedsAttention);

    // Needs attention — dead actor.
    let mut ui = enabled_ui();
    let mut dead = queue(4, 4, 0, 0, 0);
    dead.actor_failed = true;
    ui.apply_event(&AppEvent::SaveSyncQueueUpdated(dead));
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::NeedsAttention);

    // Needs attention — review items pending.
    let mut ui = enabled_ui();
    ui.apply_event(&AppEvent::SaveSyncQueueUpdated(queue(4, 4, 0, 1, 0)));
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::NeedsAttention);

    // Paused — worker throttled by network errors.
    let mut ui = enabled_ui();
    let mut paused = queue(4, 4, 0, 0, 0);
    paused.network_paused = true;
    ui.apply_event(&AppEvent::SaveSyncQueueUpdated(paused));
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::Paused);

    // Up to date — linked files checked, no pending work.
    let mut ui = enabled_ui();
    ui.apply_event(&AppEvent::SaveSyncQueueUpdated(queue(4, 4, 0, 0, 0)));
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::UpToDate);
}

/// Connection states gate the summary — save sync can never look healthy
/// when auth or transport failed.
#[test]
fn save_sync_summary_follows_auth_and_offline() {
    let mut ui = enabled_ui();
    ui.apply_event(&AppEvent::SignInRequired { reason: None });
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::AuthRequired);

    let mut ui = enabled_ui();
    ui.apply_event(&AppEvent::SaveSyncAuthenticationRequired { session_id: 1 });
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::AuthRequired);

    let mut ui = enabled_ui();
    ui.apply_event(&AppEvent::ConnectFailed {
        reason: "refused".into(),
    });
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::Offline);
}

/// Folder status is derived only from mount events + persisted config:
/// "setup needed" and "configured but stopped" are different states, and
/// Connected alone never implies Ready.
#[test]
fn folder_summary_never_implied_by_connection() {
    let mut ui = UiState::new(64);
    ui.apply_event(&AppEvent::Connected);
    assert_eq!(ui.folder_summary(), FolderSummary::SetupNeeded);

    // Persisted config for this server makes it "stopped", not "unconfigured".
    ui.apply_event(&AppEvent::ConnectionRestored {
        server: "romm.local".into(),
        mount_root: Some("C:\\RomM".into()),
    });
    assert_eq!(ui.folder_summary(), FolderSummary::Stopped);
    assert_eq!(ui.conn, ConnectionState::Connected);

    ui.apply_event(&AppEvent::MountStarting {
        path: "C:\\RomM".into(),
    });
    assert_eq!(ui.folder_summary(), FolderSummary::Starting);

    ui.apply_event(&AppEvent::MountStarted {
        path: "C:\\RomM".into(),
    });
    assert_eq!(ui.folder_summary(), FolderSummary::Ready);

    ui.apply_event(&AppEvent::MountStopped);
    assert_eq!(ui.folder_summary(), FolderSummary::Stopped);

    let mut ui = UiState::new(64);
    ui.apply_event(&AppEvent::MountStarting {
        path: "C:\\RomM".into(),
    });
    ui.apply_event(&AppEvent::MountFailed {
        reason: "denied".into(),
    });
    assert_eq!(ui.folder_summary(), FolderSummary::Failed);
}

/// A previous session's configured folder cannot leak into a new server's
/// view after reconnect or sign-out.
#[test]
fn configured_folder_does_not_survive_reconnect() {
    let mut ui = UiState::new(64);
    ui.apply_event(&AppEvent::ConnectionRestored {
        server: "romm.local".into(),
        mount_root: Some("C:\\RomM".into()),
    });
    assert_eq!(ui.folder_summary(), FolderSummary::Stopped);

    ui.apply_event(&AppEvent::Connecting);
    assert_eq!(ui.folder_summary(), FolderSummary::SetupNeeded);

    ui.apply_event(&AppEvent::ConnectionRestored {
        server: "other.local".into(),
        mount_root: Some("D:\\RomM".into()),
    });
    assert_eq!(ui.configured_mount_root.as_deref(), Some("D:\\RomM"));

    ui.apply_event(&AppEvent::SignedOut);
    assert_eq!(ui.folder_summary(), FolderSummary::SetupNeeded);
}

#[test]
fn save_sync_is_silent_until_enabled() {
    let mut ui = linked_ui();
    ui.save_sync_problem = Some("Choose a save-data folder.".into());
    assert_eq!(ui.save_sync_attention(), None);
    assert!(ui.save_sync_off_hint().is_some());
    ui.save_sync_enabled = true;
    assert_eq!(
        ui.save_sync_attention().as_deref(),
        Some("Choose a save-data folder.")
    );
    assert_eq!(ui.save_sync_off_hint(), None);
}
