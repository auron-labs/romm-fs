use super::files::export_incoming;
use super::gate::SaveSyncEnablement;
use super::incoming::reconcile_inventory;
use super::transfer::{
    is_active, run_transfer_worker, validate_configuration, validate_export_configuration,
    IncomingJob, TransferJob, TransferUpdate, WorkerJob, ACTOR_POLL_INTERVAL,
};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use rommfs_core::catalog::RomKey;
use rommfs_core::error::{Error, Result};
use rommfs_core::events::{
    AppEvent, EventSink, Level, SaveSyncGameStatus, SaveSyncIncomingStatus, SaveSyncQueueStatus,
};
use rommfs_core::romm::{RommClient, SaveSyncIdentity};
use rommfs_core::save_sync::{
    SaveMapping, SaveSyncJournal, SaveSyncScheduler, SaveSyncScope, SnapshotState,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const FALLBACK_SCAN_INTERVAL: Duration = Duration::from_secs(60);

pub(crate) struct SaveSyncAgent {
    stop: Sender<()>,
    commands: Sender<AgentCommand>,
    actor: Option<JoinHandle<()>>,
    active: Arc<AtomicBool>,
}

enum AgentCommand {
    Export {
        incoming_id: String,
        destination: PathBuf,
        response: Sender<Result<()>>,
    },
}

impl SaveSyncAgent {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start(
        scope: SaveSyncScope,
        mappings: Vec<SaveMapping>,
        identity: SaveSyncIdentity,
        client: Arc<RommClient>,
        journal_path: PathBuf,
        spool_path: PathBuf,
        debounce_secs: u32,
        session_id: u64,
        enabled: impl Into<SaveSyncEnablement>,
        sink: EventSink,
    ) -> Result<Self> {
        let enabled = enabled.into();
        let (stop, stop_rx) = mpsc::channel();
        let (commands, command_rx) = mpsc::channel();
        let active = Arc::new(AtomicBool::new(true));
        let actor_active = Arc::clone(&active);
        let actor_status = Arc::clone(&active);
        let failure_scope = scope.clone();
        let failure_mappings = mappings.clone();
        let failure_journal_path = journal_path.clone();
        let failure_spool_path = spool_path.clone();
        let thread = thread::Builder::new()
            .name("rommfs-save-sync".into())
            .spawn(move || {
                if let Err(error) = run_actor(
                    ActorConfig {
                        scope,
                        mappings,
                        identity,
                        client,
                        journal_path,
                        spool_path,
                        debounce_secs,
                        session_id,
                        enabled,
                        active: actor_active,
                    },
                    stop_rx,
                    command_rx,
                    sink.clone(),
                ) {
                    sink.emit(AppEvent::log(
                        Level::Error,
                        "save-sync",
                        format!("save sync stopped: {error}"),
                    ));
                    sink.emit(AppEvent::SaveSyncQueueUpdated(actor_failure_status(
                        session_id,
                        &failure_scope,
                        &failure_mappings,
                        &failure_journal_path,
                        &failure_spool_path,
                        &error,
                    )));
                }
                actor_status.store(false, Ordering::SeqCst);
            })?;
        Ok(Self {
            stop,
            commands,
            actor: Some(thread),
            active,
        })
    }

    pub(crate) fn stop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
        let _ = self.stop.send(());
        if let Some(actor) = self.actor.take() {
            let _ = actor.join();
        }
    }

    pub(crate) fn export_incoming(&self, incoming_id: &str, destination: PathBuf) -> Result<()> {
        let (response, result) = mpsc::channel();
        self.commands
            .send(AgentCommand::Export {
                incoming_id: incoming_id.to_owned(),
                destination,
                response,
            })
            .map_err(|_| rommfs_core::error::Error::Cancelled)?;
        result
            .recv()
            .map_err(|_| rommfs_core::error::Error::Cancelled)?
    }
}

