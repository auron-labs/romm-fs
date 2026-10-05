use super::files::publish_missing_save;
use super::gate::SaveSyncEnablement;
use super::incoming_transfer::{
    owned_revision_from_filename, IncomingInventory, VerifiedRemoteSave,
};
use super::transfer::{is_active, validate_configuration};
use rommfs_core::error::{Error, Result};
use rommfs_core::romm::SaveSyncIdentity;
use rommfs_core::save_sync::{
    resolve_save_target, IncomingSaveRecord, SaveMapping, SaveSyncJournal, SaveSyncScope,
    SnapshotState,
};
use std::sync::atomic::AtomicBool;

#[derive(Clone, Debug, Default)]
pub(super) struct ReconciliationOutcome {
    pub(super) local_hash: Option<String>,
    pub(super) remote_id: Option<String>,
    pub(super) remote_hash: Option<String>,
    pub(super) incoming: Vec<IncomingSaveRecord>,
    pub(super) message: Option<String>,
    pub(super) conflict: bool,
    pub(super) installed: bool,
}

pub(super) fn reconcile_inventory(
    journal: &mut SaveSyncJournal,
    mapping: &SaveMapping,
    identity: &SaveSyncIdentity,
    scope: &SaveSyncScope,
    inventory: IncomingInventory,
    enabled: &SaveSyncEnablement,
    active: &AtomicBool,
) -> Result<ReconciliationOutcome> {
    validate_configuration(scope, std::slice::from_ref(mapping), identity)?;
    let observation = journal.observe_local_save_for_reconciliation(mapping)?;
    let slot = journal.slot(&mapping.rom_key)?.ok_or_else(|| {
        Error::Unsupported("save mapping disappeared during reconciliation".into())
    })?;
    acknowledge_uncertain_snapshots(journal, mapping, &inventory.saves)?;
    let local_hash = observation.content_hash;

    if inventory.unsupported_count > 0 {
        let message = format!(
            "RomM returned {} save record(s) outside the verified owned revision format; no remote file was adopted",
            inventory.unsupported_count
        );
        journal.mark_reconciliation_attention(&mapping.rom_key, &message)?;
        let incoming = stage_all(
            journal,
            mapping,
            &inventory.saves,
            "ambiguous_remote_history",
            slot.local_baseline_hash.as_deref(),
            slot.remote_slot_id.as_deref(),
            slot.remote_baseline_hash.as_deref(),
        )?;
        return Ok(ReconciliationOutcome {
            local_hash,
            incoming,
            message: Some(message),
            conflict: true,
            ..Default::default()
        });
    }

    let Some(remote) = unique_latest(&inventory.saves) else {
        if inventory.saves.is_empty() {
            return reconcile_missing_remote(journal, mapping, &slot, local_hash);
        }
        let message =
            "RomM save history has a tie for the latest compatible revision; review required";
        journal.mark_reconciliation_attention(&mapping.rom_key, message)?;
        let incoming = stage_all(
            journal,
            mapping,
            &inventory.saves,
            "ambiguous_remote_history",
            slot.local_baseline_hash.as_deref(),
            slot.remote_slot_id.as_deref(),
            slot.remote_baseline_hash.as_deref(),
        )?;
        return Ok(ReconciliationOutcome {
            local_hash,
            incoming,
            message: Some(message.into()),
            conflict: true,
            ..Default::default()
        });
    };

    let remote_id = remote.metadata.id.to_string();
    let remote_history_ids = inventory
        .saves
        .iter()
        .map(|save| save.metadata.id.to_string())
        .collect::<Vec<_>>();
    if local_hash.as_deref() == Some(remote.content_hash.as_str()) {
        journal.record_reconciled_baseline(
            &mapping.rom_key,
            local_hash.as_deref(),
            Some(&remote_id),
            Some(&remote.content_hash),
            &remote_history_ids,
        )?;
        journal.mark_matching_snapshots_satisfied(
            &mapping.rom_key,
            &remote.content_hash,
            &remote_id,
        )?;
        return Ok(ReconciliationOutcome {
            local_hash,
            remote_id: Some(remote_id),
            remote_hash: Some(remote.content_hash.clone()),
            ..Default::default()
        });
    }

    if local_hash.is_none() && !slot.local_ever_existed && slot.local_baseline_hash.is_none() {
        let staged = journal.stage_incoming(
            mapping,
            &remote_id,
            &remote.bytes,
            "first_remote_save",
            None,
            slot.remote_slot_id.as_deref(),
            slot.remote_baseline_hash.as_deref(),
        )?;
        if !is_active(enabled, active) {
            journal.mark_reconciliation_attention(
                &mapping.rom_key,
                "save sync was disabled before incoming publication",
            )?;
            return Ok(ReconciliationOutcome {
                local_hash,
                remote_id: Some(remote_id),
                remote_hash: Some(remote.content_hash.clone()),
                incoming: vec![staged],
                message: Some("save sync was disabled before incoming publication".into()),
                conflict: true,
                ..Default::default()
            });
        }
        validate_configuration(scope, std::slice::from_ref(mapping), identity)?;
        let check = journal.observe_local_save_for_reconciliation(mapping)?;
        let after_check = journal.slot(&mapping.rom_key)?.ok_or_else(|| {
            Error::Unsupported("save mapping disappeared before publication".into())
        })?;
        if check.present || after_check.local_ever_existed || after_check.local_removed {
            let message = "a local save appeared while the remote download was in flight; both copies were preserved";
            journal.mark_reconciliation_attention(&mapping.rom_key, message)?;
            return Ok(ReconciliationOutcome {
                local_hash: check.content_hash,
                remote_id: Some(remote_id),
                remote_hash: Some(remote.content_hash.clone()),
                incoming: vec![staged],
                message: Some(message.into()),
                conflict: true,
                ..Default::default()
            });
        }
        let target = resolve_save_target(&scope.effective_saves_root, &mapping.relative_path)?;
        let intent = journal.begin_incoming_publication(&staged.id, mapping)?;
        let publication = (|| {
            if !is_active(enabled, active) {
                return Err(Error::Cancelled);
            }
            validate_configuration(scope, std::slice::from_ref(mapping), identity)?;
            publish_missing_save(
                &intent.path,
                &target,
                &intent.content_hash,
                &scope.effective_saves_root,
                || {
                    if !is_active(enabled, active) {
                        return Err(Error::Cancelled);
                    }
                    validate_configuration(scope, std::slice::from_ref(mapping), identity)
                },
            )?;
            journal.finish_incoming_publication(&intent.id, mapping, &remote_history_ids)
        })();
        if let Err(error) = publication {
            let reason =
                format!("incoming save was kept pending; local file was not replaced: {error}");
            journal.cancel_incoming_publication(&intent.id, &reason)?;
            let local = journal.observe_local_save_for_reconciliation(mapping).ok();
            return Ok(ReconciliationOutcome {
                local_hash: local.and_then(|local| local.content_hash),
                remote_id: Some(remote_id),
                remote_hash: Some(remote.content_hash.clone()),
                incoming: vec![journal
                    .incoming_saves()?
                    .into_iter()
                    .find(|record| record.id == intent.id)
                    .unwrap_or(intent)],
                message: Some(reason),
                conflict: true,
                ..Default::default()
            });
        }
        return Ok(ReconciliationOutcome {
            local_hash: Some(remote.content_hash.clone()),
            remote_id: Some(remote_id),
            remote_hash: Some(remote.content_hash.clone()),
            incoming: Vec::new(),
            installed: true,
            ..Default::default()
        });
    }

    let remote_changed = slot.remote_baseline_hash.as_deref() != Some(&remote.content_hash);
    let local_changed = slot.local_baseline_hash.as_deref() != local_hash.as_deref();
    if local_hash.is_some() && local_changed && !remote_changed {
        // Only the live RetroBat file moved from baseline. Keep the accepted
        // remote version and let the normal durable snapshot/upload path make
        // a new app-owned revision.
        journal.record_reconciled_baseline(
            &mapping.rom_key,
            slot.local_baseline_hash.as_deref(),
            Some(&remote_id),
            Some(&remote.content_hash),
            &remote_history_ids,
        )?;
        return Ok(ReconciliationOutcome {
            local_hash,
            remote_id: Some(remote_id),
            remote_hash: Some(remote.content_hash.clone()),
            ..Default::default()
        });
    }
    let message = if local_hash.is_none() && (slot.local_ever_existed || slot.local_removed) {
        Some(
            "the tracked local save was removed; the remote copy was preserved and not reinstalled",
        )
    } else if slot.local_baseline_hash.is_none() || slot.remote_baseline_hash.is_none() {
        Some("first sync found different local and remote saves; both copies were kept")
    } else if !local_changed && remote_changed {
        Some("RomM has a newer save than the local baseline; the local file was kept and incoming bytes were staged")
    } else if local_changed && remote_changed {
        Some("local and remote saves both changed from their baseline; both copies were kept as a conflict")
    } else {
        Some("remote and local saves diverged; both copies were kept")
    };
    let reason = if local_hash.is_none() {
        "local_save_removed"
    } else if slot.local_baseline_hash.is_none() || slot.remote_baseline_hash.is_none() {
        "first_sync_conflict"
    } else if !local_changed && remote_changed {
        "remote_changed"
    } else {
        "two_sided_conflict"
    };
    let staged = journal.stage_incoming(
        mapping,
        &remote_id,
        &remote.bytes,
        reason,
        slot.local_baseline_hash.as_deref(),
        slot.remote_slot_id.as_deref(),
        slot.remote_baseline_hash.as_deref(),
    )?;
    let message = message.unwrap_or("incoming save requires review");
    journal.mark_reconciliation_attention(&mapping.rom_key, message)?;
    Ok(ReconciliationOutcome {
        local_hash,
        remote_id: Some(remote_id),
        remote_hash: Some(remote.content_hash.clone()),
        incoming: vec![staged],
        message: Some(message.into()),
        conflict: true,
        ..Default::default()
    })
}

