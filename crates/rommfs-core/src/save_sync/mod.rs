//! Save-sync setup, portable mapping, and durable local capture primitives.
//! Network transfer is intentionally outside this module.

mod installation;
mod journal;
mod mapping;
mod path;
mod preview;
mod profile;
mod scheduler;
mod settings;

pub use installation::{
    discover_installations, installation_root_from_process_image, validate_installation,
    DiscoveryInput, DiscoveryReport, InstallationCandidate, InstallationInfo, InstallationProblem,
    InstallationSource,
};
pub use journal::{
    IncomingSaveRecord, JournalSlot, LocalObservation, SaveSyncJournal, SnapshotRecord,
    SnapshotState,
};
pub use mapping::{
    map_catalogue, MappingIssue, MappingReport, SaveMapping, SaveProfile, UnmappedEntry,
    RETROBAT_GB_SRM_PROFILE,
};
pub use path::{
    hold_save_directory_chain, path_is_reparse_point, resolve_save_target,
    validate_relative_save_path, validate_windows_path_component, SaveDirectoryChainGuard,
    MAX_SAVE_BYTES,
};
pub use preview::{
    preview_existing_saves, ExistingSavePreview, ExistingSaveScanStatus, ExistingSaveSkipCount,
    ExistingSaveSkipReason, MAX_EXISTING_SAVE_SCAN_DEPTH, MAX_EXISTING_SAVE_SCAN_ENTRIES,
};
pub use profile::{resolve_retrobat_gb_profile, RetroBatGbProfile};
pub use scheduler::{DueSnapshot, ObserveResult, RestoreReport, SaveSyncScheduler};
pub use settings::{ConsentSettings, SaveSyncScope, SaveSyncSettingsStore, DEFAULT_DEBOUNCE_SECS};

pub fn sha256_content_hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("sha256:{:x}", Sha256::digest(bytes))
}