fn actor_failure_status(
    session_id: u64,
    scope: &SaveSyncScope,
    mappings: &[SaveMapping],
    journal_path: &Path,
    spool_path: &Path,
    error: &Error,
) -> SaveSyncQueueStatus {
    let mut status = SaveSyncQueueStatus {
        session_id,
        mapped_games: mappings.len(),
        reconciled_games: 0,
        pending_outbound: 0,
        pending_incoming: 0,
        attention_games: 0,
        network_paused: false,
        authentication_required: matches!(error, Error::Auth(_)),
        actor_failed: true,
        failure: Some(error.to_string()),
        games: Vec::new(),
        incoming: Vec::new(),
    };
    let Ok(journal) = SaveSyncJournal::open(journal_path, spool_path, scope.clone()) else {
        status.games = mappings
            .iter()
            .map(|mapping| SaveSyncGameStatus {
                rom_id: mapping.rom_key.rom_id,
                rom_name: mapping.visible_rom_name.clone(),
                local_hash: None,
                remote_id: None,
                remote_hash: None,
                incoming_ids: Vec::new(),
                issue: status.failure.clone(),
                installed_incoming: false,
            })
            .collect();
        return status;
    };
    let snapshots = journal.snapshots().unwrap_or_default();
    let incoming = journal.incoming_saves().unwrap_or_default();
    let dirty_mappings = journal.dirty_mappings().unwrap_or_default();
    status.pending_outbound = snapshots
        .iter()
        .filter(|snapshot| snapshot.state != SnapshotState::RemoteComplete)
        .count()
        + dirty_mappings.len();
    status.pending_incoming = incoming.len();
    status.incoming = incoming
        .iter()
        .map(|record| SaveSyncIncomingStatus {
            incoming_id: record.id.clone(),
            rom_id: record.rom_key.rom_id,
            rom_name: mappings
                .iter()
                .find(|mapping| mapping.rom_key == record.rom_key)
                .map(|mapping| mapping.visible_rom_name.clone())
                .unwrap_or_else(|| format!("ROM {}", record.rom_key.rom_id)),
            remote_id: record.remote_id.clone(),
            content_hash: record.content_hash.clone(),
            reason: record.reason.clone(),
            state: record.state.clone(),
        })
        .collect();
    for mapping in mappings {
        let slot = journal.slot(&mapping.rom_key).ok().flatten();
        if slot.as_ref().is_some_and(|slot| slot.needs_attention) {
            status.attention_games += 1;
        }
        let mut game = SaveSyncGameStatus {
            rom_id: mapping.rom_key.rom_id,
            rom_name: mapping.visible_rom_name.clone(),
            local_hash: slot
                .as_ref()
                .and_then(|slot| slot.current_local_hash.clone()),
            remote_id: slot.as_ref().and_then(|slot| slot.remote_slot_id.clone()),
            remote_hash: slot
                .as_ref()
                .and_then(|slot| slot.remote_baseline_hash.clone()),
            incoming_ids: status
                .incoming
                .iter()
                .filter(|record| record.rom_id == mapping.rom_key.rom_id)
                .map(|record| record.incoming_id.clone())
                .collect(),
            issue: slot.as_ref().and_then(|slot| slot.last_failure.clone()),
            installed_incoming: false,
        };
        if game.issue.is_none() {
            game.issue = status.failure.clone();
        }
        status.games.push(game);
    }
    status
}

impl Drop for SaveSyncAgent {
    fn drop(&mut self) {
        self.stop();
    }
}

struct ActorConfig {
    scope: SaveSyncScope,
    mappings: Vec<SaveMapping>,
    identity: SaveSyncIdentity,
    client: Arc<RommClient>,
    journal_path: PathBuf,
    spool_path: PathBuf,
    debounce_secs: u32,
    session_id: u64,
    enabled: SaveSyncEnablement,
    active: Arc<AtomicBool>,
}

struct ActorContext<'a> {
    session_id: u64,
    scope: &'a SaveSyncScope,
    mappings: &'a [SaveMapping],
    identity: &'a SaveSyncIdentity,
    enabled: &'a SaveSyncEnablement,
    active: &'a AtomicBool,
}

struct ActorState {
    dispatched: HashMap<String, RomKey>,
    paused_until: HashMap<RomKey, Instant>,
    reconciled_keys: HashSet<RomKey>,
    inventory_dispatched: HashSet<RomKey>,
    game_states: HashMap<i64, SaveSyncGameStatus>,
    transfer_failures: HashMap<String, (RomKey, String)>,
    capture_failures: HashMap<RomKey, String>,
    reconciliation_failures: HashMap<RomKey, String>,
    attention_keys: HashSet<RomKey>,
    network_paused: bool,
    authentication_required: bool,
    next_inventory_poll: Instant,
}

impl ActorState {
    fn new(next_inventory_poll: Instant) -> Self {
        Self {
            dispatched: HashMap::new(),
            paused_until: HashMap::new(),
            reconciled_keys: HashSet::new(),
            inventory_dispatched: HashSet::new(),
            game_states: HashMap::new(),
            transfer_failures: HashMap::new(),
            capture_failures: HashMap::new(),
            reconciliation_failures: HashMap::new(),
            attention_keys: HashSet::new(),
            network_paused: false,
            authentication_required: false,
            next_inventory_poll,
        }
    }
}

