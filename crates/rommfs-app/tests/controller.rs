//! Behavioral tests for the real UI transition table (`UiState::apply`).
//! These drive the same enum the GPUI window consumes — no GPUI, no network,
//! and no duplicate state machine (PRD §6 "UI behavior" row).

use rommfs_app::controller::{ConnState, FolderSummary, MountState, SaveSyncSummary, UiState};
use rommfs_core::events::{AppEvent, Level, LogLine, SaveSyncQueueStatus};

fn log(message: &str) -> AppEvent {
    AppEvent::Log(LogLine {
        unix_secs: 0,
        level: Level::Info,
        op: "test",
        message: message.to_string(),
    })
}

/// Connect happy path: Connecting -> Connected -> catalogue counts become
/// visible exactly as the worker reported them.
#[test]
fn connect_success_shows_connected_and_catalogue_counts() {
    let mut ui = UiState::new(64);
    ui.apply(&AppEvent::Connecting);
    assert_eq!(ui.conn, ConnState::Connecting);
    assert!(ui.conn_error.is_none());

    ui.apply(&AppEvent::Connected);
    assert_eq!(ui.conn, ConnState::Connected);

    ui.apply(&AppEvent::CatalogueLoading);
    ui.apply(&AppEvent::CatalogueLoaded {
        platforms: 2,
        roms: 5,
        skipped_unsupported: 1,
    });
    assert_eq!(ui.catalogue, Some((2, 5, 1)));
    assert!(ui.conn_error.is_none());
}

/// Rejected credentials must surface as "sign-in required", never success —
/// a login failure cannot display Connected (PRD §6 UI row).
#[test]
fn auth_failure_shows_signin_required_not_connected() {
    let mut ui = UiState::new(64);
    ui.apply(&AppEvent::Connecting);
    ui.apply(&AppEvent::SignInRequired { reason: None });
    assert_eq!(ui.conn, ConnState::SignInRequired);
    assert_ne!(ui.conn, ConnState::Connected);

    let err = ui.conn_error.expect("an error explanation is visible");
    assert!(err.contains("sign-in"), "unexpected error text: {err}");

    // And a non-auth connect failure shows its own reason.
    let mut ui = UiState::new(64);
    ui.apply(&AppEvent::Connecting);
    ui.apply(&AppEvent::ConnectFailed {
        reason: "transport error: refused".into(),
    });
    assert_eq!(ui.conn, ConnState::Failed);
    assert_eq!(ui.conn_error.as_deref(), Some("transport error: refused"));
}

/// A catalogue failure after a good auth must not look like a successful
/// empty library: the error stays visible while Connected remains true.
#[test]
fn catalogue_failure_keeps_error_visible() {
    let mut ui = UiState::new(64);
    ui.apply(&AppEvent::Connecting);
    ui.apply(&AppEvent::Connected);
    ui.apply(&AppEvent::CatalogueLoading);
    ui.apply(&AppEvent::CatalogueFailed {
        reason: "server error 500".into(),
    });
    assert_eq!(ui.conn, ConnState::Connected);
    assert_eq!(ui.conn_error.as_deref(), Some("server error 500"));
    assert!(ui.catalogue.is_none(), "failed catalogue shows no counts");
}

/// Starting over clears stale state: a new Connecting resets the old
/// catalogue and download rows so a different server can't inherit them.
#[test]
fn reconnect_clears_previous_session_state() {
    let mut ui = UiState::new(64);
    for ev in [
        AppEvent::Connected,
        AppEvent::CatalogueLoaded {
            platforms: 1,
            roms: 1,
            skipped_unsupported: 0,
        },
        AppEvent::DownloadStarted {
            file_id: 1,
            rom_id: 7,
            file_name: "a.nes".into(),
            total: Some(10),
        },
    ] {
        ui.apply(&ev);
    }
    ui.apply(&AppEvent::Connecting);
    assert!(ui.catalogue.is_none());
    assert!(ui.downloads.is_empty());
    assert!(ui.conn_error.is_none());
    assert_eq!(ui.conn, ConnState::Connecting);
}

