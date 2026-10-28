//! RetroBat setup and frontend metadata. RomM games are linked in alongside the
//! user's own ROMs; setup never moves, replaces or deletes existing files.
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
mod owned;
mod play_data;
mod setup;
mod xml;

pub use artwork::{cache_artwork, cache_artwork_cancellable, ArtworkBatch};
pub use config::{configure_switch_saves, switch_save_setting};
pub use installer::{
    describe_launch_failure, discover_installer, download_installer, launch_installer,
    InstallerRelease, LaunchFailure, LaunchedInstaller,
};
pub use metadata::{
    generate_gamelists, publish_gamelists, read_gamelists, FrontendGamelists, MetadataIndex,
    MetadataPublication, PlaySync, PLAY_FAMILY_NAME,
};
pub use owned::{LocalFiles, HASH_CAP_BYTES};
pub use play_data::{
    build_remote_play, is_play_field, merge_play, parse_play_fields, render_play_fields,
    PlayBaseline, PlayMerge, PlayPush, PlayValues, PLAY_FIELDS,
};
pub use setup::{
    inspect_setup, inspect_setup_with_root_check, linked_systems, linked_systems_with_root_check,
    setup_needs_migration, setup_roms_link, setup_roms_link_with_root_guard, verify_setup_link,
    SetupOutcome, SetupRecord, SkippedSystem, ROMM_FOLDER,
};

/// Resolve effective frontend home/save directories through launcher settings.
pub fn launcher_paths(
    installation: &std::path::Path,
) -> crate::Result<(std::path::PathBuf, std::path::PathBuf)> {
    crate::save_sync::resolve_retrobat_paths(installation)
}
