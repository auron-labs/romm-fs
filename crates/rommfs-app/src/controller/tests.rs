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

#[path = "tests/catalogue.rs"]
mod catalogue;

#[path = "tests/connection.rs"]
mod connection;

#[path = "tests/mount.rs"]
mod mount;

#[path = "tests/save_sync.rs"]
mod save_sync;