/// Mount start/stop transitions land on the states the window renders.
#[test]
fn mount_start_stop_transitions() {
    let mut ui = UiState::new(64);
    let path = "C:\\RomM".to_string();

    ui.apply(&AppEvent::MountStarting { path: path.clone() });
    assert_eq!(ui.mount, MountState::Mounting);
    assert_eq!(ui.mount_path.as_deref(), Some(path.as_str()));

    ui.apply(&AppEvent::MountStarted { path: path.clone() });
    assert_eq!(ui.mount, MountState::Mounted);
    assert_eq!(ui.mount_path.as_deref(), Some(path.as_str()));
    assert!(ui.mount_error.is_none());

    ui.apply(&AppEvent::MountStopping);
    assert_eq!(
        ui.mount,
        MountState::Mounted,
        "still mounted while stopping"
    );

    ui.apply(&AppEvent::MountStopped);
    assert_eq!(ui.mount, MountState::NotMounted);
}

/// A start click disables the button immediately, before the worker can send
/// MountStarting back over the polled event channel.
#[test]
fn mount_request_is_pending_immediately() {
    let mut ui = UiState::new(64);
    ui.request_mount("C:\\RomM".into());

    assert_eq!(ui.mount, MountState::Mounting);
    assert_eq!(ui.mount_path.as_deref(), Some("C:\\RomM"));
    assert!(ui.mount_error.is_none());
}

/// A mount failure must never display Mounted (PRD §6 UI row) and must
/// surface the reason as visible text.
#[test]
fn mount_failure_never_shows_mounted_and_recovers() {
    let mut ui = UiState::new(64);
    ui.apply(&AppEvent::MountStarting {
        path: "C:\\RomM".into(),
    });
    ui.apply(&AppEvent::MountFailed {
        reason: "root not empty".into(),
    });
    assert_eq!(ui.mount, MountState::Failed);
    assert_ne!(ui.mount, MountState::Mounted);
    assert_eq!(ui.mount_error.as_deref(), Some("root not empty"));

    // A later successful attempt recovers cleanly.
    ui.apply(&AppEvent::MountStarting {
        path: "C:\\RomM".into(),
    });
    assert_eq!(ui.mount, MountState::Mounting);
    assert!(ui.mount_error.is_none(), "retry clears the old error");
    ui.apply(&AppEvent::MountStarted {
        path: "C:\\RomM".into(),
    });
    assert_eq!(ui.mount, MountState::Mounted);
}

/// Stop also ends any in-flight progress rows — nothing may look like it is
/// still downloading once the mount is down.
#[test]
fn stop_mount_marks_inflight_downloads_failed() {
    let mut ui = UiState::new(64);
    ui.apply(&AppEvent::DownloadStarted {
        file_id: 1,
        rom_id: 9,
        file_name: "g.gb".into(),
        total: Some(64),
    });
    ui.apply(&AppEvent::DownloadStarted {
        file_id: 1,
        rom_id: 10,
        file_name: "h.gb".into(),
        total: Some(64),
    });
    ui.apply(&AppEvent::DownloadFinished {
        file_id: 1,
        rom_id: 10,
        file_name: "h.gb".into(),
    });
    ui.apply(&AppEvent::MountStopping);
    ui.apply(&AppEvent::MountStopped);

    let inflight = ui.downloads.iter().find(|d| d.rom_id == 9).unwrap();
    assert_eq!(
        inflight.finished,
        Some(Err("mount stopped".into())),
        "in-flight row must show a terminal state, not fake progress"
    );
    let done = ui.downloads.iter().find(|d| d.rom_id == 10).unwrap();
    assert_eq!(done.finished, Some(Ok(())));
}

/// Download events update the view: progress accumulates, totals appear
/// when known, completion is terminal, and a retry after failure is a fresh
/// in-flight row.
#[test]
fn download_events_drive_progress_and_terminal_state() {
    let mut ui = UiState::new(64);
    ui.apply(&AppEvent::DownloadStarted {
        file_id: 1,
        rom_id: 7,
        file_name: "a.nes".into(),
        total: Some(100),
    });
    {
        let d = &ui.downloads[0];
        assert_eq!(d.rom_id, 7);
        assert_eq!(d.file_name, "a.nes");
        assert_eq!(d.received, 0);
        assert_eq!(d.total, Some(100));
        assert!(d.finished.is_none());
    }

    ui.apply(&AppEvent::DownloadProgress {
        file_id: 1,
        rom_id: 7,
        received: 40,
        total: Some(100),
    });
    assert_eq!(ui.downloads[0].received, 40);

    ui.apply(&AppEvent::DownloadFinished {
        file_id: 1,
        rom_id: 7,
        file_name: "a.nes".into(),
    });
    assert_eq!(ui.downloads[0].finished, Some(Ok(())));
}

