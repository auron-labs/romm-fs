use super::gate::SaveSyncEnablement;
use chrono::{DateTime, Utc};
use rommfs_core::error::Error;
use rommfs_core::romm::{RemoteSave, RommClient, SaveApiFailure, SaveSyncIdentity};
use rommfs_core::save_sync::{sha256_content_hash, SaveMapping, SaveSyncScope, MAX_SAVE_BYTES};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;

pub(super) struct IncomingJob {
    pub(super) mapping: SaveMapping,
    pub(super) identity: SaveSyncIdentity,
    pub(super) scope: SaveSyncScope,
}

#[derive(Clone, Debug)]
pub(super) struct VerifiedRemoteSave {
    pub(super) metadata: RemoteSave,
    pub(super) content_hash: String,
    pub(super) bytes: Vec<u8>,
    pub(super) history_time: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub(super) struct IncomingInventory {
    pub(super) saves: Vec<VerifiedRemoteSave>,
    pub(super) unsupported_count: usize,
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
    let inventory = client.save_inventory(job.mapping.rom_key.rom_id, job.mapping.profile.id)?;
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
            || key.slot != job.mapping.profile.id
            || present_ids.contains(&key.save_id)
    });
    let mut saves = Vec::new();
    let mut unsupported_count = 0;
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
        if owned_revision_from_filename(&metadata.file_name).is_none()
            || metadata.missing_from_fs
            || metadata.file_size_bytes == 0
            || metadata.file_size_bytes > MAX_SAVE_BYTES
        {
            unsupported_count += 1;
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
            .filter(|cached| cached.metadata == metadata)
            .cloned();
        let (bytes, content_hash) = if let Some(cached) = cached {
            (cached.bytes, cached.content_hash)
        } else {
            if !super::transfer::is_active(enabled, active) {
                return Err(cancelled_failure());
            }
            let bytes = client.download_save_content(metadata.id)?;
            if bytes.len() as u64 != metadata.file_size_bytes {
                return Err(SaveApiFailure {
                    error: Error::InvalidCatalogue(format!(
                        "RomM save {} content size did not match inventory",
                        metadata.id
                    )),
                    retry_after: None,
                });
            }
            let content_hash = sha256_content_hash(&bytes);
            cache.saves.insert(
                key.clone(),
                CachedRemoteSave {
                    metadata: metadata.clone(),
                    bytes: bytes.clone(),
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
            (bytes, content_hash)
        };
        saves.push(VerifiedRemoteSave {
            metadata,
            content_hash,
            bytes,
            history_time,
        });
    }
    Ok(IncomingInventory {
        saves,
        unsupported_count,
    })
}

fn validate_inventory_identity(inventory: &[RemoteSave], job: &IncomingJob) -> Option<String> {
    inventory
        .iter()
        .any(|save| {
            save.rom_id != job.mapping.rom_key.rom_id
                || save.user_id != job.identity.account_id
                || save.slot.as_deref() != Some(job.mapping.profile.id)
        })
        .then(|| {
            "RomM save inventory did not match the authenticated account, ROM, and slot".into()
        })
}

pub(super) fn owned_revision_from_filename(filename: &str) -> Option<&str> {
    let stem = filename.strip_prefix("rommfs-")?.strip_suffix(".srm")?;
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
