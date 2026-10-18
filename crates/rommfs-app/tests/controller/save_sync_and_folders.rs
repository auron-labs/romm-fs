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

#[test]
fn transfer_stages_share_revision_identity_and_do_not_create_pending_work() {
    let mut ui = enabled_ui();
    for phase in ["captured", "uploading", "verified"] {
        ui.apply_event(&AppEvent::SaveSyncTransferProgress {
            session_id: 1,
            rom_id: 7,
            revision: "older".into(),
            phase: phase.into(),
            detail: None,
        });
    }
    assert_eq!(ui.save_sync_transfers.len(), 1);
    assert_eq!(ui.save_sync_transfers[0].phase, "verified");
    ui.apply_event(&AppEvent::SaveSyncTransferProgress {
        session_id: 1,
        rom_id: 7,
        revision: "newer".into(),
        phase: "captured".into(),
        detail: None,
    });
    ui.apply_event(&AppEvent::SaveSyncQueueUpdated(queue(1, 1, 1, 0, 0)));
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::Syncing);
    // An older completion cannot remove a different pending revision.
    ui.apply_event(&AppEvent::SaveSyncTransferProgress {
        session_id: 1,
        rom_id: 7,
        revision: "older".into(),
        phase: "verified".into(),
        detail: None,
    });
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::Syncing);
    ui.apply_event(&AppEvent::SaveSyncQueueUpdated(queue(1, 1, 0, 0, 0)));
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::UpToDate);
}

#[test]
fn checking_does_not_change_consent_and_stale_scan_results_are_ignored() {
    let mut ui = enabled_ui();
    ui.apply_event(&AppEvent::SaveSyncSessionChanged { session_id: 2 });
    assert!(ui.save_sync_enabled);
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::Checking);
    ui.apply_event(&AppEvent::SaveSyncScanFinished {
        session_id: 1,
        completed_at: 40,
    });
    assert_eq!(ui.save_scan_at, None);
    // A complete scan with zero files is still a successful scan.
    ui.apply_event(&AppEvent::SaveSyncScanFinished {
        session_id: 2,
        completed_at: 50,
    });
    assert_eq!(ui.save_scan_at, Some(50));
}

#[test]
fn review_is_visible_with_consent_off_and_beats_other_pending_work() {
    let mut ui = enabled_ui();
    let mut status = queue(2, 2, 1, 0, 1);
    status.failure = Some("unverified remote revision".into());
    ui.apply_event(&AppEvent::SaveSyncQueueUpdated(status));
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::NeedsAttention);
    ui.save_sync_enabled = false;
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::Off);
    assert!(ui.save_sync_attention().is_some());
    assert_eq!(ui.save_sync_games.len(), 2);
}

#[test]
fn stop_acknowledges_pending_and_completes_without_navigation() {
    let mut ui = UiState::new(32);
    ui.apply_event(&AppEvent::MountStarted {
        path: "C:/Games with spaces".into(),
    });
    ui.apply_event(&AppEvent::MountStopping);
    assert!(ui.mount_stopping);
    assert_eq!(ui.mount, MountState::Mounted);
    ui.apply_event(&AppEvent::MountStopped);
    assert!(!ui.mount_stopping);
    assert_eq!(ui.folder_summary(), FolderSummary::Stopped);
    ui.apply_event(&AppEvent::ShutdownStarting);
    assert!(ui.closing);
}

#[test]
fn skipped_locations_are_separate_from_linkable_files() {
    let mut ui = UiState::new(32);
    ui.save_file_unmatched = vec![
        rommfs_core::save_sync::UnmatchedFile {
            path: "memcards".into(),
            reason: "unsupported memory card".into(),
            mappable: false,
            no_candidate: false,
        },
        rommfs_core::save_sync::UnmatchedFile {
            path: "unknown.sav".into(),
            reason: "choose a game".into(),
            mappable: true,
            no_candidate: false,
        },
    ];
    assert_eq!(ui.save_files_ignored(), 1);
}

