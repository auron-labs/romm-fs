//! Controller behavior tests grouped by lifecycle and workflow.
use super::*;
use rommfs_core::cache::FakeClock;
use rommfs_core::romm::{PlatformDto, RomDto, RomFileDto, SaveSyncIdentity};
use rommfs_core::save_sync::RETROBAT_GB_SRM_PROFILE;
use rommfs_fixture::{FixtureBodyBarrier, FixtureSaveRecord, FixtureServer, ResponseSpec};
use std::io::Write;

struct UnusedSource;

impl ContentSource for UnusedSource {
    fn fetch(
        &self,
        _key: &RomKey,
        _writer: &mut dyn Write,
        _progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<u64> {
        unreachable!("the eviction test never reads ROM content")
    }
}

fn create_test_retrobat_install(root: &Path, create_saves: bool) {
    let launcher_home = root.join("emulationstation/.emulationstation");
    std::fs::create_dir_all(root.join("system")).unwrap();
    std::fs::create_dir_all(root.join("emulators/retroarch")).unwrap();
    std::fs::create_dir_all(&launcher_home).unwrap();
    if create_saves {
        std::fs::create_dir_all(root.join("saves")).unwrap();
    }
    std::fs::write(root.join("RetroBat.exe"), b"exe").unwrap();
    std::fs::write(root.join("system/version.info"), "8.2.1\n").unwrap();
    std::fs::write(
            root.join("emulators/retroarch/retroarch.cfg"),
            "savefile_directory = \":\\saves\"\nsavefiles_in_content_dir = \"false\"\nsort_savefiles_enable = \"false\"\n",
        )
        .unwrap();
    std::fs::write(
        root.join("emulationstation/emulatorLauncher.cfg"),
        "home=.\\.emulationstation\nsaves=.\\..\\saves\n",
    )
    .unwrap();
    std::fs::write(launcher_home.join("es_settings.cfg"), "<config/>\n").unwrap();
    std::fs::write(
            launcher_home.join("es_systems.cfg"),
            r#"<systemList><system><name>gb</name><command>"%HOME%\emulatorLauncher.exe" -gameinfo %GAMEINFOXML% %CONTROLLERSCONFIG% -system %SYSTEM% -emulator %EMULATOR% -core %CORE% -rom %ROM%</command><emulators><emulator name="libretro"><cores><core>gambatte</core></cores></emulator></emulators></system></systemList>"#,
        )
        .unwrap();
}

#[test]
fn integration_and_metadata_summaries_follow_the_latest_result() {
    let mut state = UiState::new(10);
    let root = Some("C:/RetroBat".to_string());
    state.apply_event(&AppEvent::IntegrationUpdated {
        mode: IntegrationMode::RetroBat,
        retrobat_root: root.clone(),
        setup_complete: true,
        setup_migration: false,
        setup_issues: Vec::new(),
        error: None,
    });
    assert!(state.retrobat_setup_ok);
    state.apply_event(&AppEvent::IntegrationUpdated {
        mode: IntegrationMode::RetroBat,
        retrobat_root: root,
        setup_complete: false,
        setup_migration: false,
        setup_issues: Vec::new(),
        error: None,
    });
    assert!(!state.retrobat_setup_ok);

    let (sink, events) = rommfs_core::events::channel();
    let mut worker = Worker::new(sink);
    worker.metadata_job_id = 1;
    worker.metadata_status(true, "Waiting for RetroBat.exe to close", None);
    for event in events.try_iter() {
        state.apply_event(&event);
    }
    assert_eq!(
        state.frontend_metadata_summary(),
        FrontendMetadataSummary::Waiting
    );
    assert!(state.frontend_metadata_updated_at.is_none());

    state.apply_event(&AppEvent::FrontendMetadata {
        job_id: 1,
        pending: true,
        detail: "Publishing game information".into(),
        error: None,
    });
    state.apply_event(&AppEvent::FrontendMetadata {
        job_id: 1,
        pending: false,
        detail: "Updated with issues; 1 system left as it was".into(),
        error: Some("one system failed".into()),
    });
    assert_eq!(
        state.frontend_metadata_summary(),
        FrontendMetadataSummary::UpdatedWithIssues
    );
}

#[path = "tests/catalogue.rs"]
mod catalogue;

#[path = "tests/connection.rs"]
mod connection;

#[path = "tests/mount.rs"]
mod mount;

#[path = "tests/save_sync.rs"]
mod save_sync;

#[test]
fn inspected_retrobat_readiness_matches_the_selected_folders() {
    let mut state = UiState::new(10);
    for usable in [true, false] {
        state.apply_event(&AppEvent::RetroBatSetup {
            running: true,
            detail: "Connecting".into(),
            skipped: Vec::new(),
            error: None,
        });
        state.apply_event(&AppEvent::MountRootConfigured {
            server: "server".into(),
            mount_root: "games-A".into(),
        });
        state.apply_event(&AppEvent::IntegrationUpdated {
            mode: IntegrationMode::RetroBat,
            retrobat_root: Some("RetroBat".into()),
            setup_complete: usable,
            setup_migration: false,
            setup_issues: vec![rommfs_core::retrobat::SkippedSystem {
                system: "snes".into(),
                reason: "Busy".into(),
            }],
            error: None,
        });
        state.apply_event(&AppEvent::RetroBatSetup {
            running: false,
            detail: "Finished".into(),
            skipped: Vec::new(),
            error: None,
        });
        assert_eq!(
            state.retrobat_ready_for(Some(Path::new("RetroBat")), Path::new("games-A")),
            usable
        );
        assert!(!state.retrobat_ready_for(Some(Path::new("RetroBat")), Path::new("games-B")));
        assert!(!state.retrobat_ready_for(Some(Path::new("Other")), Path::new("games-A")));
    }
}

#[test]
fn reconnecting_the_selected_games_folder_makes_it_ready() {
    let mut state = UiState::new(10);
    for games in ["games-A", "games-B"] {
        if games == "games-B" {
            assert!(!state.retrobat_ready_for(Some(Path::new("RetroBat")), Path::new(games)));
        }
        state.apply_event(&AppEvent::MountRootConfigured {
            server: "server".into(),
            mount_root: games.into(),
        });
        state.apply_event(&AppEvent::IntegrationUpdated {
            mode: IntegrationMode::RetroBat,
            retrobat_root: Some("RetroBat".into()),
            setup_complete: true,
            setup_migration: false,
            setup_issues: Vec::new(),
            error: None,
        });
        assert!(state.retrobat_ready_for(Some(Path::new("RetroBat")), Path::new(games)));
    }
}