fn run_actor(
    config: ActorConfig,
    stop_rx: Receiver<()>,
    command_rx: Receiver<AgentCommand>,
    sink: EventSink,
) -> Result<()> {
    let ActorConfig {
        scope,
        mappings,
        identity,
        client,
        journal_path,
        spool_path,
        debounce_secs,
        session_id,
        enabled,
        active,
    } = config;
    if !is_active(&enabled, &active) {
        return Ok(());
    }
    validate_configuration(&scope, &mappings, &identity)?;
    let mut journal = SaveSyncJournal::open(journal_path, spool_path, scope.clone())?;
    let mapping_report = rommfs_core::save_sync::MappingReport {
        mappings: mappings.clone(),
        ..Default::default()
    };
    journal.reconcile_mappings(&mapping_report)?;
    let mut active_mappings = Vec::new();
    for mapping in mappings {
        let slot = journal.slot(&mapping.rom_key)?;
        if slot.as_ref().is_some_and(|slot| {
            slot.mapping_confirmed
                && slot.candidate_available
                && slot.relative_path == mapping.relative_path
        }) {
            active_mappings.push(mapping);
        } else if slot.is_some_and(|slot| !slot.mapping_confirmed && !slot.needs_attention) {
            journal.confirm_mapping(&mapping.rom_key, &mapping.relative_path)?;
            active_mappings.push(mapping);
        }
    }
    let mappings = active_mappings;
    let context = ActorContext {
        session_id,
        scope: &scope,
        mappings: &mappings,
        identity: &identity,
        enabled: &enabled,
        active: &active,
    };

    if !is_active(&enabled, &active) {
        return Ok(());
    }
    let (watch_tx, watch_rx) = mpsc::channel();
    let mut watcher = create_watcher(watch_tx, &scope.effective_saves_root, &sink);
    let (transfer_tx, transfer_updates) = mpsc::channel::<WorkerJob>();
    let (result_tx, result_rx) = mpsc::channel();
    let transfer_gate = enabled.clone();
    let transfer_active = Arc::clone(&active);
    let transfer_client = Arc::clone(&client);
    thread::Builder::new()
        .name("rommfs-save-upload".into())
        .spawn(move || {
            run_transfer_worker(
                transfer_updates,
                result_tx,
                transfer_client,
                transfer_gate,
                transfer_active,
            )
        })?;

    let mut scheduler = SaveSyncScheduler::with_seconds(debounce_secs);
    let restored = scheduler.restore_dirty(&mut journal, Instant::now())?;
    for (key, error) in restored.failures {
        sink.emit(AppEvent::log(
            Level::Warn,
            "save-sync",
            format!(
                "could not restore pending save for ROM {}: {error}",
                key.rom_id
            ),
        ));
    }
    let mut state = ActorState::new(Instant::now() + FALLBACK_SCAN_INTERVAL);
    let mut root_watched = false;
    update_root_watch(
        watcher.as_mut(),
        &scope.effective_saves_root,
        &mut root_watched,
        &sink,
    );
    if safe_directory(&scope.effective_saves_root) {
        observe_mappings(
            &mut scheduler,
            &mut journal,
            &context,
            Instant::now(),
            false,
            &sink,
        )?;
    }
    emit_queue_status(&context, &mappings, &journal, &scheduler, &state, &sink)?;
    for mapping in &mappings {
        if transfer_tx
            .send(WorkerJob::Reconcile(Box::new(IncomingJob {
                mapping: mapping.clone(),
                identity: identity.clone(),
                scope: scope.clone(),
            })))
            .is_ok()
        {
            state.inventory_dispatched.insert(mapping.rom_key.clone());
        }
    }
    let mut next_fallback_scan = Instant::now() + FALLBACK_SCAN_INTERVAL;
    state.next_inventory_poll = next_fallback_scan;

    loop {
        if !active.load(Ordering::SeqCst) || !enabled.is_enabled() {
            break;
        }
        if stop_rx.try_recv().is_ok() {
            break;
        }
        for command in command_rx.try_iter() {
            match command {
                AgentCommand::Export {
                    incoming_id,
                    destination,
                    response,
                } => {
                    let result = if is_active(&enabled, &active) {
                        export_staged_incoming(&mut journal, &incoming_id, &destination, &context)
                    } else {
                        Err(rommfs_core::error::Error::Cancelled)
                    };
                    if let Err(error) = &result {
                        sink.emit(AppEvent::log(
                            Level::Error,
                            "save-sync",
                            format!("incoming save export failed: {error}"),
                        ));
                    }
                    let _ = response.send(result);
                }
            }
        }

        for signal in watch_rx.try_iter() {
            let path = match signal {
                WatcherSignal::Path(path) => path,
                WatcherSignal::Rescan => {
                    next_fallback_scan = Instant::now();
                    continue;
                }
                WatcherSignal::Error(message) => {
                    sink.emit(AppEvent::log(
                        Level::Warn,
                        "save-sync",
                        format!("native save watcher failed; scanning remains active: {message}"),
                    ));
                    next_fallback_scan = Instant::now();
                    continue;
                }
            };
            if same_path(&path, &scope.effective_saves_root) {
                update_root_watch(
                    watcher.as_mut(),
                    &scope.effective_saves_root,
                    &mut root_watched,
                    &sink,
                );
                if safe_directory(&scope.effective_saves_root) {
                    observe_mappings(
                        &mut scheduler,
                        &mut journal,
                        &context,
                        Instant::now(),
                        true,
                        &sink,
                    )?;
                }
                continue;
            }
            if let Some(mapping) = mappings
                .iter()
                .find(|mapping| same_path(&path, &mapping.target_path))
            {
                if journal
                    .slot(&mapping.rom_key)?
                    .is_some_and(|slot| slot.needs_attention)
                {
                    if let Err(error) = journal.observe_local_save_for_reconciliation(mapping) {
                        sink.emit(AppEvent::log(
                            Level::Warn,
                            "save-sync",
                            format!("save conflict observation deferred: {error}"),
                        ));
                    }
                } else {
                    scheduler.hint(mapping, Instant::now());
                }
            }
        }

        for update in result_rx.try_iter() {
            match update {
                TransferUpdate::Retry {
                    revision,
                    state: snapshot_state,
                    message,
                } => {
                    journal.record_remote_outcome(
                        &revision,
                        snapshot_state,
                        Some("retry_wait"),
                        Some(&message),
                    )?;
                    if let Some(key) = state.dispatched.get(&revision).cloned() {
                        state
                            .transfer_failures
                            .insert(revision.clone(), (key.clone(), message.clone()));
                        ensure_game_state(&key, &mappings, &mut state);
                        sink.emit(AppEvent::SaveSyncTransferProgress {
                            session_id: context.session_id,
                            rom_id: key.rom_id,
                            revision: revision.clone(),
                            phase: "retry_wait".into(),
                            detail: Some(message.clone()),
                        });
                    }
                    sink.emit(AppEvent::log(Level::Warn, "save-sync", message));
                    emit_queue_status(&context, &mappings, &journal, &scheduler, &state, &sink)?;
                }
                TransferUpdate::Verified {
                    revision,
                    remote_id,
                    content_hash,
                } => {
                    journal.record_remote_complete(&revision, &remote_id, &content_hash)?;
                    let key = state.dispatched.remove(&revision);
                    if let Some(key) = &key {
                        state.paused_until.remove(key);
                        state.attention_keys.remove(key);
                    }
                    state.transfer_failures.remove(&revision);
                    if let Some(key) = key {
                        sink.emit(AppEvent::SaveSyncTransferProgress {
                            session_id: context.session_id,
                            rom_id: key.rom_id,
                            revision: revision.clone(),
                            phase: "verified".into(),
                            detail: None,
                        });
                    }
                    sink.emit(AppEvent::log(
                        Level::Info,
                        "save-sync",
                        format!("verified upload revision {revision}"),
                    ));
                    emit_queue_status(&context, &mappings, &journal, &scheduler, &state, &sink)?;
                }
                TransferUpdate::Attention { revision, message } => {
                    journal.mark_remote_attention(&revision, &message)?;
                    let key = state.dispatched.remove(&revision);
                    if let Some(key) = key {
                        state.attention_keys.insert(key.clone());
                        state
                            .transfer_failures
                            .insert(revision.clone(), (key.clone(), message.clone()));
                        ensure_game_state(&key, &mappings, &mut state);
                        state
                            .paused_until
                            .insert(key.clone(), Instant::now() + FALLBACK_SCAN_INTERVAL);
                        sink.emit(AppEvent::SaveSyncTransferProgress {
                            session_id: context.session_id,
                            rom_id: key.rom_id,
                            revision: revision.clone(),
                            phase: "attention".into(),
                            detail: Some(message.clone()),
                        });
                    }
                    sink.emit(AppEvent::log(Level::Warn, "save-sync", message));
                    emit_queue_status(&context, &mappings, &journal, &scheduler, &state, &sink)?;
                }
                TransferUpdate::Paused {
                    revision,
                    state: snapshot_state,
                    message,
                    authentication_required,
                    network_paused,
                } => {
                    let first_authentication_failure =
                        authentication_required && !state.authentication_required;
                    state.network_paused |= network_paused;
                    state.authentication_required |= authentication_required;
                    if first_authentication_failure {
                        sink.emit(AppEvent::SaveSyncAuthenticationRequired {
                            session_id: context.session_id,
                        });
                    }
                    if let Some(key) = state.dispatched.get(&revision).cloned() {
                        state
                            .transfer_failures
                            .insert(revision.clone(), (key.clone(), message.clone()));
                        ensure_game_state(&key, &mappings, &mut state);
                    }
                    journal.record_remote_outcome(
                        &revision,
                        snapshot_state,
                        Some("paused"),
                        Some(&message),
                    )?;
                    let key = state.dispatched.remove(&revision);
                    if let Some(key) = key {
                        state
                            .paused_until
                            .insert(key.clone(), Instant::now() + FALLBACK_SCAN_INTERVAL);
                        sink.emit(AppEvent::SaveSyncTransferProgress {
                            session_id: context.session_id,
                            rom_id: key.rom_id,
                            revision: revision.clone(),
                            phase: "paused".into(),
                            detail: Some(message.clone()),
                        });
                    }
                    sink.emit(AppEvent::log(Level::Warn, "save-sync", message));
                    emit_queue_status(&context, &mappings, &journal, &scheduler, &state, &sink)?;
                }
                TransferUpdate::Reconciled { key, result } => {
                    handle_reconciled_update(
                        key,
                        result,
                        &context,
                        &mut journal,
                        &mut scheduler,
                        &mut state,
                        &sink,
                    )?;
                    emit_queue_status(&context, &mappings, &journal, &scheduler, &state, &sink)?;
                }
                TransferUpdate::ReconciliationRetry { key, message } => {
                    remember_reconciliation_failure(&key, &message, &mappings, &mut state);
                    log_reconciliation_retry(&key, &message, &mappings, &sink);
                    if let Some(mapping) = mappings.iter().find(|mapping| mapping.rom_key == key) {
                        emit_reconciliation_failure(
                            &key, mapping, &message, &context, &journal, &state, &sink,
                        )?;
                    }
                    emit_queue_status(&context, &mappings, &journal, &scheduler, &state, &sink)?;
                }
            }
        }

        let now = Instant::now();
        if now >= next_fallback_scan {
            if safe_directory(&scope.effective_saves_root) {
                observe_mappings(&mut scheduler, &mut journal, &context, now, true, &sink)?;
            }
            next_fallback_scan = now + FALLBACK_SCAN_INTERVAL;
        }

        if now >= state.next_inventory_poll && !state.network_paused {
            for mapping in &mappings {
                if state.inventory_dispatched.contains(&mapping.rom_key) {
                    continue;
                }
                if transfer_tx
                    .send(WorkerJob::Reconcile(Box::new(IncomingJob {
                        mapping: mapping.clone(),
                        identity: identity.clone(),
                        scope: scope.clone(),
                    })))
                    .is_ok()
                {
                    state.inventory_dispatched.insert(mapping.rom_key.clone());
                }
            }
            state.next_inventory_poll = now + FALLBACK_SCAN_INTERVAL;
        }

        for (key, captured) in scheduler.capture_due(&mut journal, now) {
            match captured {
                Ok(snapshot) => {
                    state.capture_failures.remove(&key);
                    sink.emit(AppEvent::SaveSyncTransferProgress {
                        session_id: context.session_id,
                        rom_id: snapshot.rom_key.rom_id,
                        revision: snapshot.revision.clone(),
                        phase: "captured".into(),
                        detail: None,
                    });
                    sink.emit(AppEvent::log(
                        Level::Info,
                        "save-sync",
                        format!("captured local revision {}", snapshot.revision),
                    ));
                    emit_queue_status(&context, &mappings, &journal, &scheduler, &state, &sink)?;
                }
                Err(error) => {
                    state
                        .capture_failures
                        .insert(key.clone(), error.to_string());
                    ensure_game_state(&key, &mappings, &mut state);
                    sink.emit(AppEvent::log(
                        Level::Warn,
                        "save-sync",
                        format!("save capture deferred: {error}"),
                    ));
                    emit_queue_status(&context, &mappings, &journal, &scheduler, &state, &sink)?;
                }
            }
        }
        for snapshot in journal.ready_snapshots()? {
            if state.network_paused
                || !state.reconciled_keys.contains(&snapshot.rom_key)
                || state.dispatched.contains_key(&snapshot.revision)
                || state
                    .dispatched
                    .values()
                    .any(|key| key == &snapshot.rom_key)
                || state
                    .paused_until
                    .get(&snapshot.rom_key)
                    .is_some_and(|deadline| *deadline > now)
            {
                continue;
            }
            let Some(mapping) = mappings
                .iter()
                .find(|mapping| mapping.rom_key == snapshot.rom_key)
                .cloned()
            else {
                continue;
            };
            let Some(slot) = journal.slot(&snapshot.rom_key)? else {
                continue;
            };
            let history = journal
                .snapshots()?
                .into_iter()
                .filter(|record| {
                    record.rom_key == snapshot.rom_key
                        && record.state == SnapshotState::RemoteComplete
                })
                .collect();
            journal.record_remote_outcome(
                &snapshot.revision,
                SnapshotState::RemoteInFlight,
                Some("request_started"),
                None,
            )?;
            if transfer_tx
                .send(WorkerJob::Upload(Box::new(TransferJob {
                    snapshot: snapshot.clone(),
                    mapping,
                    identity: identity.clone(),
                    scope: scope.clone(),
                    history,
                    baseline_id: slot.remote_slot_id,
                    baseline_hash: slot.remote_baseline_hash,
                    accepted_remote_ids: slot.remote_history_ids,
                })))
                .is_ok()
            {
                state
                    .dispatched
                    .insert(snapshot.revision.clone(), snapshot.rom_key.clone());
                sink.emit(AppEvent::SaveSyncTransferProgress {
                    session_id: context.session_id,
                    rom_id: snapshot.rom_key.rom_id,
                    revision: snapshot.revision.clone(),
                    phase: "uploading".into(),
                    detail: None,
                });
                emit_queue_status(&context, &mappings, &journal, &scheduler, &state, &sink)?;
            }
        }
        match stop_rx.recv_timeout(ACTOR_POLL_INTERVAL) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
    active.store(false, Ordering::SeqCst);
    drop(watcher);
    drop(transfer_tx);
    Ok(())
}

fn handle_reconciled_update(
    key: RomKey,
    result: std::result::Result<
        super::incoming_transfer::IncomingInventory,
        rommfs_core::romm::SaveApiFailure,
    >,
    context: &ActorContext<'_>,
    journal: &mut SaveSyncJournal,
    scheduler: &mut SaveSyncScheduler,
    state: &mut ActorState,
    sink: &EventSink,
) -> Result<()> {
    state.inventory_dispatched.remove(&key);
    if !is_active(context.enabled, context.active) {
        return Ok(());
    }
    let Some(mapping) = context
        .mappings
        .iter()
        .find(|mapping| mapping.rom_key == key)
    else {
        return Ok(());
    };

    match result {
        Ok(inventory) => {
            let outcome = match validate_configuration(
                context.scope,
                std::slice::from_ref(mapping),
                context.identity,
            ) {
                Ok(()) => reconcile_inventory(
                    journal,
                    mapping,
                    context.identity,
                    context.scope,
                    inventory,
                    context.enabled,
                    context.active,
                ),
                Err(error) => Err(error),
            };
            match outcome {
                Ok(outcome) => {
                    state.next_inventory_poll = Instant::now() + FALLBACK_SCAN_INTERVAL;
                    state.reconciliation_failures.remove(&key);
                    state.reconciled_keys.insert(key.clone());
                    state.paused_until.remove(&key);
                    for snapshot in journal.snapshots()?.into_iter().filter(|snapshot| {
                        snapshot.rom_key == key && snapshot.state == SnapshotState::RemoteComplete
                    }) {
                        state.transfer_failures.remove(&snapshot.revision);
                    }
                    if outcome.conflict {
                        state.attention_keys.insert(key.clone());
                        scheduler.cancel(&key);
                    } else {
                        state.attention_keys.remove(&key);
                        let restored =
                            scheduler.restore_dirty_for(journal, &key, Instant::now())?;
                        for (dirty_key, error) in restored.failures {
                            sink.emit(AppEvent::log(
                                Level::Warn,
                                "save-sync",
                                format!(
                                    "could not resume pending save for ROM {}: {error}",
                                    dirty_key.rom_id
                                ),
                            ));
                        }
                    }
                    let game = SaveSyncGameStatus {
                        rom_id: key.rom_id,
                        rom_name: mapping.visible_rom_name.clone(),
                        local_hash: outcome.local_hash,
                        remote_id: outcome.remote_id,
                        remote_hash: outcome.remote_hash,
                        incoming_ids: outcome
                            .incoming
                            .iter()
                            .map(|record| record.id.clone())
                            .collect(),
                        issue: outcome.message.clone(),
                        installed_incoming: outcome.installed,
                    };
                    state.game_states.insert(key.rom_id, game.clone());
                    sink.emit(AppEvent::SaveSyncReconciliation {
                        session_id: context.session_id,
                        game,
                        mapped_games: context.mappings.len(),
                        reconciled_games: state.reconciled_keys.len(),
                        pending_incoming: journal.incoming_saves()?.len(),
                        attention_games: state.attention_keys.len(),
                        failure: queue_failure(state, context.mappings),
                    });
                    sink.emit(AppEvent::log(
                        if outcome.conflict {
                            Level::Warn
                        } else {
                            Level::Info
                        },
                        "save-sync",
                        outcome.message.unwrap_or_else(|| {
                            if outcome.installed {
                                format!(
                                    "installed first remote save for {}",
                                    mapping.visible_rom_name
                                )
                            } else {
                                format!("reconciled saves for {}", mapping.visible_rom_name)
                            }
                        }),
                    ));
                }
                Err(error) => {
                    let message = error.to_string();
                    remember_reconciliation_failure(&key, &message, context.mappings, state);
                    state.next_inventory_poll = Instant::now() + FALLBACK_SCAN_INTERVAL;
                    emit_reconciliation_failure(
                        &key, mapping, &message, context, journal, state, sink,
                    )?;
                    sink.emit(AppEvent::log(
                        Level::Error,
                        "save-sync",
                        format!(
                            "reconciliation failed for {}: {message}",
                            mapping.visible_rom_name
                        ),
                    ));
                }
            }
        }
        Err(failure) => {
            let authentication_required = matches!(&failure.error, Error::Auth(_));
            let permission_denied = matches!(&failure.error, Error::Forbidden(_));
            if authentication_required || permission_denied {
                state.network_paused = true;
            }
            if authentication_required {
                // Only a 401 invalidates the held RomM token. A 403 pauses save
                // traffic without disrupting ROM access.
                state.authentication_required = true;
                sink.emit(AppEvent::SaveSyncAuthenticationRequired {
                    session_id: context.session_id,
                });
            }
            let message = failure.to_string();
            remember_reconciliation_failure(&key, &message, context.mappings, state);
            state.next_inventory_poll = Instant::now() + FALLBACK_SCAN_INTERVAL;
            emit_reconciliation_failure(&key, mapping, &message, context, journal, state, sink)?;
            sink.emit(AppEvent::log(
                Level::Error,
                "save-sync",
                format!(
                    "inventory failed for {}: {message}",
                    mapping.visible_rom_name
                ),
            ));
        }
    }
    Ok(())
}

fn remember_reconciliation_failure(
    key: &RomKey,
    message: &str,
    mappings: &[SaveMapping],
    state: &mut ActorState,
) {
    state
        .reconciliation_failures
        .insert(key.clone(), message.to_owned());
    let Some(mapping) = mappings.iter().find(|mapping| &mapping.rom_key == key) else {
        return;
    };
    let game = state
        .game_states
        .entry(key.rom_id)
        .or_insert_with(|| SaveSyncGameStatus {
            rom_id: key.rom_id,
            rom_name: mapping.visible_rom_name.clone(),
            local_hash: None,
            remote_id: None,
            remote_hash: None,
            incoming_ids: Vec::new(),
            issue: None,
            installed_incoming: false,
        });
    game.issue = Some(message.to_owned());
}

fn ensure_game_state(key: &RomKey, mappings: &[SaveMapping], state: &mut ActorState) {
    let Some(mapping) = mappings.iter().find(|mapping| &mapping.rom_key == key) else {
        return;
    };
    state
        .game_states
        .entry(key.rom_id)
        .or_insert_with(|| SaveSyncGameStatus {
            rom_id: key.rom_id,
            rom_name: mapping.visible_rom_name.clone(),
            local_hash: None,
            remote_id: None,
            remote_hash: None,
            incoming_ids: Vec::new(),
            issue: None,
            installed_incoming: false,
        });
}

fn game_failure<'a>(key: &RomKey, state: &'a ActorState) -> Option<&'a str> {
    let transfer_failure = state
        .transfer_failures
        .iter()
        .filter(|(_, (failed_key, _))| failed_key == key)
        .min_by_key(|(revision, _)| revision.as_str())
        .map(|(_, (_, failure))| failure.as_str());
    transfer_failure
        .or_else(|| state.capture_failures.get(key).map(String::as_str))
        .or_else(|| state.reconciliation_failures.get(key).map(String::as_str))
}

