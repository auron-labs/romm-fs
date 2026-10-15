//! Commands crossing the window-to-controller boundary.
use super::*;

/// Commands the window sends to the controller.
pub enum Command {
    CancelAuxiliary,
    MaterializeAuxiliary,
    SetStartAtLogin {
        enabled: bool,
    },
    SelectSaveFolder {
        path: PathBuf,
    },
    AssociateSaveFile {
        relative_path: PathBuf,
        rom_id: i64,
    },
    /// Associate every eligible unmatched save file under a save-root-relative
    /// directory group with one game (not its descendant groups).
    AssociateSaveFolder {
        prefix: PathBuf,
        rom_id: i64,
    },
    DownloadRetroBatInstaller,
    CancelRetroBatInstaller,
    LaunchRetroBatInstaller,
    ShowRetroBatInstaller,
    SetupRetroBat {
        installation: PathBuf,
        managed_root: PathBuf,
    },
    /// Delete foreign symlinks/junctions (never data-bearing reparse points) under a refused
    /// mount root, then republish the remaining foreign list.
    RemoveForeignLinks {
        root: PathBuf,
    },
    SetIntegration {
        mode: IntegrationMode,
        retrobat_root: Option<PathBuf>,
    },
    RestoreConnection,
    BeginPairing {
        url: String,
    },
    CancelPairing,
    Logout,
    Connect {
        url: String,
        username: String,
        password: String,
    },
    /// Apply the games-folder path typed in Settings to the session's
    /// remembered config — validation stays with StartMount.
    SetMountRoot {
        path: String,
    },
    StartMount {
        path: String,
    },
    StopMount,
    /// Stop and re-start the games folder in one worker step, publishing a
    /// pending catalogue refresh (phase 2b "Update ready").
    RestartMount {
        path: String,
    },
    RefreshCatalogue,
    RefreshSaveSync,
    SelectSaveSyncInstallation {
        path: PathBuf,
    },
    SetSaveSyncEnabled {
        enabled: bool,
    },
    SetSaveSyncDebounce {
        seconds: u32,
    },
    /// Turn favourites and play-history sync on or off for this server and
    /// account. Consent is never inferred.
    SetPlayDataSync {
        enabled: bool,
    },
    ExportSaveSyncIncoming {
        session_id: u64,
        incoming_id: String,
        destination: PathBuf,
    },
    Shutdown,
}

pub(super) struct CommandEnvelope {
    pub(super) command: Command,
    pub(super) save_sync_epoch: u64,
    pub(super) connection_epoch: u64,
}

pub(super) fn invalidates_save_sync_scope(command: &Command) -> bool {
    matches!(
        command,
        Command::SetIntegration { .. }
            | Command::SelectSaveFolder { .. }
            | Command::AssociateSaveFile { .. }
            | Command::AssociateSaveFolder { .. }
            | Command::SetupRetroBat { .. }
            | Command::BeginPairing { .. }
            | Command::CancelPairing
            | Command::Logout
            | Command::RestoreConnection
            | Command::Connect { .. }
            | Command::StartMount { .. }
            | Command::RestartMount { .. }
            | Command::RefreshSaveSync
            | Command::SelectSaveSyncInstallation { .. }
            | Command::SetSaveSyncEnabled { enabled: false }
            | Command::Shutdown
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mounting_commands_invalidate_the_save_sync_scope() {
        // A restart is a stop and a start, so it must invalidate like StartMount.
        for command in [
            Command::StartMount {
                path: "/srv/games".into(),
            },
            Command::RestartMount {
                path: "/srv/games".into(),
            },
        ] {
            assert!(invalidates_save_sync_scope(&command));
        }
    }
}
