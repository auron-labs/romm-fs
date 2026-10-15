// Behavioral tests for public controller state transitions.
use super::*;

/// Auxiliary support-file downloads have their own status fields — they must
/// never masquerade as RetroBat setup (and vice versa).
#[test]
fn auxiliary_status_stays_separate_from_retrobat_setup() {
    let mut ui = UiState::new(64);
    ui.retrobat_setup_running = true;
    ui.apply_event(&AppEvent::AuxiliaryStatus {
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

    ui.apply_event(&AppEvent::AuxiliaryStatus {
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
    ui.apply_event(&AppEvent::RetroBatInstaller {
        running: false,
        ready: true,
        received: 0,
        total: None,
        detail: "ready".into(),
        error: Some("Windows needs permission (os error 740)".into()),
    });
    ui.apply_event(&AppEvent::RetroBatSetup {
        running: false,
        detail: "failed".into(),
        skipped: Vec::new(),
        error: Some("link failed".into()),
    });
    ui.apply_event(&AppEvent::AuxiliaryStatus {
        running: false,
        detail: "failed".into(),
        error: Some("nope".into()),
        summary: None,
    });
    // Selecting an installation drops the installer error and, because the
    // installation changed, the results of the previous one.
    ui.apply_event(&AppEvent::IntegrationUpdated {
        mode: IntegrationMode::RetroBat,
        retrobat_root: Some("C:\\RetroBat".into()),
        setup_complete: false,
        setup_migration: false,
        error: None,
    });
    assert!(ui.installer_error.is_none());
    assert!(ui.retrobat_setup_error.is_none() && ui.retrobat_setup_detail.is_none());
    assert!(ui.auxiliary_error.is_none() && ui.auxiliary_detail.is_none());

    // A failure for this installation survives re-applying the same selection.
    ui.apply_event(&AppEvent::RetroBatSetup {
        running: false,
        detail: "failed".into(),
        skipped: Vec::new(),
        error: Some("link failed".into()),
    });
    ui.apply_event(&AppEvent::IntegrationUpdated {
        mode: IntegrationMode::RetroBat,
        retrobat_root: Some("C:\\RetroBat".into()),
        setup_complete: false,
        setup_migration: false,
        error: None,
    });
    assert!(ui.retrobat_setup_error.is_some());

    // Restarting setup clears any installer error and the previous setup error.
    ui.installer_error = Some("again".into());
    ui.apply_event(&AppEvent::RetroBatSetup {
        running: true,
        detail: "working".into(),
        skipped: Vec::new(),
        error: None,
    });
    assert!(ui.installer_error.is_none() && ui.retrobat_setup_error.is_none());
}

#[test]
fn auxiliary_summary_carries_structured_notes_and_restart_clears_it() {
    use rommfs_core::events::{AuxiliarySummary, AuxiliaryWarning, AuxiliaryWarningKind};
    let mut ui = UiState::new(64);
    ui.apply_event(&AppEvent::AuxiliaryStatus {
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
    ui.apply_event(&AppEvent::AuxiliaryStatus {
        running: true,
        detail: "again".into(),
        error: None,
        summary: None,
    });
    assert!(ui.auxiliary_summary.is_none());
}