fn queue_failure(state: &ActorState, mappings: &[SaveMapping]) -> Option<String> {
    mappings
        .iter()
        .find_map(|mapping| game_failure(&mapping.rom_key, state).map(str::to_owned))
}

fn emit_reconciliation_failure(
    key: &RomKey,
    mapping: &SaveMapping,
    message: &str,
    context: &ActorContext<'_>,
    journal: &SaveSyncJournal,
    state: &ActorState,
    sink: &EventSink,
) -> Result<()> {
    let mut game = state
        .game_states
        .get(&key.rom_id)
        .cloned()
        .unwrap_or(SaveSyncGameStatus {
            rom_id: key.rom_id,
            rom_name: mapping.visible_rom_name.clone(),
            local_hash: None,
            remote_id: None,
            remote_hash: None,
            incoming_ids: Vec::new(),
            issue: None,
            installed_incoming: false,
        });
    game.issue = Some(message.to_owned());
    sink.emit(AppEvent::SaveSyncReconciliation {
        session_id: context.session_id,
        game,
        mapped_games: context.mappings.len(),
        reconciled_games: state.reconciled_keys.len(),
        pending_incoming: journal.incoming_saves()?.len(),
        attention_games: state.attention_keys.len(),
        failure: queue_failure(state, context.mappings),
    });
    Ok(())
}