/// Terminal failure clears the downloading state AND keeps the cause visible.
#[test]
fn download_failure_is_terminal_and_retryable() {
    let mut ui = UiState::new(64);
    ui.apply(&AppEvent::DownloadStarted {
        file_id: 1,
        rom_id: 8,
        file_name: "b.sfc".into(),
        total: None,
    });
    ui.apply(&AppEvent::DownloadProgress {
        file_id: 1,
        rom_id: 8,
        received: 12,
        total: None,
    });
    ui.apply(&AppEvent::DownloadFailed {
        file_id: 1,
        rom_id: 8,
        file_name: "b.sfc".into(),
        reason: "truncated body".into(),
    });
    let d = &ui.downloads[0];
    assert_eq!(d.finished, Some(Err("truncated body".into())));
    assert_eq!(d.received, 12, "progress at failure stays visible");

    // Retry replaces the terminal failure with a fresh in-flight row.
    ui.apply(&AppEvent::DownloadStarted {
        file_id: 1,
        rom_id: 8,
        file_name: "b.sfc".into(),
        total: None,
    });
    let d = &ui.downloads[0];
    assert!(d.finished.is_none());
    assert_eq!(d.received, 0);
}

/// Unknown totals render as indeterminate — total stays None through
/// progress until the transfer ends (no fabricated totals).
#[test]
fn download_without_total_stays_indeterminate() {
    let mut ui = UiState::new(64);
    ui.apply(&AppEvent::DownloadStarted {
        file_id: 1,
        rom_id: 5,
        file_name: "m.bin".into(),
        total: None,
    });
    ui.apply(&AppEvent::DownloadProgress {
        file_id: 1,
        rom_id: 5,
        received: 1024,
        total: None,
    });
    let d = &ui.downloads[0];
    assert_eq!(d.received, 1024);
    assert_eq!(d.total, None);
}

/// The log view is bounded: pushing more than the cap keeps only the newest
/// lines, oldest evicted first (the cap the window relies on).
#[test]
fn log_buffer_caps_at_limit_newest_last() {
    let mut ui = UiState::new(20);
    for i in 0..30 {
        ui.apply(&log(&format!("line {i}")));
    }
    let messages: Vec<&str> = ui.log.lines().map(|l| l.message.as_str()).collect();
    assert_eq!(messages.len(), 20);
    assert_eq!(messages.last(), Some(&"line 29"));
    assert_eq!(messages.first(), Some(&"line 10"));
}

/// Eviction outcomes are surfaced through the bounded log so a user can see
/// data being reclaimed.
#[test]
fn evict_events_land_in_log() {
    let mut ui = UiState::new(64);
    ui.apply(&AppEvent::Evicted {
        rom_id: 3,
        file_name: "old.nes".into(),
    });
    ui.apply(&AppEvent::EvictFailed {
        rom_id: 4,
        reason: "deferred".into(),
    });
    let messages: Vec<String> = ui.log.lines().map(|l| l.message.clone()).collect();
    assert!(messages.iter().any(|m| m.contains("old.nes")));
    assert!(messages.iter().any(|m| m.contains("rom 4")));
}

