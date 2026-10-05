//! Opt-in save watcher and upload pipeline. Filesystem callbacks only enqueue
//! path hints; hashing/journaling run on the actor and HTTP runs on one worker.

mod actor;
mod files;
mod gate;
mod incoming;
mod incoming_transfer;
mod transfer;

#[cfg(test)]
#[path = "save_sync_agent/tests.rs"]
mod tests;

pub(crate) use actor::export_pending_incoming;
pub(crate) use actor::SaveSyncAgent;
pub(crate) use gate::SaveSyncCommandGate;
#[cfg(test)]
use transfer::{remote_filename_matches_revision, validate_configuration};