fn log_reconciliation_retry(
    key: &RomKey,
    message: &str,
    mappings: &[SaveMapping],
    sink: &EventSink,
) {
    if let Some(mapping) = mappings.iter().find(|mapping| &mapping.rom_key == key) {
        sink.emit(AppEvent::log(
            Level::Warn,
            "save-sync",
            format!(
                "incoming reconciliation for {} will retry: {message}",
                mapping.visible_rom_name
            ),
        ));
    }
}

enum WatcherSignal {
    Path(PathBuf),
    Rescan,
    Error(String),
}

fn create_watcher(
    path_tx: Sender<WatcherSignal>,
    root: &Path,
    sink: &EventSink,
) -> Option<RecommendedWatcher> {
    let callback = move |event: notify::Result<notify::Event>| {
        let event = match event {
            Ok(event) => event,
            Err(error) => {
                let _ = path_tx.send(WatcherSignal::Error(error.to_string()));
                return;
            }
        };
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        if matches!(event.kind, EventKind::Any | EventKind::Other) || event.paths.is_empty() {
            let _ = path_tx.send(WatcherSignal::Rescan);
        }
        for path in event.paths {
            let _ = path_tx.send(WatcherSignal::Path(path));
        }
    };
    match notify::recommended_watcher(callback) {
        Ok(mut watcher) => {
            let parent = root.parent().unwrap_or(root);
            if parent.exists() {
                if let Err(error) = watcher.watch(parent, RecursiveMode::NonRecursive) {
                    sink.emit(AppEvent::log(
                        Level::Warn,
                        "save-sync",
                        format!("save root boundary watch failed: {error}"),
                    ));
                }
            }
            Some(watcher)
        }
        Err(error) => {
            sink.emit(AppEvent::log(
                Level::Warn,
                "save-sync",
                format!("native save watcher unavailable; periodic scans remain active: {error}"),
            ));
            None
        }
    }
}

