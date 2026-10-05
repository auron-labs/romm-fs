use super::gate::SaveSyncEnablement;
pub(super) use super::incoming_transfer::IncomingJob;
use super::incoming_transfer::{self, IncomingContentCache, IncomingInventory};
use rommfs_core::error::{Error, Result};
use rommfs_core::romm::{RemoteSave, RommClient, SaveApiFailure, SaveSyncIdentity};
use rommfs_core::save_sync::{
    resolve_retrobat_gb_profile, resolve_save_target, sha256_content_hash,
    validate_relative_save_path, SaveMapping, SaveSyncScope, SnapshotRecord, SnapshotState,
    MAX_SAVE_BYTES, RETROBAT_GB_SRM_PROFILE,
};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

pub(super) const ACTOR_POLL_INTERVAL: Duration = Duration::from_millis(200);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(15 * 60);

pub(super) enum TransferUpdate {
    Retry {
        revision: String,
        state: SnapshotState,
        message: String,
    },
    Verified {
        revision: String,
        remote_id: String,
        content_hash: String,
    },
    Attention {
        revision: String,
        message: String,
    },
    Paused {
        revision: String,
        state: SnapshotState,
        message: String,
        authentication_required: bool,
        network_paused: bool,
    },
    Reconciled {
        key: rommfs_core::catalog::RomKey,
        result: std::result::Result<IncomingInventory, SaveApiFailure>,
    },
    ReconciliationRetry {
        key: rommfs_core::catalog::RomKey,
        message: String,
    },
}

pub(super) struct TransferJob {
    pub(super) snapshot: SnapshotRecord,
    pub(super) mapping: SaveMapping,
    pub(super) identity: SaveSyncIdentity,
    pub(super) scope: SaveSyncScope,
    pub(super) history: Vec<SnapshotRecord>,
    pub(super) baseline_id: Option<String>,
    pub(super) baseline_hash: Option<String>,
    pub(super) accepted_remote_ids: Vec<String>,
}

pub(super) enum WorkerJob {
    Upload(Box<TransferJob>),
    Reconcile(Box<IncomingJob>),
}

struct RetryingJob {
    job: TransferJob,
    due_at: Instant,
    failures: u32,
    ambiguous: bool,
}

struct RetryingReconciliation {
    job: IncomingJob,
    due_at: Instant,
    failures: u32,
}

struct ReconciliationContext<'a> {
    client: &'a RommClient,
    cache: &'a mut IncomingContentCache,
    enabled: &'a SaveSyncEnablement,
    active: &'a AtomicBool,
    network_paused: &'a AtomicBool,
    sign_in_required: &'a AtomicBool,
    updates: &'a Sender<TransferUpdate>,
}