/// Verify only already-submitted UUID revisions from authenticated, byte-checked
/// inventory. This advances remote progress without authorizing a new POST or
/// changing the local conflict winner.
fn acknowledge_uncertain_snapshots(
    journal: &mut SaveSyncJournal,
    mapping: &SaveMapping,
    saves: &[VerifiedRemoteSave],
) -> Result<()> {
    let uncertain = journal
        .snapshots()?
        .into_iter()
        .filter(|snapshot| {
            snapshot.rom_key == mapping.rom_key
                && matches!(
                    snapshot.state,
                    SnapshotState::RemoteInFlight | SnapshotState::RemoteAmbiguous
                )
        })
        .collect::<Vec<_>>();

    for snapshot in uncertain {
        let mut matches = saves.iter().filter(|save| {
            owned_revision_from_filename(&save.metadata.file_name)
                == Some(snapshot.revision.as_str())
        });
        let Some(remote) = matches.next() else {
            continue;
        };
        if matches.next().is_some() || remote.content_hash != snapshot.content_hash {
            continue;
        }
        journal.record_remote_complete(
            &snapshot.revision,
            &remote.metadata.id.to_string(),
            &snapshot.content_hash,
        )?;
    }
    Ok(())
}

fn reconcile_missing_remote(
    journal: &mut SaveSyncJournal,
    mapping: &SaveMapping,
    slot: &rommfs_core::save_sync::JournalSlot,
    local_hash: Option<String>,
) -> Result<ReconciliationOutcome> {
    if slot.remote_slot_id.is_some() || slot.remote_baseline_hash.is_some() {
        let message = "the previously accepted remote save is missing; local bytes were preserved and no remote delete was requested";
        journal.mark_reconciliation_attention(&mapping.rom_key, message)?;
        return Ok(ReconciliationOutcome {
            local_hash,
            message: Some(message.into()),
            conflict: true,
            ..Default::default()
        });
    }
    if local_hash.is_some() {
        // Initial local-only save: the durable local revision remains queued
        // for upload, while this empty inventory becomes its remote baseline.
        journal.record_reconciled_baseline(
            &mapping.rom_key,
            local_hash.as_deref(),
            None,
            None,
            &[],
        )?;
    } else if !slot.local_ever_existed {
        journal.record_reconciled_baseline(&mapping.rom_key, None, None, None, &[])?;
    } else {
        let message =
            "the tracked local save was removed; no remote delete or local restore was attempted";
        journal.mark_reconciliation_attention(&mapping.rom_key, message)?;
        return Ok(ReconciliationOutcome {
            local_hash,
            message: Some(message.into()),
            conflict: true,
            ..Default::default()
        });
    }
    Ok(ReconciliationOutcome {
        local_hash,
        ..Default::default()
    })
}

fn stage_all(
    journal: &mut SaveSyncJournal,
    mapping: &SaveMapping,
    saves: &[VerifiedRemoteSave],
    reason: &str,
    baseline_local_hash: Option<&str>,
    baseline_remote_id: Option<&str>,
    baseline_remote_hash: Option<&str>,
) -> Result<Vec<IncomingSaveRecord>> {
    saves
        .iter()
        .map(|save| {
            journal.stage_incoming(
                mapping,
                &save.metadata.id.to_string(),
                &save.bytes,
                reason,
                baseline_local_hash,
                baseline_remote_id,
                baseline_remote_hash,
            )
        })
        .collect()
}

fn unique_latest(saves: &[VerifiedRemoteSave]) -> Option<&VerifiedRemoteSave> {
    let latest_time = saves.iter().map(|save| &save.history_time).max()?;
    let mut latest = saves
        .iter()
        .filter(|save| &save.history_time == latest_time);
    let winner = latest.next()?;
    latest.next().is_none().then_some(winner)
}