#[test]
fn file_id_separates_transfers_and_history_never_prunes_active_or_failed_rows() {
    let mut ui = UiState::new(32);
    for file_id in 1..=150 {
        ui.apply(&AppEvent::DownloadStarted {
            rom_id: 7,
            file_id,
            file_name: format!("file{file_id}.nsp"),
            total: Some(10),
        });
    }
    ui.apply(&AppEvent::DownloadProgress {
        rom_id: 7,
        file_id: 2,
        received: 5,
        total: Some(10),
    });
    assert_eq!(ui.downloads.len(), 150);
    assert_eq!(
        ui.downloads
            .iter()
            .find(|d| d.file_id == 1)
            .unwrap()
            .received,
        0
    );
    assert_eq!(
        ui.downloads
            .iter()
            .find(|d| d.file_id == 2)
            .unwrap()
            .received,
        5
    );
    for file_id in 151..=300 {
        ui.apply(&AppEvent::DownloadFinished {
            rom_id: 7,
            file_id,
            file_name: format!("file{file_id}.nsp"),
        });
    }
    ui.apply(&AppEvent::DownloadFailed {
        rom_id: 7,
        file_id: 301,
        file_name: "failed.nsp".into(),
        reason: "HTTP 500".into(),
    });
    assert_eq!(
        ui.downloads.iter().filter(|d| d.finished.is_none()).count(),
        150
    );
    assert_eq!(
        ui.downloads
            .iter()
            .filter(|d| matches!(d.finished, Some(Ok(()))))
            .count(),
        128
    );
    ui.clear_completed_downloads();
    assert_eq!(ui.downloads.len(), 151);
    assert!(matches!(
        ui.downloads.last().unwrap().finished,
        Some(Err(_))
    ));
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

/// The old bug: invalid credentials collapsed to a bare "sign-in required"
/// with the real reason discarded. The event now carries it through.
#[test]
fn signin_required_preserves_the_real_reason() {
    let mut ui = UiState::new(64);
    ui.apply(&AppEvent::Connecting);
    ui.apply(&AppEvent::SignInRequired {
        reason: Some("credentials rejected by server".into()),
    });
    assert_eq!(ui.conn, ConnState::SignInRequired);
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
    ui.apply(&AppEvent::SaveSyncQueueUpdated(queue(0, 0, 0, 0, 0)));
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
    ui.apply(&AppEvent::Connected);
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::Off);

    // Needs setup — enabled but source config invalid.
    let mut ui = UiState::new(64);
    ui.apply(&AppEvent::Connected);
    ui.save_sync_enabled = true;
    ui.save_sync_available = false;
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::NeedsSetup);

    // Checking — enabled, available, no completed check yet.
    let ui = enabled_ui();
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::Checking);

    // Reconciling — links known, check incomplete.
    let mut ui = enabled_ui();
    ui.apply(&AppEvent::SaveSyncQueueUpdated(queue(4, 2, 0, 0, 0)));
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::Reconciling);

    // Syncing — outbound transfers pending.
    let mut ui = enabled_ui();
    ui.apply(&AppEvent::SaveSyncQueueUpdated(queue(4, 4, 2, 0, 0)));
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::Syncing);

    // Needs attention — a failed operation beats an otherwise-busy queue.
    let mut ui = enabled_ui();
    let mut failed = queue(4, 4, 2, 0, 0);
    failed.failure = Some("upload rejected".into());
    ui.apply(&AppEvent::SaveSyncQueueUpdated(failed));
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::NeedsAttention);

    // Needs attention — dead actor.
    let mut ui = enabled_ui();
    let mut dead = queue(4, 4, 0, 0, 0);
    dead.actor_failed = true;
    ui.apply(&AppEvent::SaveSyncQueueUpdated(dead));
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::NeedsAttention);

    // Needs attention — review items pending.
    let mut ui = enabled_ui();
    ui.apply(&AppEvent::SaveSyncQueueUpdated(queue(4, 4, 0, 1, 0)));
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::NeedsAttention);

    // Paused — worker throttled by network errors.
    let mut ui = enabled_ui();
    let mut paused = queue(4, 4, 0, 0, 0);
    paused.network_paused = true;
    ui.apply(&AppEvent::SaveSyncQueueUpdated(paused));
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::Paused);

    // Up to date — linked files checked, no pending work.
    let mut ui = enabled_ui();
    ui.apply(&AppEvent::SaveSyncQueueUpdated(queue(4, 4, 0, 0, 0)));
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::UpToDate);
}