pub(super) fn run_transfer_worker(
    rx: Receiver<WorkerJob>,
    updates: Sender<TransferUpdate>,
    client: Arc<RommClient>,
    enabled: SaveSyncEnablement,
    active: Arc<AtomicBool>,
) {
    let mut waiting: Vec<RetryingJob> = Vec::new();
    let mut reconciling: Vec<RetryingReconciliation> = Vec::new();
    let mut incoming_cache = IncomingContentCache::default();
    let network_paused = AtomicBool::new(false);
    let sign_in_required = AtomicBool::new(false);
    loop {
        if !active.load(Ordering::SeqCst) || !enabled.is_enabled() {
            break;
        }
        if network_paused.load(Ordering::SeqCst) {
            for job in waiting.drain(..) {
                send_paused_upload(job, sign_in_required.load(Ordering::SeqCst), &updates);
            }
            match rx.recv_timeout(ACTOR_POLL_INTERVAL) {
                Ok(WorkerJob::Upload(job)) => send_paused_upload(
                    RetryingJob {
                        job: *job,
                        due_at: Instant::now(),
                        failures: 0,
                        ambiguous: false,
                    },
                    sign_in_required.load(Ordering::SeqCst),
                    &updates,
                ),
                Ok(WorkerJob::Reconcile(_)) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            continue;
        }
        let now = Instant::now();
        let next_due = waiting
            .iter()
            .map(|job| job.due_at)
            .chain(reconciling.iter().map(|job| job.due_at))
            .min();
        let next = next_due
            .map(|due| due.saturating_duration_since(now).min(ACTOR_POLL_INTERVAL))
            .unwrap_or(ACTOR_POLL_INTERVAL);
        match rx.recv_timeout(next) {
            Ok(WorkerJob::Reconcile(job)) => {
                let mut context = ReconciliationContext {
                    client: &client,
                    cache: &mut incoming_cache,
                    enabled: &enabled,
                    active: &active,
                    network_paused: &network_paused,
                    sign_in_required: &sign_in_required,
                    updates: &updates,
                };
                if let Some(retry) = attempt_reconciliation(*job, &mut context, 0) {
                    reconciling.push(retry);
                }
            }
            Ok(WorkerJob::Upload(job)) => waiting.push(RetryingJob {
                job: *job,
                due_at: Instant::now(),
                failures: 0,
                ambiguous: false,
            }),
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
        if network_paused.load(Ordering::SeqCst) {
            continue;
        }
        let now = Instant::now();
        if let Some(index) = reconciling.iter().position(|job| job.due_at <= now) {
            let retrying = reconciling.swap_remove(index);
            if !active.load(Ordering::SeqCst) || !enabled.is_enabled() {
                break;
            }
            let mut context = ReconciliationContext {
                client: &client,
                cache: &mut incoming_cache,
                enabled: &enabled,
                active: &active,
                network_paused: &network_paused,
                sign_in_required: &sign_in_required,
                updates: &updates,
            };
            if let Some(retry) =
                attempt_reconciliation(retrying.job, &mut context, retrying.failures)
            {
                reconciling.push(retry);
            }
            continue;
        }
        let Some(index) = waiting.iter().position(|job| job.due_at <= now) else {
            continue;
        };
        let mut retrying = waiting.swap_remove(index);
        if !active.load(Ordering::SeqCst) || !enabled.is_enabled() {
            break;
        }
        match upload_attempt(
            &client,
            &retrying.job,
            retrying.ambiguous,
            &enabled,
            &active,
        ) {
            UploadAttempt::Verified { remote_id } => {
                let _ = updates.send(TransferUpdate::Verified {
                    revision: retrying.job.snapshot.revision,
                    remote_id,
                    content_hash: retrying.job.snapshot.content_hash,
                });
            }
            UploadAttempt::Attention(message) => {
                let _ = updates.send(TransferUpdate::Attention {
                    revision: retrying.job.snapshot.revision,
                    message,
                });
            }
            UploadAttempt::Paused(message) => {
                let state = if retrying.ambiguous {
                    SnapshotState::RemoteAmbiguous
                } else {
                    SnapshotState::Ready
                };
                let _ = updates.send(TransferUpdate::Paused {
                    revision: retrying.job.snapshot.revision,
                    state,
                    message,
                    authentication_required: false,
                    network_paused: false,
                });
            }
            UploadAttempt::AuthenticationRequired {
                message,
                sign_in_required: required,
            } => {
                network_paused.store(true, Ordering::SeqCst);
                sign_in_required.store(required, Ordering::SeqCst);
                let state = if retrying.ambiguous {
                    SnapshotState::RemoteAmbiguous
                } else {
                    SnapshotState::Ready
                };
                let _ = updates.send(TransferUpdate::Paused {
                    revision: retrying.job.snapshot.revision,
                    state,
                    message,
                    authentication_required: required,
                    network_paused: true,
                });
            }
            UploadAttempt::Retry {
                message,
                retry_after,
                ambiguous,
            } => {
                retrying.failures = retrying.failures.saturating_add(1);
                retrying.ambiguous |= ambiguous;
                retrying.due_at =
                    Instant::now() + retry_delay(retrying.failures, retry_after.as_deref());
                let state = if retrying.ambiguous {
                    SnapshotState::RemoteAmbiguous
                } else {
                    SnapshotState::RemoteInFlight
                };
                let _ = updates.send(TransferUpdate::Retry {
                    revision: retrying.job.snapshot.revision.clone(),
                    state,
                    message,
                });
                waiting.push(retrying);
            }
        }
    }
}

fn send_paused_upload(
    retrying: RetryingJob,
    authentication_required: bool,
    updates: &Sender<TransferUpdate>,
) {
    let state = if retrying.ambiguous {
        SnapshotState::RemoteAmbiguous
    } else {
        SnapshotState::Ready
    };
    let _ = updates.send(TransferUpdate::Paused {
        revision: retrying.job.snapshot.revision,
        state,
        message:
            "save network requests are paused until save sync is re-enabled or reauthenticated"
                .into(),
        authentication_required,
        network_paused: true,
    });
}

fn attempt_reconciliation(
    job: IncomingJob,
    context: &mut ReconciliationContext<'_>,
    failures: u32,
) -> Option<RetryingReconciliation> {
    let key = job.mapping.rom_key.clone();
    match incoming_transfer::reconcile_attempt(
        context.client,
        &job,
        context.cache,
        context.enabled,
        context.active,
    ) {
        Ok(inventory) => {
            let _ = context.updates.send(TransferUpdate::Reconciled {
                key,
                result: Ok(inventory),
            });
            None
        }
        Err(failure) if incoming_transfer::is_network_pause(&failure) => {
            let required = incoming_transfer::requires_sign_in(&failure);
            context.network_paused.store(true, Ordering::SeqCst);
            context.sign_in_required.store(required, Ordering::SeqCst);
            let _ = context.updates.send(TransferUpdate::Reconciled {
                key,
                result: Err(failure),
            });
            None
        }
        Err(failure) if incoming_transfer::is_retryable_failure(&failure) => {
            let next_failures = failures.saturating_add(1);
            let message = incoming_transfer::failure_message(&failure);
            let due_at = Instant::now()
                + retry_delay(next_failures, incoming_transfer::retry_after(&failure));
            let _ = context
                .updates
                .send(TransferUpdate::ReconciliationRetry { key, message });
            Some(RetryingReconciliation {
                job,
                due_at,
                failures: next_failures,
            })
        }
        Err(failure) => {
            let _ = context.updates.send(TransferUpdate::Reconciled {
                key,
                result: Err(failure),
            });
            None
        }
    }
}

enum UploadAttempt {
    Verified {
        remote_id: String,
    },
    Attention(String),
    Paused(String),
    AuthenticationRequired {
        message: String,
        sign_in_required: bool,
    },
    Retry {
        message: String,
        retry_after: Option<String>,
        ambiguous: bool,
    },
}

fn upload_attempt(
    client: &RommClient,
    job: &TransferJob,
    already_ambiguous: bool,
    enabled: &SaveSyncEnablement,
    active: &AtomicBool,
) -> UploadAttempt {
    if !is_active(enabled, active) {
        return UploadAttempt::Paused("save sync is disabled".into());
    }
    if let Err(error) = validate_configuration(
        &job.scope,
        std::slice::from_ref(&job.mapping),
        &job.identity,
    ) {
        return UploadAttempt::Paused(format!("save-sync scope changed: {error}"));
    }
    let slot_name = job.mapping.profile.id;
    let inventory = match client.save_inventory(job.snapshot.rom_key.rom_id, slot_name) {
        Ok(inventory) => inventory,
        Err(failure) => return api_failure(failure, already_ambiguous, false),
    };
    if !is_active(enabled, active) {
        return UploadAttempt::Paused("save sync was disabled during inventory".into());
    }
    if let Some(problem) = validate_inventory(&inventory, job) {
        return UploadAttempt::Paused(problem);
    }

    let revision_match: Vec<&RemoteSave> = inventory
        .iter()
        .filter(|save| remote_filename_matches_revision(&save.file_name, &job.snapshot.revision))
        .collect();
    if revision_match.len() > 1 {
        return UploadAttempt::Attention(format!(
            "multiple remote save records match revision {}; review required",
            job.snapshot.revision
        ));
    }
    if let Some(remote) = revision_match.first() {
        if let Some(problem) = validate_remote_save(
            remote,
            job,
            job.snapshot.path.metadata().ok().map(|m| m.len()),
        ) {
            return UploadAttempt::Attention(problem);
        }
        match verify_remote_bytes(client, remote, &job.snapshot.content_hash, enabled, active) {
            Ok(true) => return post_upload_inventory(client, remote, job, enabled, active),
            Ok(false) => {
                return UploadAttempt::Attention(format!(
                    "remote revision {} did not match its local SHA-256; review required",
                    job.snapshot.revision
                ))
            }
            Err(failure) => return api_failure(failure, true, false),
        }
    }

    if let Some(problem) = verify_existing_baseline(client, &inventory, job, enabled, active) {
        return problem;
    }
    if !is_active(enabled, active) {
        return UploadAttempt::Paused("save sync was disabled before upload".into());
    }
    if let Err(error) = validate_configuration(
        &job.scope,
        std::slice::from_ref(&job.mapping),
        &job.identity,
    ) {
        return UploadAttempt::Paused(format!("save-sync scope changed before upload: {error}"));
    }
    let bytes = match read_snapshot(&job.snapshot.path, &job.snapshot.content_hash) {
        Ok(bytes) => bytes,
        Err(error) => return UploadAttempt::Paused(error.to_string()),
    };
    if !is_active(enabled, active) {
        return UploadAttempt::Paused("save sync was disabled before upload".into());
    }
    let filename = remote_filename(&job.snapshot.revision);
    let response = match client.upload_save(
        job.snapshot.rom_key.rom_id,
        slot_name,
        "retroarch-gambatte",
        &filename,
        &bytes,
    ) {
        Ok(response) => response,
        Err(failure) => return api_failure(failure, true, true),
    };
    if let Some(problem) = validate_remote_save(&response, job, Some(bytes.len() as u64)) {
        return UploadAttempt::Attention(problem);
    }
    if !is_active(enabled, active) {
        return UploadAttempt::Paused(format!(
            "upload response for revision {} is not yet read-back verified",
            job.snapshot.revision
        ));
    }
    match verify_remote_bytes(
        client,
        &response,
        &job.snapshot.content_hash,
        enabled,
        active,
    ) {
        Ok(true) => post_upload_inventory(client, &response, job, enabled, active),
        Ok(false) => UploadAttempt::Attention(format!(
            "remote read-back for revision {} did not match its local SHA-256",
            job.snapshot.revision
        )),
        Err(failure) => api_failure(failure, true, false),
    }
}

fn validate_inventory(inventory: &[RemoteSave], job: &TransferJob) -> Option<String> {
    for save in inventory {
        if save.rom_id != job.snapshot.rom_key.rom_id
            || save.user_id != job.identity.account_id
            || save.slot.as_deref() != Some(job.mapping.profile.id)
        {
            return Some(
                "RomM save inventory did not match the authenticated account and slot".into(),
            );
        }
    }
    None
}

fn validate_remote_save(
    save: &RemoteSave,
    job: &TransferJob,
    expected_size: Option<u64>,
) -> Option<String> {
    if save.rom_id != job.snapshot.rom_key.rom_id
        || save.user_id != job.identity.account_id
        || save.slot.as_deref() != Some(job.mapping.profile.id)
        || !remote_filename_matches_revision(&save.file_name, &job.snapshot.revision)
        || save.missing_from_fs
        || expected_size.is_some_and(|size| save.file_size_bytes != size)
    {
        return Some(format!(
            "RomM save metadata for revision {} did not match ROM, account, slot, filename, or size",
            job.snapshot.revision
        ));
    }
    None
}

fn verify_existing_baseline(
    client: &RommClient,
    inventory: &[RemoteSave],
    job: &TransferJob,
    enabled: &SaveSyncEnablement,
    active: &AtomicBool,
) -> Option<UploadAttempt> {
    let mut known_ids = HashSet::new();
    for record in &job.history {
        if let Some(id) = &record.remote_slot_id {
            if let Ok(id) = id.parse::<i64>() {
                known_ids.insert(id);
            }
        }
    }
    known_ids.extend(
        job.accepted_remote_ids
            .iter()
            .filter_map(|id| id.parse::<i64>().ok()),
    );
    if let Some(id) = &job.baseline_id {
        if let Ok(id) = id.parse::<i64>() {
            known_ids.insert(id);
        }
    }
    let unexpected = inventory
        .iter()
        .filter(|save| !known_ids.contains(&save.id))
        .collect::<Vec<_>>();
    if !unexpected.is_empty() {
        if unexpected.len() != 1 {
            return Some(UploadAttempt::Attention(format!(
                "multiple untracked remote revisions exist for {}; review the history",
                job.mapping.visible_rom_name
            )));
        }
        let remote = unexpected[0];
        let expected_size = match fs::metadata(&job.snapshot.path) {
            Ok(metadata) => metadata.len(),
            Err(error) => {
                return Some(UploadAttempt::Paused(format!(
                    "local snapshot metadata is unavailable: {error}"
                )))
            }
        };
        if incoming_transfer::owned_revision_from_filename(&remote.file_name).is_none()
            || remote.missing_from_fs
            || remote.file_size_bytes != expected_size
        {
            return Some(UploadAttempt::Attention(format!(
                "unexpected remote save exists for {} (local data was kept); review the conflict",
                job.mapping.visible_rom_name
            )));
        }
        return match verify_remote_bytes(
            client,
            remote,
            &job.snapshot.content_hash,
            enabled,
            active,
        ) {
            Ok(true) => Some(UploadAttempt::Verified {
                remote_id: remote.id.to_string(),
            }),
            Ok(false) => Some(UploadAttempt::Attention(format!(
                "unexpected remote save differs from the local snapshot for {}; review the conflict",
                job.mapping.visible_rom_name
            ))),
            Err(failure) => Some(api_failure(failure, false, false)),
        };
    }
    match (&job.baseline_id, &job.baseline_hash) {
        (Some(baseline_id), Some(baseline_hash)) => {
            let Ok(id) = baseline_id.parse::<i64>() else {
                return Some(UploadAttempt::Paused(
                    "stored RomM save ID is invalid".into(),
                ));
            };
            let Some(remote) = inventory.iter().find(|save| save.id == id) else {
                return Some(UploadAttempt::Attention(format!(
                    "the previously accepted remote baseline disappeared for {}; review required",
                    job.mapping.visible_rom_name
                )));
            };
            if remote.missing_from_fs {
                return Some(UploadAttempt::Attention(format!(
                    "the previously accepted remote baseline is missing from RomM storage for {}; review required",
                    job.mapping.visible_rom_name
                )));
            }
            if !is_active(enabled, active) {
                return Some(UploadAttempt::Paused(
                    "save sync was disabled during baseline check".into(),
                ));
            }
            match verify_remote_bytes(client, remote, baseline_hash, enabled, active) {
                Ok(true) => None,
                Ok(false) => Some(UploadAttempt::Attention(format!(
                    "remote baseline bytes changed for {}; local snapshot was preserved",
                    job.mapping.visible_rom_name
                ))),
                Err(failure) => Some(api_failure(failure, false, false)),
            }
        }
        (None, None) if inventory.is_empty() => None,
        (None, None) => Some(UploadAttempt::Attention(format!(
            "a remote save already exists for {}; initial sync requires review",
            job.mapping.visible_rom_name
        ))),
        _ => Some(UploadAttempt::Paused(
            "stored remote save baseline is incomplete".into(),
        )),
    }
}

fn post_upload_inventory(
    client: &RommClient,
    uploaded: &RemoteSave,
    job: &TransferJob,
    enabled: &SaveSyncEnablement,
    active: &AtomicBool,
) -> UploadAttempt {
    if !is_active(enabled, active) {
        return UploadAttempt::Paused("save sync was disabled before post-upload inventory".into());
    }
    let inventory = match client.save_inventory(job.snapshot.rom_key.rom_id, job.mapping.profile.id)
    {
        Ok(inventory) => inventory,
        Err(failure) => return api_failure(failure, true, false),
    };
    if let Some(problem) = validate_inventory(&inventory, job) {
        return UploadAttempt::Paused(problem);
    }
    let mut allowed: HashSet<i64> = job
        .history
        .iter()
        .filter_map(|record| record.remote_slot_id.as_deref()?.parse().ok())
        .collect();
    allowed.extend(
        job.accepted_remote_ids
            .iter()
            .filter_map(|id| id.parse::<i64>().ok()),
    );
    if let Some(id) = &job.baseline_id {
        if let Ok(id) = id.parse::<i64>() {
            allowed.insert(id);
        }
    }
    allowed.insert(uploaded.id);
    if inventory.iter().any(|save| !allowed.contains(&save.id)) {
        return UploadAttempt::Attention(format!(
            "a competing remote save appeared while uploading {}; local snapshot was preserved",
            job.mapping.visible_rom_name
        ));
    }
    let Some(verified) = inventory.iter().find(|save| save.id == uploaded.id) else {
        return UploadAttempt::Paused(
            "uploaded save was absent from the verification inventory".into(),
        );
    };
    if let Some(problem) = validate_remote_save(verified, job, Some(uploaded.file_size_bytes)) {
        return UploadAttempt::Attention(problem);
    }
    UploadAttempt::Verified {
        remote_id: uploaded.id.to_string(),
    }
}

fn verify_remote_bytes(
    client: &RommClient,
    remote: &RemoteSave,
    expected_hash: &str,
    enabled: &SaveSyncEnablement,
    active: &AtomicBool,
) -> std::result::Result<bool, SaveApiFailure> {
    if !is_active(enabled, active) {
        return Err(SaveApiFailure {
            error: Error::Cancelled,
            retry_after: None,
        });
    }
    let bytes = client.download_save_content(remote.id)?;
    Ok(sha256_content_hash(&bytes) == expected_hash)
}

fn read_snapshot(path: &Path, expected_hash: &str) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_SAVE_BYTES
    {
        return Err(Error::Unsupported(
            "private save snapshot is not a valid save file".into(),
        ));
    }
    let bytes = fs::read(path)?;
    if sha256_content_hash(&bytes) != expected_hash {
        return Err(Error::Unsupported(
            "private snapshot failed SHA-256 verification".into(),
        ));
    }
    Ok(bytes)
}

pub(super) fn remote_filename(revision: &str) -> String {
    format!("rommfs-{revision}.srm")
}

pub(super) fn remote_filename_matches_revision(filename: &str, expected_revision: &str) -> bool {
    if !is_app_revision(expected_revision) {
        return false;
    }
    let Some(stem) = filename
        .strip_prefix("rommfs-")
        .and_then(|name| name.strip_suffix(".srm"))
    else {
        return false;
    };
    let revision = if let Some((revision, tag)) = stem.split_once(" [") {
        let Some(tag) = tag.strip_suffix(']') else {
            return false;
        };
        if !is_server_datetime_tag(tag) {
            return false;
        }
        revision
    } else {
        stem
    };
    revision == expected_revision
}

pub(super) fn is_app_revision(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.len() == 36
        && bytes.get(14) == Some(&b'4')
        && bytes
            .get(19)
            .is_some_and(|byte| matches!(byte, b'8' | b'9' | b'a' | b'b'))
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

fn is_server_datetime_tag(value: &str) -> bool {
    value.len() == 19
        && value.bytes().enumerate().all(|(index, byte)| match index {
            4 | 7 | 13 | 16 => byte == b'-',
            10 => byte == b'_',
            _ => byte.is_ascii_digit(),
        })
}

pub(super) fn validate_configuration(
    scope: &SaveSyncScope,
    mappings: &[SaveMapping],
    identity: &SaveSyncIdentity,
) -> Result<()> {
    if !identity.can_read_saves() || !identity.can_write_saves() {
        return Err(Error::Unsupported(
            "save-sync account lacks assets.read or assets.write".into(),
        ));
    }
    validate_account_and_mappings(scope, mappings, identity.account_id)?;
    Ok(())
}

/// Exporting an already-staged incoming record is local-only and does not need
/// the save API's read/write permissions. It still requires the currently
/// authenticated account and unchanged RetroBat profile/mappings.
pub(super) fn validate_export_configuration(
    scope: &SaveSyncScope,
    mappings: &[SaveMapping],
    identity: &SaveSyncIdentity,
) -> Result<()> {
    validate_account_and_mappings(scope, mappings, identity.account_id)
}

fn validate_account_and_mappings(
    scope: &SaveSyncScope,
    mappings: &[SaveMapping],
    verified_account_id: i64,
) -> Result<()> {
    let account_id = scope.account_id.parse::<i64>().map_err(|_| {
        Error::Unsupported("save-sync account scope is not a verified numeric account ID".into())
    })?;
    if verified_account_id != account_id || scope.server_id.trim().is_empty() {
        return Err(Error::Unsupported(
            "save-sync account or server no longer matches the scoped journal".into(),
        ));
    }
    let visible_names = mappings
        .iter()
        .map(|mapping| mapping.visible_rom_name.clone())
        .collect::<Vec<_>>();
    let profile = resolve_retrobat_gb_profile(&scope.installation_root, &visible_names)?;
    if profile.effective_saves_root != scope.effective_saves_root {
        return Err(Error::Unsupported(
            "RetroBat effective saves root changed after consent".into(),
        ));
    }

    let mut rom_ids = HashSet::new();
    let mut targets = HashSet::new();
    for mapping in mappings {
        if mapping.rom_key.server_id != scope.server_id
            || !rom_ids.insert(mapping.rom_key.rom_id)
            || mapping.profile.id != RETROBAT_GB_SRM_PROFILE
            || mapping.profile.system_dir != "gb"
            || mapping.profile.rom_extension != ".gb"
            || mapping.profile.save_extension != ".srm"
        {
            return Err(Error::Unsupported(
                "save mapping no longer matches the selected server and Game Boy profile".into(),
            ));
        }
        let Some((stem, extension)) = mapping.visible_rom_name.rsplit_once('.') else {
            return Err(Error::Unsupported(
                "mapped Game Boy ROM name is invalid".into(),
            ));
        };
        if stem.is_empty() || !extension.eq_ignore_ascii_case("gb") {
            return Err(Error::Unsupported(
                "mapped Game Boy ROM name is invalid".into(),
            ));
        }
        let relative_path = Path::new("gb").join(format!("{stem}.srm"));
        validate_relative_save_path(&relative_path).map_err(Error::Unsupported)?;
        if mapping.relative_path != relative_path
            || mapping.target_path != scope.effective_saves_root.join(&relative_path)
        {
            return Err(Error::Unsupported(
                "save mapping target changed after consent".into(),
            ));
        }
        resolve_save_target(&scope.effective_saves_root, &relative_path)?;
        let target_identity = mapping
            .target_path
            .to_string_lossy()
            .replace('/', "\\")
            .to_lowercase();
        if !targets.insert(target_identity) {
            return Err(Error::Unsupported(
                "multiple ROM mappings resolve to the same save target".into(),
            ));
        }
        match fs::symlink_metadata(mapping.target_path.with_extension("rtc")) {
            Ok(_) => {
                return Err(Error::Unsupported(
                    "Gambatte RTC sidecars are not supported by the SRAM-only save profile".into(),
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn api_failure(failure: SaveApiFailure, ambiguous: bool, post_attempt: bool) -> UploadAttempt {
    match failure.error {
        Error::Auth(_) => UploadAttempt::AuthenticationRequired {
            message: failure.error.to_string(),
            sign_in_required: true,
        },
        Error::Forbidden(_) => UploadAttempt::AuthenticationRequired {
            message: failure.error.to_string(),
            sign_in_required: false,
        },
        Error::Cancelled => UploadAttempt::Paused(failure.error.to_string()),
        Error::Transport(_) => UploadAttempt::Retry {
            message: failure.error.to_string(),
            retry_after: failure.retry_after,
            ambiguous: ambiguous || post_attempt,
        },
        Error::Http { status: 409, .. } => UploadAttempt::Attention(format!(
            "RomM refused save upload with HTTP 409; no overwrite was attempted: {}",
            failure.error
        )),
        Error::Http {
            status: 429 | 500..=599,
            ..
        } => UploadAttempt::Retry {
            message: failure.error.to_string(),
            retry_after: failure.retry_after,
            ambiguous: ambiguous || post_attempt,
        },
        Error::InvalidCatalogue(_) if post_attempt => UploadAttempt::Retry {
            message: failure.error.to_string(),
            retry_after: failure.retry_after,
            ambiguous: true,
        },
        _ => UploadAttempt::Paused(failure.error.to_string()),
    }
}

fn retry_delay(failures: u32, retry_after: Option<&str>) -> Duration {
    if let Some(value) = retry_after {
        if let Ok(seconds) = value.trim().parse::<u64>() {
            return Duration::from_secs(seconds).min(MAX_RETRY_DELAY);
        }
        if let Ok(time) = httpdate::parse_http_date(value) {
            return time
                .duration_since(SystemTime::now())
                .unwrap_or_default()
                .min(MAX_RETRY_DELAY);
        }
    }
    Duration::from_secs(2u64.saturating_pow(failures.min(9))).min(MAX_RETRY_DELAY)
}

pub(super) fn is_active(enabled: &SaveSyncEnablement, active: &AtomicBool) -> bool {
    active.load(Ordering::SeqCst) && enabled.is_enabled()
}