fn update_root_watch(
    watcher: Option<&mut RecommendedWatcher>,
    root: &Path,
    watched: &mut bool,
    sink: &EventSink,
) {
    if *watched || !safe_directory(root) {
        return;
    }
    let Some(watcher) = watcher else { return };
    match watcher.watch(root, RecursiveMode::Recursive) {
        Ok(()) => *watched = true,
        Err(error) => sink.emit(AppEvent::log(
            Level::Warn,
            "save-sync",
            format!("recursive save watch failed; periodic scans remain active: {error}"),
        )),
    }
}

fn safe_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| {
        metadata.is_dir() && !rommfs_core::save_sync::path_is_reparse_point(&metadata)
    })
}

fn observe_mappings(
    scheduler: &mut SaveSyncScheduler,
    journal: &mut SaveSyncJournal,
    context: &ActorContext<'_>,
    now: Instant,
    settle_missing: bool,
    sink: &EventSink,
) -> Result<()> {
    validate_configuration(context.scope, context.mappings, context.identity)?;
    for mapping in context.mappings {
        let observation = (|| {
            let slot = journal.slot(&mapping.rom_key)?;
            if slot.as_ref().is_some_and(|slot| slot.needs_attention) {
                journal.observe_local_save_for_reconciliation(mapping)?;
                return Ok(());
            }
            if settle_missing {
                scheduler.hint(mapping, now);
                Ok(())
            } else {
                scheduler.observe(journal, mapping, now).map(|_| ())
            }
        })();
        if let Err(error) = observation {
            sink.emit(AppEvent::log(
                Level::Warn,
                "save-sync",
                format!("save scan deferred: {error}"),
            ));
        }
    }
    Ok(())
}