/// Connection states gate the summary — save sync can never look healthy
/// when auth or transport failed.
#[test]
fn save_sync_summary_follows_auth_and_offline() {
    let mut ui = enabled_ui();
    ui.apply(&AppEvent::SignInRequired { reason: None });
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::AuthRequired);

    let mut ui = enabled_ui();
    ui.apply(&AppEvent::SaveSyncAuthenticationRequired { session_id: 1 });
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::AuthRequired);

    let mut ui = enabled_ui();
    ui.apply(&AppEvent::ConnectFailed {
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
    ui.apply(&AppEvent::Connected);
    assert_eq!(ui.folder_summary(), FolderSummary::SetupNeeded);

    // Persisted config for this server makes it "stopped", not "unconfigured".
    ui.apply(&AppEvent::ConnectionRestored {
        server: "romm.local".into(),
        mount_root: Some("C:\\RomM".into()),
    });
    assert_eq!(ui.folder_summary(), FolderSummary::Stopped);
    assert_eq!(ui.conn, ConnState::Connected);

    ui.apply(&AppEvent::MountStarting {
        path: "C:\\RomM".into(),
    });
    assert_eq!(ui.folder_summary(), FolderSummary::Starting);

    ui.apply(&AppEvent::MountStarted {
        path: "C:\\RomM".into(),
    });
    assert_eq!(ui.folder_summary(), FolderSummary::Ready);

    ui.apply(&AppEvent::MountStopped);
    assert_eq!(ui.folder_summary(), FolderSummary::Stopped);

    let mut ui = UiState::new(64);
    ui.apply(&AppEvent::MountStarting {
        path: "C:\\RomM".into(),
    });
    ui.apply(&AppEvent::MountFailed {
        reason: "denied".into(),
    });
    assert_eq!(ui.folder_summary(), FolderSummary::Failed);
}

/// A previous session's configured folder cannot leak into a new server's
/// view after reconnect or sign-out.
#[test]
fn configured_folder_does_not_survive_reconnect() {
    let mut ui = UiState::new(64);
    ui.apply(&AppEvent::ConnectionRestored {
        server: "romm.local".into(),
        mount_root: Some("C:\\RomM".into()),
    });
    assert_eq!(ui.folder_summary(), FolderSummary::Stopped);

    ui.apply(&AppEvent::Connecting);
    assert_eq!(ui.folder_summary(), FolderSummary::SetupNeeded);

    ui.apply(&AppEvent::ConnectionRestored {
        server: "other.local".into(),
        mount_root: Some("D:\\RomM".into()),
    });
    assert_eq!(ui.configured_mount_root.as_deref(), Some("D:\\RomM"));

    ui.apply(&AppEvent::SignedOut);
    assert_eq!(ui.folder_summary(), FolderSummary::SetupNeeded);
}

/// Auxiliary support-file downloads have their own status fields — they must
/// never masquerade as RetroBat setup (and vice versa).
#[test]
fn auxiliary_status_stays_separate_from_retrobat_setup() {
    let mut ui = UiState::new(64);
    ui.retrobat_setup_running = true;
    ui.apply(&AppEvent::AuxiliaryStatus {
        running: true,
        detail: "downloading keys".into(),
        error: None,
        summary: None,
    });
    assert!(ui.auxiliary_running);
    assert_eq!(ui.auxiliary_detail.as_deref(), Some("downloading keys"));
    assert!(ui.auxiliary_error.is_none());
    // The RetroBat flag the caller left set is untouched either way.
    assert!(ui.retrobat_setup_running);

    ui.apply(&AppEvent::AuxiliaryStatus {
        running: false,
        detail: "done".into(),
        error: Some("network".into()),
        summary: None,
    });
    assert!(!ui.auxiliary_running);
    assert_eq!(ui.auxiliary_error.as_deref(), Some("network"));
    assert!(ui.retrobat_setup_running);
}

#[test]
fn selecting_an_installation_clears_stale_installer_and_setup_state() {
    use rommfs_core::connection_settings::IntegrationMode;
    let mut ui = UiState::new(64);
    ui.apply(&AppEvent::RetroBatInstaller {
        running: false,
        ready: true,
        received: 0,
        total: None,
        detail: "ready".into(),
        error: Some("Windows needs permission (os error 740)".into()),
    });
    ui.apply(&AppEvent::RetroBatSetup {
        running: false,
        detail: "failed".into(),
        error: Some("link failed".into()),
    });
    ui.apply(&AppEvent::AuxiliaryStatus {
        running: false,
        detail: "failed".into(),
        error: Some("nope".into()),
        summary: None,
    });
    // Selecting an installation drops the installer error and, because the
    // installation changed, the results of the previous one.
    ui.apply(&AppEvent::IntegrationUpdated {
        mode: IntegrationMode::RetroBat,
        retrobat_root: Some("C:\\RetroBat".into()),
        setup_complete: false,
        error: None,
    });
    assert!(ui.installer_error.is_none());
    assert!(ui.retrobat_setup_error.is_none() && ui.retrobat_setup_detail.is_none());
    assert!(ui.auxiliary_error.is_none() && ui.auxiliary_detail.is_none());

    // A failure for this installation survives re-applying the same selection.
    ui.apply(&AppEvent::RetroBatSetup {
        running: false,
        detail: "failed".into(),
        error: Some("link failed".into()),
    });
    ui.apply(&AppEvent::IntegrationUpdated {
        mode: IntegrationMode::RetroBat,
        retrobat_root: Some("C:\\RetroBat".into()),
        setup_complete: false,
        error: None,
    });
    assert!(ui.retrobat_setup_error.is_some());

    // Restarting setup clears any installer error and the previous setup error.
    ui.installer_error = Some("again".into());
    ui.apply(&AppEvent::RetroBatSetup {
        running: true,
        detail: "working".into(),
        error: None,
    });
    assert!(ui.installer_error.is_none() && ui.retrobat_setup_error.is_none());
}

#[test]
fn auxiliary_summary_carries_structured_notes_and_restart_clears_it() {
    use rommfs_core::events::{AuxiliarySummary, AuxiliaryWarning, AuxiliaryWarningKind};
    let mut ui = UiState::new(64);
    ui.apply(&AppEvent::AuxiliaryStatus {
        running: false,
        detail: "done".into(),
        error: None,
        summary: Some(AuxiliarySummary {
            published: 146,
            unchanged: 0,
            warnings: vec![AuxiliaryWarning {
                kind: AuxiliaryWarningKind::MissingOnServer,
                subject: "a.bin".into(),
                message: "raw".into(),
            }],
        }),
    });
    let summary = ui.auxiliary_summary.as_ref().unwrap();
    assert_eq!(
        summary.warnings[0].kind,
        AuxiliaryWarningKind::MissingOnServer
    );
    assert!(ui.auxiliary_error.is_none());
    ui.apply(&AppEvent::AuxiliaryStatus {
        running: true,
        detail: "again".into(),
        error: None,
        summary: None,
    });
    assert!(ui.auxiliary_summary.is_none());
}

use rommfs_core::connection_settings::IntegrationMode;

fn linked_ui() -> UiState {
    let mut ui = UiState::new(64);
    ui.apply(&AppEvent::Connected);
    ui
}

#[test]
fn retrobat_steps_unlock_in_order_with_one_primary() {
    use rommfs_app::controller::{RetroBatStep, StepStatus};
    let mut ui = linked_ui();
    let steps = ui.retrobat_steps();
    assert_eq!(steps.install, StepStatus::Ready);
    assert!(matches!(steps.link, StepStatus::Blocked(_)));
    assert!(matches!(steps.support, StepStatus::Blocked(_)));
    assert_eq!(steps.primary(), Some(RetroBatStep::Install));

    ui.apply(&AppEvent::IntegrationUpdated {
        mode: IntegrationMode::RetroBat,
        retrobat_root: Some("C:\\RetroBat".into()),
        setup_complete: false,
        error: None,
    });
    let steps = ui.retrobat_steps();
    assert_eq!(steps.install, StepStatus::Done);
    assert_eq!(steps.link, StepStatus::Ready);
    assert_eq!(steps.primary(), Some(RetroBatStep::Link));
    // Selecting an installation is not the same as the layout being set up.
    assert!(!ui.retrobat_setup_ok);

    ui.apply(&AppEvent::RetroBatSetup {
        running: true,
        detail: "working".into(),
        error: None,
    });
    assert_eq!(ui.retrobat_steps().link, StepStatus::Running);
    assert_eq!(ui.retrobat_steps().primary(), None);

    ui.apply(&AppEvent::RetroBatSetup {
        running: false,
        detail: "linked".into(),
        error: None,
    });
    let steps = ui.retrobat_steps();
    assert_eq!(steps.link, StepStatus::Done);
    // Support files also need the library to be loaded.
    assert!(matches!(steps.support, StepStatus::Blocked(_)));
    ui.catalogue = Some((3, 10, 0));
    let steps = ui.retrobat_steps();
    assert_eq!(steps.support, StepStatus::Ready);
    assert_eq!(steps.primary(), Some(RetroBatStep::Support));
    assert!(ui.integration_notice.is_none());

    // Re-applying the same selection keeps the result; a new root drops it.
    ui.apply(&AppEvent::IntegrationUpdated {
        mode: IntegrationMode::RetroBat,
        retrobat_root: Some("C:\\RetroBat".into()),
        setup_complete: false,
        error: None,
    });
    assert_eq!(ui.retrobat_steps().link, StepStatus::Done);
    ui.apply(&AppEvent::IntegrationUpdated {
        mode: IntegrationMode::RetroBat,
        retrobat_root: Some("D:\\RetroBat".into()),
        setup_complete: false,
        error: None,
    });
    assert_eq!(ui.retrobat_steps().link, StepStatus::Ready);

    // A verified record on disk counts after a restart.
    ui.apply(&AppEvent::IntegrationUpdated {
        mode: IntegrationMode::RetroBat,
        retrobat_root: Some("E:\\RetroBat".into()),
        setup_complete: true,
        error: None,
    });
    assert_eq!(ui.retrobat_steps().link, StepStatus::Done);

    // A running games folder blocks (re)setup.
    ui.apply(&AppEvent::MountStarted {
        path: "C:\\RomM".into(),
    });
    assert!(matches!(
        ui.retrobat_steps().support,
        StepStatus::Blocked(_)
    ));
}

#[test]
fn support_files_do_not_wait_for_the_games_folder_link() {
    use rommfs_app::controller::{RetroBatStep, StepStatus};
    let mut ui = linked_ui();
    ui.catalogue = Some((3, 10, 0));
    ui.apply(&AppEvent::IntegrationUpdated {
        mode: IntegrationMode::RetroBat,
        retrobat_root: Some("C:\\RetroBat".into()),
        setup_complete: false,
        error: None,
    });
    ui.apply(&AppEvent::RetroBatSetup {
        running: false,
        detail: "failed".into(),
        error: Some("denied".into()),
    });
    let steps = ui.retrobat_steps();
    assert_eq!(steps.link, StepStatus::Failed);
    assert_eq!(steps.support, StepStatus::Ready);
    // The failed step stays the one primary action.
    assert_eq!(steps.primary(), Some(RetroBatStep::Link));
}

#[test]
fn retrobat_setup_failure_is_a_retry_not_done() {
    use rommfs_app::controller::StepStatus;
    let mut ui = linked_ui();
    ui.apply(&AppEvent::IntegrationUpdated {
        mode: IntegrationMode::RetroBat,
        retrobat_root: Some("C:\\RetroBat".into()),
        setup_complete: false,
        error: None,
    });
    ui.apply(&AppEvent::RetroBatSetup {
        running: false,
        detail: "failed".into(),
        error: Some("denied".into()),
    });
    assert_eq!(ui.retrobat_steps().link, StepStatus::Failed);
}

#[test]
fn loaded_catalogue_means_the_folder_is_not_unconfigured() {
    let mut ui = UiState::new(64);
    ui.catalogue = Some((3, 10, 0));
    assert_eq!(ui.folder_summary(), FolderSummary::Stopped);
}

#[test]
fn progress_counts_are_parsed_from_status_lines() {
    use rommfs_app::controller::parse_progress_counts;
    assert_eq!(
        parse_progress_counts("Auxiliary files: 32 / 146"),
        Some((32, 146))
    );
    assert_eq!(parse_progress_counts("Starting"), None);
    assert_eq!(parse_progress_counts("0 / 0"), None);
}
