//! Windows-only fsk adapter implementation (module is cfg(windows) via lib.rs).

use rommfs_core::cache::HydratedRemover;
use rommfs_core::fscore::RommFs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

/// Mount-root validation (PRD §5): mount only into an empty directory or a
/// recognized app-owned root (marker file we wrote). Never over an existing
/// ROM library, never recursively cleared.
const ROOT_MARKER: &str = ".rommfs-root";

/// What may be mounted.
pub enum RootCheck {
    EmptyReady,
    RecognizedOwned,
}

/// Inspect `root`: Ok(kind) when mountable, Err(reason) otherwise.
/// `server_id` scopes the marker so different servers never share a root.
pub fn check_mount_root(root: &Path, server_id: &str) -> anyhow::Result<RootCheck> {
    let _ = (root, server_id);
    todo!("exists+is_dir; empty -> EmptyReady; marker w/ matching server_id -> RecognizedOwned; else Err")
}

/// Write the app-owned marker (safe to re-run).
pub fn claim_mount_root(root: &Path, server_id: &str) -> anyhow::Result<()> {
    let _ = (root, server_id);
    todo!()
}

/// ProjFS namespace context captured lazily from raw events.
/// Ownership: only used while `active` is true and the `fsk::MountSession`
/// below is alive; cleared before `PrjStopVirtualizing` runs on drop.
#[derive(Default)]
pub struct ProjfsHandle {
    context: AtomicUsize,
    active: AtomicBool,
}

impl ProjfsHandle {
    /// Store the context observed in a raw event (value copy, not a pointer
    /// into callback storage).
    pub fn note_context(&self, namespace_context: isize) {
        self.context.store(namespace_context as usize, Ordering::SeqCst);
        self.active.store(true, Ordering::SeqCst);
    }

    /// Prevent further API use (call before stopping the provider).
    pub fn seal(&self) {
        self.active.store(false, Ordering::SeqCst);
    }
}

/// Evicts the ProjFS-hydrated copy of a file via `PrjDeleteFile` /
/// `PrjUpdateFileIfNeeded` — the "provider-aware" removal PRD R4 requires.
/// Ordinary `DeleteFile` is forbidden here (tombstone hides the ROM).
pub struct ProjfsRemover {
    root: PathBuf,
    handle: Arc<ProjfsHandle>,
}

impl ProjfsRemover {
    pub fn new(root: PathBuf, handle: Arc<ProjfsHandle>) -> Self {
        Self { root, handle }
    }
}

impl HydratedRemover for ProjfsRemover {
    fn remove_hydrated(&self, rel_path: &str) -> std::io::Result<()> {
        let _ = rel_path;
        todo!("PrjDeleteFile(ctx, rel, 0, &causes); treat NOT_FOUND as ok; log causes; Err -> defer")
    }
}

/// A live mount: ProjFS provider over `RommFs` inside `root`.
/// `stop()` seals the handle then drops the session (PrjStopVirtualizing).
pub struct WindowsMount {
    session: fsk::MountSession,
    root: PathBuf,
    handle: Arc<ProjfsHandle>,
}

impl WindowsMount {
    /// Mount `fs` at `root` (must pass `check_mount_root` first).
    /// After mount, triggers one lightweight access so the namespace context
    /// is captured before any eviction runs.
    pub fn mount(fs: Arc<RommFs>, root: impl AsRef<Path>) -> anyhow::Result<(Self, Arc<ProjfsHandle>)> {
        let _ = (fs, root.as_ref());
        todo!("RommFsk::new(fs, handle); fsk::MountSession::mount; read_dir(root) to capture ctx; return")
    }

    /// Stop the provider. Pending work must be released (PRD R3).
    pub fn stop(self) {
        self.handle.seal();
        drop(self.session);
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// fsk `Filesystem` implementation over `RommFs`.
struct RommFsk {
    core: Arc<RommFs>,
    handle: Arc<ProjfsHandle>,
}

// impl fsk::Filesystem for RommFsk — statfs/metadata/lookup/read_directory/
// read delegate to RommFs; mutation methods stay default-ENOTSUP;
// raw_event_mask = Notification | ReadDirectory | Lookup;
// raw_event: capture namespace_context (any Windows event);
//   Operation::Notification + PRE_DELETE|PRE_RENAME|PRE_SET_HARDLINK|
//   PRE_CONVERT_TO_FULL|FILE_OVERWRITTEN -> Reject(ACCESS_DENIED);
//   FILE_OPENED -> note_open(inode for FilePathName), keep guard till
//   FILE_HANDLE_CLOSED_NO_MODIFICATION / CLOSED_FILE_MODIFIED (also vetoed
//   earlier for modified);
//   Operation::Cancel -> release waiters for that command_id if tracked.