#[test]
fn six_review_versions_belong_to_one_save_and_do_not_change_another_slot() {
    let mut ui = enabled_ui();
    let mut status = queue(2, 2, 0, 0, 1);
    status.games[1].rom_id = status.games[0].rom_id;
    status.games[0].incoming_ids = (0..6).map(|ix| format!("incoming-{ix}")).collect();
    status.games[0].work = rommfs_core::events::SaveSyncWork::NeedsReview;
    status.incoming = status.games[0]
        .incoming_ids
        .iter()
        .map(|id| rommfs_core::events::SaveSyncIncomingStatus {
            incoming_id: id.clone(),
            rom_id: 0,
            rom_name: "Same game".into(),
            remote_id: id.clone(),
            content_hash: "verified-body".into(),
            reason: "ambiguous_remote_history".into(),
            state: "staged".into(),
        })
        .collect();
    ui.apply_event(&AppEvent::SaveSyncQueueUpdated(status));
    assert_eq!(ui.save_sync_games.len(), 2);
    assert_eq!(ui.save_sync_review_versions(0, 0).count(), 6);
    assert_eq!(ui.save_sync_review_versions(0, 1).count(), 0);
    assert_eq!(ui.save_sync_pending_incoming, 0);
    assert_eq!(ui.save_sync_attention_games, 1);
    assert!(ui
        .save_sync_summary_text()
        .starts_with("1 save needs review"));
}

#[test]
fn late_discovery_does_not_undo_a_deliberate_disable() {
    let mut ui = enabled_ui();
    ui.request_save_sync_enabled(false);
    let update = AppEvent::SaveSyncUpdated {
        session_id: 1,
        server_id: None,
        candidates: vec![],
        skipped: 0,
        selected_root: None,
        documented_saves_root: None,
        effective_saves_root: None,
        profile_version: None,
        account_id: None,
        mapped_targets: 0,
        catalogue_unmapped: 0,
        existing_saves: None,
        preview_saves: vec![],
        available: true,
        selected_problem: None,
        enabled: true,
        debounce_secs: 5,
    };
    ui.apply_event(&update);
    assert!(!ui.save_sync_enabled);
    ui.apply_event(&AppEvent::SaveSyncSessionChanged { session_id: 2 });
    assert!(!ui.save_sync_enabled);
    ui.apply_event(&update);
    assert!(!ui.save_sync_enabled);
}

#[test]
fn signout_clears_the_previous_identitys_consent_and_scan_time() {
    let mut ui = enabled_ui();
    ui.apply_event(&AppEvent::SaveSyncScanFinished {
        session_id: 1,
        completed_at: 50,
    });
    ui.apply_event(&AppEvent::SignedOut);
    assert!(!ui.save_sync_enabled);
    assert_eq!(ui.save_scan_at, None);
}

#[test]
fn a_check_retains_review_rows_and_history_while_rejecting_old_completions() {
    let mut ui = enabled_ui();
    ui.apply_event(&AppEvent::SaveSyncQueueUpdated(queue(1, 1, 0, 0, 0)));
    ui.apply_event(&AppEvent::SaveSyncCheckStarted { session_id: 2 });
    assert!(ui.save_sync_enabled);
    assert_eq!(ui.save_sync_games.len(), 1);
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::Checking);
    let mut stale = queue(0, 0, 0, 0, 0);
    stale.session_id = 1;
    ui.apply_event(&AppEvent::SaveSyncQueueUpdated(stale));
    assert_eq!(ui.save_sync_games.len(), 1);
}

#[test]
fn a_memory_card_conflict_is_visible_beside_healthy_tracked_saves() {
    let mut ui = enabled_ui();
    ui.apply_event(&AppEvent::SaveSyncQueueUpdated(queue(1, 1, 0, 0, 0)));
    ui.memory_card_conflicts
        .push(rommfs_core::events::MemoryCardConflictStatus {
            emulator: "pcsx2".into(),
            card_name: "Card 1".into(),
            relative_path: "memcards/Card 1.ps2".into(),
            remote_version_id: 31,
        });
    assert_eq!(ui.save_sync_review_count(), 1);
    assert_eq!(ui.save_sync_summary(), SaveSyncSummary::NeedsAttention);
    assert_eq!(ui.save_sync_summary_text(), "1 save needs review");
}
