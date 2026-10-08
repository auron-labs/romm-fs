//! Fresh-installation setup and local frontend metadata. ROM ownership remains
//! the native adapter's responsibility; setup never migrates an existing library.
mod artwork;
mod config;
mod metadata;
mod setup;
mod xml;

pub use artwork::{cache_artwork, cache_artwork_cancellable, ArtworkBatch};
pub use config::{configure_switch_saves, switch_save_setting};
pub use metadata::{generate_gamelists, MetadataIndex, MetadataPublication};
pub use setup::{setup_roms_link, verify_setup_link, SetupRecord};

/// Resolve effective frontend home/save directories through launcher settings.
pub fn launcher_paths(
    installation: &std::path::Path,
) -> crate::Result<(std::path::PathBuf, std::path::PathBuf)> {
    crate::save_sync::resolve_retrobat_paths(installation)
}
