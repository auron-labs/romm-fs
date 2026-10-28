// Behavioral tests for public controller state transitions.
use super::*;

#[test]
fn retrobat_steps_unlock_in_order_with_one_primary() {
    use rommfs_app::controller::{RetroBatStep, StepStatus};
    let mut ui = linked_ui();
    let steps = ui.retrobat_steps();
    assert_eq!(steps.install, StepStatus::Ready);
    assert!(matches!(steps.link, StepStatus::Blocked(_)));
    assert!(matches!(steps.support, StepStatus::Blocked(_)));
    assert_eq!(steps.primary(), Some(RetroBatStep::Install));

    ui.apply_event(&AppEvent::IntegrationUpdated {
        mode: IntegrationMode::RetroBat,
        retrobat_root: Some("C:\\RetroBat".into()),
        setup_complete: false,
        setup_migration: false,
        setup_issues: Vec::new(),
        error: None,
    });
    let steps = ui.retrobat_steps();
    assert_eq!(steps.install, StepStatus::Done);
    assert_eq!(steps.link, StepStatus::Ready);
    assert_eq!(steps.primary(), Some(RetroBatStep::Link));
    // Selecting an installation is not the same as the layout being set up.
    assert!(!ui.retrobat_setup_ok);

    ui.apply_event(&AppEvent::RetroBatSetup {
        running: true,
        detail: "working".into(),
        skipped: Vec::new(),
        error: None,
    });
    assert_eq!(ui.retrobat_steps().link, StepStatus::Running);
    assert_eq!(ui.retrobat_steps().primary(), None);

    ui.apply_event(&AppEvent::RetroBatSetup {
        running: false,
        detail: "linked".into(),
        skipped: Vec::new(),
        error: None,
    });
    let steps = ui.retrobat_steps();
    assert_eq!(steps.link, StepStatus::Done);
    // Support files also need the library to be loaded.
    assert!(matches!(steps.support, StepStatus::Blocked(_)));
    ui.catalogue = Some((3, 10, 10, 0));
    let steps = ui.retrobat_steps();
    assert_eq!(steps.support, StepStatus::Ready);
    assert_eq!(steps.primary(), Some(RetroBatStep::Support));
    assert!(ui.integration_notice.is_none());

    // A failed inspection clears the result even for the same installation.
    ui.apply_event(&AppEvent::IntegrationUpdated {
        mode: IntegrationMode::RetroBat,
        retrobat_root: Some("C:\\RetroBat".into()),
        setup_complete: false,
        setup_migration: false,
        setup_issues: Vec::new(),
        error: None,
    });
    assert_eq!(ui.retrobat_steps().link, StepStatus::Ready);
    ui.apply_event(&AppEvent::IntegrationUpdated {
        mode: IntegrationMode::RetroBat,
        retrobat_root: Some("D:\\RetroBat".into()),
        setup_complete: false,
        setup_migration: false,
        setup_issues: Vec::new(),
        error: None,
    });
    assert_eq!(ui.retrobat_steps().link, StepStatus::Ready);

    // A verified record on disk counts after a restart.
    ui.apply_event(&AppEvent::IntegrationUpdated {
        mode: IntegrationMode::RetroBat,
        retrobat_root: Some("E:\\RetroBat".into()),
        setup_complete: true,
        setup_migration: false,
        setup_issues: Vec::new(),
        error: None,
    });
    assert_eq!(ui.retrobat_steps().link, StepStatus::Done);

    // A running games folder blocks (re)setup.
    ui.apply_event(&AppEvent::MountStarted {
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
    ui.catalogue = Some((3, 10, 10, 0));
    ui.apply_event(&AppEvent::IntegrationUpdated {
        mode: IntegrationMode::RetroBat,
        retrobat_root: Some("C:\\RetroBat".into()),
        setup_complete: false,
        setup_migration: false,
        setup_issues: Vec::new(),
        error: None,
    });
    ui.apply_event(&AppEvent::RetroBatSetup {
        running: false,
        detail: "failed".into(),
        skipped: Vec::new(),
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
    ui.apply_event(&AppEvent::IntegrationUpdated {
        mode: IntegrationMode::RetroBat,
        retrobat_root: Some("C:\\RetroBat".into()),
        setup_complete: false,
        setup_migration: false,
        setup_issues: Vec::new(),
        error: None,
    });
    ui.apply_event(&AppEvent::RetroBatSetup {
        running: false,
        detail: "failed".into(),
        skipped: Vec::new(),
        error: Some("denied".into()),
    });
    assert_eq!(ui.retrobat_steps().link, StepStatus::Failed);
}

#[test]
fn an_outdated_setup_record_is_a_migration_prompt_not_a_blocked_start() {
    use rommfs_app::controller::StepStatus;
    let mut ui = linked_ui();
    // An earlier build's record: not complete, but step 2 is actionable, so the
    // Overview offers the update instead of a guaranteed-to-fail start.
    ui.apply_event(&AppEvent::IntegrationUpdated {
        mode: IntegrationMode::RetroBat,
        retrobat_root: Some("C:\\RetroBat".into()),
        setup_complete: false,
        setup_migration: true,
        setup_issues: Vec::new(),
        error: None,
    });
    assert!(ui.retrobat_setup_migration);
    assert!(!ui.retrobat_setup_ok);
    assert_eq!(ui.retrobat_steps().link, StepStatus::Ready);

    // Running setup clears the prompt; the next verified event reports current.
    ui.apply_event(&AppEvent::RetroBatSetup {
        running: false,
        detail: "linked".into(),
        skipped: Vec::new(),
        error: None,
    });
    assert!(ui.retrobat_setup_ok);
    ui.apply_event(&AppEvent::IntegrationUpdated {
        mode: IntegrationMode::RetroBat,
        retrobat_root: Some("C:\\RetroBat".into()),
        setup_complete: true,
        setup_migration: false,
        setup_issues: Vec::new(),
        error: None,
    });
    assert!(!ui.retrobat_setup_migration);
    assert!(ui.retrobat_setup_ok);
}

#[test]
fn loaded_catalogue_means_the_folder_is_not_unconfigured() {
    let mut ui = UiState::new(64);
    ui.catalogue = Some((3, 10, 10, 0));
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

#[test]
fn eligible_files_remain_linkable_even_without_automatic_candidates() {
    use rommfs_core::save_sync::UnmatchedFile;
    let mut ui = linked_ui();
    let file = |name: &str, no_candidate| UnmatchedFile {
        path: name.into(),
        reason: "select the owning game".into(),
        mappable: true,
        no_candidate,
    };
    ui.save_file_unmatched = vec![file("a.nvmem", true), file("b.sav", false)];
    assert_eq!(ui.save_files_ignored(), 0);
}
