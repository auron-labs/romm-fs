//! Save-sync setup, portable mapping, and durable local capture primitives.
//! Network transfer is intentionally outside this module.

mod installation;
mod journal;
mod links;
pub use links::{path_is_symbolic_link, remove_symbolic_link};
mod legacy_gb_profile;
mod mapping;
mod ordinary_files;
pub use ordinary_files::{ensure_distinct, SaveFilePlan, SaveFileRules, UnmatchedFile};
mod path;
mod preview;
mod retrobat_paths;
pub(crate) use retrobat_paths::{resolve_auxiliary_root, resolve_retrobat_paths};
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
pub use legacy_gb_profile::{resolve_retrobat_gb_profile, RetroBatGbProfile};
pub use mapping::{
    map_catalogue, MappingIssue, MappingReport, SaveMapping, SaveProfile, UnmappedEntry,
    RETROBAT_GB_SRM_PROFILE,
};
pub use path::{
    ensure_no_reparse_components, hold_save_directory_chain, path_is_reparse_point,
    resolve_save_target, validate_relative_save_path, validate_windows_path_component,
    SaveDirectoryChainGuard, MAX_SAVE_BYTES,
};
pub use preview::{
    preview_existing_saves, ExistingSavePreview, ExistingSaveScanStatus, ExistingSaveSkipCount,
    ExistingSaveSkipReason, MAX_EXISTING_SAVE_SCAN_DEPTH, MAX_EXISTING_SAVE_SCAN_ENTRIES,
};
pub use scheduler::{DueSnapshot, ObserveResult, RestoreReport, SaveSyncScheduler};
pub use settings::{
    paths_overlap, ConsentSettings, SaveSyncScope, SaveSyncSettingsStore, DEFAULT_DEBOUNCE_SECS,
};

pub fn sha256_content_hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("sha256:{:x}", Sha256::digest(bytes))
}

pub fn hash_save_content(reader: &mut impl std::io::Read) -> crate::Result<String> {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("sha256:{:x}", hash.finalize()))
}
mod file_identity;
pub use file_identity::{
    file_slot, path_from_file_slot, portable_save_path, save_file_key, FILE_SLOT_PREFIX,
    GENERIC_FILE_PROFILE,
};
