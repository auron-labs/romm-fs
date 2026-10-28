use super::gate::SaveSyncEnablement;
use chrono::{DateTime, Utc};
use rommfs_core::error::Error;
use rommfs_core::romm::{RemoteSave, RommClient, SaveApiFailure, SaveSyncIdentity};
use rommfs_core::save_sync::{sha256_content_hash, SaveMapping, SaveSyncScope, MAX_SAVE_BYTES};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub(super) struct IncomingJob {
    pub(super) mapping: SaveMapping,
    pub(super) identity: SaveSyncIdentity,
    pub(super) scope: SaveSyncScope,
    pub(super) verify_reviewed_history: bool,
}

#[derive(Clone, Debug)]
pub(super) struct VerifiedRemoteSave {
    pub(super) metadata: RemoteSave,
    pub(super) content_hash: String,
    pub(super) bytes: Vec<u8>,
    pub(super) file: Option<Arc<tempfile::NamedTempFile>>,
    pub(super) history_time: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub(super) struct IncomingInventory {
    pub(super) saves: Vec<VerifiedRemoteSave>,
    pub(super) rejected: Vec<RejectedRemoteSave>,
    pub(super) history_fingerprint: Vec<String>,
}

#[derive(Clone, Debug)]
pub(super) struct RejectedRemoteSave {
    pub(super) id: String,
    pub(super) reason: String,
}

pub(super) fn remote_save_fingerprint(save: &RemoteSave) -> String {
    let fields = [
        save.id.to_string(),
        save.rom_id.to_string(),
        save.user_id.to_string(),
        save.file_name.clone(),
        save.file_size_bytes.to_string(),
        save.missing_from_fs.to_string(),
        save.created_at.clone(),
        save.updated_at.clone(),
        save.emulator.clone().unwrap_or_default(),
        save.slot.clone().unwrap_or_default(),
    ];
    let mut encoded = Vec::new();
    for field in fields {
        encoded.extend_from_slice(&(field.len() as u64).to_be_bytes());
        encoded.extend_from_slice(field.as_bytes());
    }
    format!("{}:{}", save.id, sha256_content_hash(&encoded))
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CacheKey {
    server_id: String,
    account_id: i64,
    rom_id: i64,
    slot: String,
    save_id: i64,
}

#[derive(Clone)]
struct CachedRemoteSave {
    metadata: RemoteSave,
    bytes: Vec<u8>,
    file: Option<Arc<tempfile::NamedTempFile>>,
    content_hash: String,
}

#[derive(Default)]
pub(super) struct IncomingContentCache {
    scope: Option<SaveSyncScope>,
    saves: HashMap<CacheKey, CachedRemoteSave>,
}

impl IncomingContentCache {
    fn enter_scope(&mut self, scope: &SaveSyncScope) {
        if self.scope.as_ref() != Some(scope) {
            self.saves.clear();
            self.scope = Some(scope.clone());
        }
    }
}

pub(super) fn reconcile_attempt(
    client: &RommClient,
    job: &IncomingJob,
    cache: &mut IncomingContentCache,
    enabled: &SaveSyncEnablement,
    active: &AtomicBool,
) -> std::result::Result<IncomingInventory, SaveApiFailure> {
    if !super::transfer::is_active(enabled, active) {
        return Err(cancelled_failure());
    }
    super::transfer::validate_configuration(
        &job.scope,
        std::slice::from_ref(&job.mapping),
        &job.identity,
    )
    .map_err(|error| SaveApiFailure {
        error,
        retry_after: None,
    })?;
    cache.enter_scope(&job.scope);
    let slot = job.mapping.remote_slot().map_err(|error| SaveApiFailure {
        error,
        retry_after: None,
    })?;
    let mut inventory = client.save_inventory(job.mapping.rom_key.rom_id, &slot)?;
    if let Some(legacy) = job.mapping.legacy_slot() {
        inventory.extend(client.save_inventory(job.mapping.rom_key.rom_id, legacy)?);
    }
    if let Some(message) = validate_inventory_identity(&inventory, job) {
        return Err(SaveApiFailure {
            error: Error::InvalidCatalogue(message),
            retry_after: None,
        });
    }
    let present_ids = inventory
        .iter()
        .map(|metadata| metadata.id)
        .collect::<HashSet<_>>();
    cache.saves.retain(|key, _| {
        key.rom_id != job.mapping.rom_key.rom_id
            || key.slot != slot
            || present_ids.contains(&key.save_id)
    });
    let mut saves = Vec::new();
    let mut rejected = Vec::new();
    let mut history_fingerprint = Vec::new();
    let mut seen_ids = HashSet::new();
    for metadata in inventory {
        if metadata.id <= 0 || !seen_ids.insert(metadata.id) {
            return Err(SaveApiFailure {
                error: Error::InvalidCatalogue(
                    "RomM save history contains duplicate or invalid IDs".into(),
                ),
                retry_after: None,
            });
        }
        let reason = if owned_revision_from_filename(&metadata.file_name).is_none() {
            Some("filename is outside the verified owned revision format")
        } else if metadata.missing_from_fs {
            Some("file is missing from RomM storage")
        } else if job.mapping.profile.id != rommfs_core::save_sync::GENERIC_FILE_PROFILE
            && (metadata.file_size_bytes == 0 || metadata.file_size_bytes > MAX_SAVE_BYTES)
        {
            Some("size is outside the supported Game Boy save range")
        } else {
            None
        };
        if let Some(reason) = reason {
            history_fingerprint.push(format!("{}\t-", remote_save_fingerprint(&metadata)));
            rejected.push(RejectedRemoteSave {
                id: metadata.id.to_string(),
                reason: format!("{} ({})", metadata.file_name, reason),
            });
            continue;
        }
        let history_time = parse_history_time(&metadata.updated_at)
            .or_else(|| parse_history_time(&metadata.created_at))
            .ok_or_else(|| SaveApiFailure {
                error: Error::InvalidCatalogue(
                    "RomM save history has malformed ordering timestamps".into(),
                ),
                retry_after: None,
            })?;
        let key = CacheKey {
            server_id: job.scope.server_id.clone(),
            account_id: job.identity.account_id,
            rom_id: metadata.rom_id,
            slot: metadata.slot.clone().unwrap_or_default(),
            save_id: metadata.id,
        };
        let cached = cache
            .saves
            .get(&key)
            .filter(|cached| cached.metadata == metadata && !job.verify_reviewed_history)
            .cloned();
        let (bytes, file, content_hash) = if let Some(cached) = cached {
            (cached.bytes, cached.file, cached.content_hash)
        } else {
            if !super::transfer::is_active(enabled, active) {
                return Err(cancelled_failure());
            }
            let (bytes, file, content_hash) =
                if job.mapping.profile.id == rommfs_core::save_sync::GENERIC_FILE_PROFILE {
                    let mut file = tempfile::NamedTempFile::new().map_err(|e| SaveApiFailure {
                        error: e.into(),
                        retry_after: None,
                    })?;
                    client.download_save_to(
                        metadata.id,
                        metadata.file_size_bytes,
                        &mut file,
                        active,
                    )?;
                    file.as_file().sync_all().map_err(|e| SaveApiFailure {
                        error: e.into(),
                        retry_after: None,
                    })?;
                    let mut reader = file.reopen().map_err(|e| SaveApiFailure {
                        error: e.into(),
                        retry_after: None,
                    })?;
                    let hash = rommfs_core::save_sync::hash_save_content(&mut reader).map_err(
                        |error| SaveApiFailure {
                            error,
                            retry_after: None,
                        },
                    )?;
                    (Vec::new(), Some(Arc::new(file)), hash)
                } else {
                    let bytes = client.download_save_content(metadata.id)?;
                    let hash = sha256_content_hash(&bytes);
                    (bytes, None, hash)
                };
            if file.is_none() && bytes.len() as u64 != metadata.file_size_bytes {
                return Err(SaveApiFailure {
                    error: Error::InvalidCatalogue(format!(
                        "RomM save {} content size did not match inventory",
                        metadata.id
                    )),
                    retry_after: None,
                });
            }
            cache.saves.insert(
                key.clone(),
                CachedRemoteSave {
                    metadata: metadata.clone(),
                    bytes: bytes.clone(),
                    file: file.clone(),
                    content_hash: content_hash.clone(),
                },
            );
            while cache.saves.len() > 16 {
                let Some(oldest_key) = cache
                    .saves
                    .keys()
                    .find(|cached_key| **cached_key != key)
                    .cloned()
                else {
                    break;
                };
                cache.saves.remove(&oldest_key);
            }
            (bytes, file, content_hash)
        };
        history_fingerprint.push(format!(
            "{}\t{}",
            remote_save_fingerprint(&metadata),
            content_hash
        ));
        saves.push(VerifiedRemoteSave {
            metadata,
            content_hash,
            bytes,
            file,
            history_time,
        });
    }
    history_fingerprint.sort();
    Ok(IncomingInventory {
        saves,
        rejected,
        history_fingerprint,
    })
}

fn validate_inventory_identity(inventory: &[RemoteSave], job: &IncomingJob) -> Option<String> {
    inventory
        .iter()
        .any(|save| {
            save.rom_id != job.mapping.rom_key.rom_id
                || save.user_id != job.identity.account_id
                || !job.mapping.accepts_remote_slot(save.slot.as_deref())
        })
        .then(|| {
            "RomM save inventory did not match the authenticated account, ROM, and slot".into()
        })
}

pub(super) fn owned_revision_from_filename(filename: &str) -> Option<&str> {
    let name = filename.strip_prefix("rommfs-")?;
    let stem = name
        .strip_suffix(".srm")
        .or_else(|| name.strip_suffix(".bin"))?;
    let revision = if let Some((revision, tag)) = stem.split_once(" [") {
        if !is_server_datetime_tag(tag.strip_suffix(']')?) {
            return None;
        }
        revision
    } else {
        stem
    };
    super::transfer::is_app_revision(revision).then_some(revision)
}

fn parse_history_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

fn is_server_datetime_tag(value: &str) -> bool {
    value.len() == 19
        && value.bytes().enumerate().all(|(index, byte)| match index {
            4 | 7 | 13 | 16 => byte == b'-',
            10 => byte == b'_',
            _ => byte.is_ascii_digit(),
        })
}

fn cancelled_failure() -> SaveApiFailure {
    SaveApiFailure {
        error: Error::Cancelled,
        retry_after: None,
    }
}

pub(super) fn is_network_pause(failure: &SaveApiFailure) -> bool {
    matches!(failure.error, Error::Auth(_) | Error::Forbidden(_))
}

pub(super) fn requires_sign_in(failure: &SaveApiFailure) -> bool {
    matches!(failure.error, Error::Auth(_))
}

pub(super) fn is_retryable_failure(failure: &SaveApiFailure) -> bool {
    matches!(
        failure.error,
        Error::Transport(_)
            | Error::Http {
                status: 429 | 500..=599,
                ..
            }
    )
}

pub(super) fn failure_message(failure: &SaveApiFailure) -> String {
    failure.error.to_string()
}

pub(super) fn retry_after(failure: &SaveApiFailure) -> Option<&str> {
    failure.retry_after.as_deref()
}