fn export_staged_incoming(
    journal: &mut SaveSyncJournal,
    incoming_id: &str,
    destination: &Path,
    context: &ActorContext<'_>,
) -> Result<()> {
    if !is_active(context.enabled, context.active) {
        return Err(rommfs_core::error::Error::Cancelled);
    }
    export_staged_record(
        journal,
        incoming_id,
        destination,
        context.scope,
        context.mappings,
        context.identity,
    )
}

pub(crate) fn export_pending_incoming(
    scope: &SaveSyncScope,
    mappings: &[SaveMapping],
    identity: &SaveSyncIdentity,
    journal_path: &Path,
    spool_path: &Path,
    incoming_id: &str,
    destination: &Path,
) -> Result<()> {
    validate_export_configuration(scope, mappings, identity)?;
    let mut journal = SaveSyncJournal::open(journal_path, spool_path, scope.clone())?;
    export_staged_record(
        &mut journal,
        incoming_id,
        destination,
        scope,
        mappings,
        identity,
    )
}

fn export_staged_record(
    journal: &mut SaveSyncJournal,
    incoming_id: &str,
    destination: &Path,
    scope: &SaveSyncScope,
    mappings: &[SaveMapping],
    identity: &SaveSyncIdentity,
) -> Result<()> {
    validate_export_configuration(scope, mappings, identity)?;
    let record = journal
        .incoming_saves()?
        .into_iter()
        .find(|record| record.id == incoming_id)
        .ok_or_else(|| {
            rommfs_core::error::Error::Unsupported(
                "incoming save is no longer available for export".into(),
            )
        })?;
    let mapping = mappings
        .iter()
        .find(|mapping| mapping.rom_key == record.rom_key)
        .ok_or_else(|| {
            rommfs_core::error::Error::Unsupported(
                "incoming save no longer has a visible catalogue mapping".into(),
            )
        })?;
    validate_export_configuration(scope, std::slice::from_ref(mapping), identity)?;
    let targets = mappings
        .iter()
        .map(|mapping| mapping.target_path.clone())
        .collect::<Vec<_>>();
    export_incoming(
        &record.path,
        destination,
        &record.content_hash,
        scope,
        &targets,
    )
}

