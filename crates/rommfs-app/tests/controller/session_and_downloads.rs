// Behavioral tests for public controller state transitions.
use super::*;

/// Connect happy path: Connecting -> Connected -> catalogue counts become
/// visible exactly as the worker reported them.
#[test]
fn connect_success_shows_connected_and_catalogue_counts() {
    let mut ui = UiState::new(64);
    ui.apply_event(&AppEvent::Connecting);
    assert_eq!(ui.conn, ConnectionState::Connecting);
    assert!(ui.conn_error.is_none());

    ui.apply_event(&AppEvent::Connected);
    assert_eq!(ui.conn, ConnectionState::Connected);

    ui.apply_event(&AppEvent::CatalogueLoading);
    ui.apply_event(&AppEvent::CatalogueLoaded {
        platforms: 2,
        games: 4,
        files: 5,
        skipped_unsupported: 1,
    });
    assert_eq!(ui.catalogue, Some((2, 4, 5, 1)));
    assert!(ui.conn_error.is_none());
}

/// Rejected credentials must surface as "sign-in required", never success —
/// a login failure cannot display Connected (PRD §6 UI row).
#[test]
fn auth_failure_shows_signin_required_not_connected() {
    let mut ui = UiState::new(64);
    ui.apply_event(&AppEvent::Connecting);
    ui.apply_event(&AppEvent::SignInRequired { reason: None });
    assert_eq!(ui.conn, ConnectionState::SignInRequired);
    assert_ne!(ui.conn, ConnectionState::Connected);

    let err = ui.conn_error.expect("an error explanation is visible");
    assert!(err.contains("sign-in"), "unexpected error text: {err}");

    // And a non-auth connect failure shows its own reason.
    let mut ui = UiState::new(64);
    ui.apply_event(&AppEvent::Connecting);
    ui.apply_event(&AppEvent::ConnectFailed {
        reason: "transport error: refused".into(),
    });
    assert_eq!(ui.conn, ConnectionState::Failed);
    assert_eq!(ui.conn_error.as_deref(), Some("transport error: refused"));
}

/// A catalogue failure after a good auth must not look like a successful
/// empty library: the error stays visible while Connected remains true.
#[test]
fn catalogue_failure_keeps_error_visible() {
    let mut ui = UiState::new(64);
    ui.apply_event(&AppEvent::Connecting);
    ui.apply_event(&AppEvent::Connected);
    ui.apply_event(&AppEvent::CatalogueLoading);
    ui.apply_event(&AppEvent::CatalogueFailed {
        reason: "server error 500".into(),
    });
    assert_eq!(ui.conn, ConnectionState::Connected);
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
            games: 1,
            files: 1,
            skipped_unsupported: 0,
        },
        AppEvent::DownloadStarted {
            file_id: 1,
            rom_id: 7,
            file_name: "a.nes".into(),
            total: Some(10),
        },
    ] {
        ui.apply_event(&ev);
    }
    ui.apply_event(&AppEvent::Connecting);
    assert!(ui.catalogue.is_none());
    assert!(ui.downloads.is_empty());
    assert!(ui.conn_error.is_none());
    assert_eq!(ui.conn, ConnectionState::Connecting);
}

/// Mount start/stop transitions land on the states the window renders.
#[test]
fn mount_start_stop_transitions() {
    let mut ui = UiState::new(64);
    let path = "C:\\RomM".to_string();

    ui.apply_event(&AppEvent::MountStarting { path: path.clone() });
    assert_eq!(ui.mount, MountState::Mounting);
    assert_eq!(ui.mount_path.as_deref(), Some(path.as_str()));

    ui.apply_event(&AppEvent::MountStarted { path: path.clone() });
    assert_eq!(ui.mount, MountState::Mounted);
    assert_eq!(ui.mount_path.as_deref(), Some(path.as_str()));
    assert!(ui.mount_error.is_none());

    ui.apply_event(&AppEvent::MountStopping);
    assert_eq!(
        ui.mount,
        MountState::Mounted,
        "still mounted while stopping"
    );

    ui.apply_event(&AppEvent::MountStopped);
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
    ui.apply_event(&AppEvent::MountStarting {
        path: "C:\\RomM".into(),
    });
    ui.apply_event(&AppEvent::MountFailed {
        reason: "root not empty".into(),
    });
    assert_eq!(ui.mount, MountState::Failed);
    assert_ne!(ui.mount, MountState::Mounted);
    assert_eq!(ui.mount_error.as_deref(), Some("root not empty"));

    // A later successful attempt recovers cleanly.
    ui.apply_event(&AppEvent::MountStarting {
        path: "C:\\RomM".into(),
    });
    assert_eq!(ui.mount, MountState::Mounting);
    assert!(ui.mount_error.is_none(), "retry clears the old error");
    ui.apply_event(&AppEvent::MountStarted {
        path: "C:\\RomM".into(),
    });
    assert_eq!(ui.mount, MountState::Mounted);
}

