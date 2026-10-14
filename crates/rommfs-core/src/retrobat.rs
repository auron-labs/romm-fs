//! Fresh-installation setup and local frontend metadata. ROM ownership remains
//! the native adapter's responsibility; setup never migrates an existing library.
mod artwork;
mod auxiliary;
mod config;
mod installer;
pub use auxiliary::{materialize_auxiliaries, AuxiliaryReport};

pub fn auxiliary_paths(
    installation: &std::path::Path,
) -> crate::Result<(std::path::PathBuf, std::path::PathBuf)> {
    Ok((
        crate::save_sync::resolve_auxiliary_root(installation, "bios")?,
        crate::save_sync::resolve_auxiliary_root(installation, "cheats")?,
    ))
}
mod metadata;
mod setup;
mod xml;

pub use artwork::{cache_artwork, cache_artwork_cancellable, ArtworkBatch};
pub use config::{configure_switch_saves, switch_save_setting};
pub use installer::{
    describe_launch_failure, discover_installer, download_installer, launch_installer,
    InstallerRelease, LaunchFailure, LaunchedInstaller,
};
pub use metadata::{generate_gamelists, MetadataIndex, MetadataPublication};
pub use setup::{inspect_setup, setup_roms_link, verify_setup_link, SetupOutcome, SetupRecord};

/// Resolve effective frontend home/save directories through launcher settings.
pub fn launcher_paths(
    installation: &std::path::Path,
) -> crate::Result<(std::path::PathBuf, std::path::PathBuf)> {
    crate::save_sync::resolve_retrobat_paths(installation)
}