fn emit_queue_status(
    context: &ActorContext<'_>,
    mappings: &[SaveMapping],
    journal: &SaveSyncJournal,
    scheduler: &SaveSyncScheduler,
    state: &ActorState,
    sink: &EventSink,
) -> Result<()> {
    let incoming = journal
        .incoming_saves()?
        .into_iter()
        .map(|record| SaveSyncIncomingStatus {
            incoming_id: record.id,
            rom_id: record.rom_key.rom_id,
            rom_name: mappings
                .iter()
                .find(|mapping| mapping.rom_key == record.rom_key)
                .map(|mapping| mapping.visible_rom_name.clone())
                .unwrap_or_else(|| format!("ROM {}", record.rom_key.rom_id)),
            remote_id: record.remote_id,
            content_hash: record.content_hash,
            reason: record.reason,
            state: record.state,
        })
        .collect::<Vec<_>>();
    let snapshots = journal.snapshots()?;
    let pending_outbound = scheduler.pending_count()
        + snapshots
            .iter()
            .filter(|snapshot| snapshot.state != SnapshotState::RemoteComplete)
            .count();
    let mut games = state.game_states.values().cloned().collect::<Vec<_>>();
    for game in &mut games {
        if let Some(mapping) = mappings
            .iter()
            .find(|mapping| mapping.rom_key.rom_id == game.rom_id)
        {
            if let Some(failure) = game_failure(&mapping.rom_key, state) {
                game.issue = Some(failure.to_owned());
            }
        }
    }
    games.sort_by_key(|game| game.rom_id);
    sink.emit(AppEvent::SaveSyncQueueUpdated(SaveSyncQueueStatus {
        session_id: context.session_id,
        mapped_games: mappings.len(),
        reconciled_games: state.reconciled_keys.len(),
        pending_outbound,
        pending_incoming: incoming.len(),
        attention_games: state.attention_keys.len(),
        network_paused: state.network_paused,
        authentication_required: state.authentication_required,
        actor_failed: false,
        failure: queue_failure(state, mappings),
        games,
        incoming,
    }));
    Ok(())
}

fn same_path(left: &Path, right: &Path) -> bool {
    let key = |path: &Path| path.to_string_lossy().replace('/', "\\").to_lowercase();
    key(left) == key(right)
}