/// Stop also ends any in-flight progress rows — nothing may look like it is
/// still downloading once the mount is down.
#[test]
fn stop_mount_marks_inflight_downloads_failed() {
    let mut ui = UiState::new(64);
    ui.apply_event(&AppEvent::DownloadStarted {
        file_id: 1,
        rom_id: 9,
        file_name: "g.gb".into(),
        total: Some(64),
    });
    ui.apply_event(&AppEvent::DownloadStarted {
        file_id: 1,
        rom_id: 10,
        file_name: "h.gb".into(),
        total: Some(64),
    });
    ui.apply_event(&AppEvent::DownloadFinished {
        file_id: 1,
        rom_id: 10,
        file_name: "h.gb".into(),
    });
    ui.apply_event(&AppEvent::MountStopping);
    ui.apply_event(&AppEvent::MountStopped);

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
    ui.apply_event(&AppEvent::DownloadStarted {
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

    ui.apply_event(&AppEvent::DownloadProgress {
        file_id: 1,
        rom_id: 7,
        received: 40,
        total: Some(100),
    });
    assert_eq!(ui.downloads[0].received, 40);

    ui.apply_event(&AppEvent::DownloadFinished {
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
    ui.apply_event(&AppEvent::DownloadStarted {
        file_id: 1,
        rom_id: 8,
        file_name: "b.sfc".into(),
        total: None,
    });
    ui.apply_event(&AppEvent::DownloadProgress {
        file_id: 1,
        rom_id: 8,
        received: 12,
        total: None,
    });
    ui.apply_event(&AppEvent::DownloadFailed {
        file_id: 1,
        rom_id: 8,
        file_name: "b.sfc".into(),
        reason: "truncated body".into(),
    });
    let d = &ui.downloads[0];
    assert_eq!(d.finished, Some(Err("truncated body".into())));
    assert_eq!(d.received, 12, "progress at failure stays visible");

    // Retry replaces the terminal failure with a fresh in-flight row.
    ui.apply_event(&AppEvent::DownloadStarted {
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
    ui.apply_event(&AppEvent::DownloadStarted {
        file_id: 1,
        rom_id: 5,
        file_name: "m.bin".into(),
        total: None,
    });
    ui.apply_event(&AppEvent::DownloadProgress {
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
        ui.apply_event(&log(&format!("line {i}")));
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
    ui.apply_event(&AppEvent::Evicted {
        rom_id: 3,
        file_name: "old.nes".into(),
    });
    ui.apply_event(&AppEvent::EvictFailed {
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
        ui.apply_event(&AppEvent::DownloadStarted {
            rom_id: 7,
            file_id,
            file_name: format!("file{file_id}.nsp"),
            total: Some(10),
        });
    }
    ui.apply_event(&AppEvent::DownloadProgress {
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
        ui.apply_event(&AppEvent::DownloadFinished {
            rom_id: 7,
            file_id,
            file_name: format!("file{file_id}.nsp"),
        });
    }
    ui.apply_event(&AppEvent::DownloadFailed {
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

/// WS4.1: metadata progress tracks only the active pass. A new pass restarts
/// the counts, a stale pass cannot finish/revive a newer one, and cancellation
/// stops a late callback from resurrecting the pass.
#[test]
fn metadata_progress_tracks_the_active_pass_only() {
    let start = |job_id| AppEvent::FrontendMetadata {
        job_id,
        pending: true,
        detail: "Downloading artwork".into(),
        error: None,
    };
    let finish = |job_id| AppEvent::FrontendMetadata {
        job_id,
        pending: false,
        detail: "Artwork & gamelists up to date".into(),
        error: None,
    };
    let progress = |job_id, completed| AppEvent::FrontendMetadataProgress {
        job_id,
        completed,
        total: 10,
    };

    let mut ui = UiState::new(64);
    ui.apply_event(&start(1));
    assert!(ui.frontend_metadata_pending);
    ui.apply_event(&progress(1, 3));
    assert_eq!(ui.frontend_metadata_progress, Some((3, 10)));
    // Non-monotonic within the pass is ignored.
    ui.apply_event(&progress(1, 2));
    assert_eq!(ui.frontend_metadata_progress, Some((3, 10)));
    ui.apply_event(&progress(1, 10));
    assert_eq!(ui.frontend_metadata_progress, Some((10, 10)));

    // A finish clears the counts and records the age.
    ui.apply_event(&finish(1));
    assert!(!ui.frontend_metadata_pending);
    assert!(ui.frontend_metadata_progress.is_none());
    assert!(ui.frontend_metadata_updated_at.is_some());

    // Pass 2 starts; pass 1's late callbacks are ignored.
    ui.apply_event(&start(2));
    ui.apply_event(&progress(1, 9));
    assert!(ui.frontend_metadata_progress.is_none());
    ui.apply_event(&progress(2, 4));
    assert_eq!(ui.frontend_metadata_progress, Some((4, 10)));
    // A stale finish or start for pass 1 cannot end or revive pass 2.
    ui.apply_event(&finish(1));
    assert!(ui.frontend_metadata_pending);
    assert_eq!(ui.frontend_metadata_progress, Some((4, 10)));
    ui.apply_event(&start(1));
    assert!(ui.frontend_metadata_pending);

    // Cancellation invalidates pass 2; later events for it are ignored.
    ui.apply_event(&AppEvent::FrontendMetadataInvalidated { job_id: 2 });
    assert!(!ui.frontend_metadata_pending);
    assert!(ui.frontend_metadata_progress.is_none());
    ui.apply_event(&progress(2, 10));
    assert!(ui.frontend_metadata_progress.is_none());
    ui.apply_event(&start(2));
    assert!(!ui.frontend_metadata_pending);

    // A newer pass (3) still starts normally.
    ui.apply_event(&start(3));
    assert!(ui.frontend_metadata_pending);
    ui.apply_event(&progress(3, 1));
    assert_eq!(ui.frontend_metadata_progress, Some((1, 10)));
}
