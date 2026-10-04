//! Windows-only fsk adapter implementation (module is cfg(windows) via lib.rs).
//!
//! Verified fsk 0.0.9 mechanics this relies on (see .planning/BACKEND-DECISION.md):
//! - Enumeration collects ALL entries via repeated `read_directory` calls,
//!   then pages through `PrjFillDirEntryBuffer` — small OS buffers are fsk's
//!   problem, ours is to page correctly via `RommFs::read_dir` cookies.
//! - Content flows only through `GetFileData` -> `Filesystem::read` ->
//!   `PrjWriteFileData`; `read` may block for the lazy download (R3).
//! - `MountOptions.read_only` is IGNORED on Windows — read-only is enforced
//!   by vetoing mutation notifications in `raw_event`.
//! - Notifications arrive as `Operation::Notification` raw events with the
//!   `PRJ_NOTIFY_*` value OR'd into `flags`; `RawOutcome::Reject` becomes the
//!   callback HRESULT — for the PRE_* notifications that vetoes the operation.
//! - `PRJ_CALLBACK_DATA` (borrowed, callback-scoped) carries `FilePathName`,
//!   `FileId` (per-open handle GUID) and `NamespaceVirtualizationContext`.
//! - `open_file`/`close_file` never fire on Windows; open tracking uses
//!   `PRJ_NOTIFY_FILE_OPENED` / `*_HANDLE_CLOSED_*` instead.

use fsk::raw::{NativeRequest, Operation, RawEvent, RawEventMask, RawOutcome};
use fsk::{
    DirectoryEntry, DirectorySink, Error as FskError, FileType, Filesystem, Metadata,
    ReadDirectoryResult, StatFs,
};
use rommfs_core::cache::{ActiveGuard, HydratedRemover};
use rommfs_core::fscore::RommFs;
use rommfs_core::tree::EntryKind;
use std::collections::HashMap;
use std::ffi::c_void;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND};
use windows_sys::Win32::Storage::ProjectedFileSystem::{
    PrjDeleteFile, PRJ_CALLBACK_DATA, PRJ_NOTIFY_FILE_HANDLE_CLOSED_FILE_DELETED,
    PRJ_NOTIFY_FILE_HANDLE_CLOSED_FILE_MODIFIED, PRJ_NOTIFY_FILE_HANDLE_CLOSED_NO_MODIFICATION,
    PRJ_NOTIFY_FILE_OPENED, PRJ_NOTIFY_FILE_OVERWRITTEN, PRJ_NOTIFY_FILE_PRE_CONVERT_TO_FULL,
    PRJ_NOTIFY_PRE_DELETE, PRJ_NOTIFY_PRE_RENAME, PRJ_NOTIFY_PRE_SET_HARDLINK,
    PRJ_UPDATE_FAILURE_CAUSES, PRJ_UPDATE_FAILURE_CAUSE_NONE, PRJ_UPDATE_NONE,
};

/// Mount-root validation (PRD §5): mount only into an empty directory or a
/// recognized app-owned root (marker file we wrote). Never over an existing
/// ROM library, never recursively cleared.
const ROOT_MARKER: &str = ".rommfs-root";

/// ENOENT/EACCES/EINVAL in fsk's Darwin-style positive error space; the fsk
/// Windows adapter maps them to ERROR_FILE_NOT_FOUND / ERROR_ACCESS_DENIED /
/// ERROR_INVALID_PARAMETER.
const ENOENT: FskError = FskError(2);
const EACCES: FskError = FskError(13);
const EINVAL: FskError = FskError(22);

/// Pre-mutation notifications we veto so projected ROM entries stay
/// read-only (R2). `FILE_OVERWRITTEN` is post-only (ProjFS ignores the
/// veto's effect there) but rejecting it costs nothing.
const MUTATION_VETO_MASK: u32 = PRJ_NOTIFY_PRE_DELETE
    | PRJ_NOTIFY_PRE_RENAME
    | PRJ_NOTIFY_PRE_SET_HARDLINK
    | PRJ_NOTIFY_FILE_PRE_CONVERT_TO_FULL
    | PRJ_NOTIFY_FILE_OVERWRITTEN;

/// Handle-close notifications that release the open-file guard.
const CLOSE_MASK: u32 = PRJ_NOTIFY_FILE_HANDLE_CLOSED_NO_MODIFICATION
    | PRJ_NOTIFY_FILE_HANDLE_CLOSED_FILE_MODIFIED
    | PRJ_NOTIFY_FILE_HANDLE_CLOSED_FILE_DELETED;

/// What may be mounted.
pub enum RootCheck {
    EmptyReady,
    RecognizedOwned,
}

/// Inspect `root`: Ok(kind) when mountable, Err(reason) otherwise.
/// `server_id` scopes the marker so different servers never share a root.
pub fn check_mount_root(root: &Path, server_id: &str) -> anyhow::Result<RootCheck> {
    if !root.exists() {
        anyhow::bail!("mount root {} does not exist", root.display());
    }
    if !root.is_dir() {
        anyhow::bail!("mount root {} is not a directory", root.display());
    }
    let marker = root.join(ROOT_MARKER);
    if marker.is_file() {
        let owner = std::fs::read_to_string(&marker).unwrap_or_default();
        if owner.trim() == server_id {
            return Ok(RootCheck::RecognizedOwned);
        }
        anyhow::bail!(
            "{} is a RomMFS-managed root for a different server",
            root.display()
        );
    }
    if std::fs::read_dir(root)?.next().is_none() {
        return Ok(RootCheck::EmptyReady);
    }
    anyhow::bail!(
        "{} is not empty and is not a RomMFS-managed root",
        root.display()
    )
}

/// Write the app-owned marker (safe to re-run).
pub fn claim_mount_root(root: &Path, server_id: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(root)?;
    std::fs::write(root.join(ROOT_MARKER), server_id)?;
    Ok(())
}

/// ProjFS namespace context captured lazily from raw events.
/// Ownership: only used while `active` is true and the `fsk::MountSession`
/// below is alive; cleared before `PrjStopVirtualizing` runs on drop.
#[derive(Default)]
pub struct ProjfsHandle {
    context: AtomicUsize,
    active: AtomicBool,
    /// Once sealed (mount stopping) a late callback must not re-arm the
    /// handle — the context it captured would dangle mid-stop.
    sealed: AtomicBool,
}

impl ProjfsHandle {
    /// Store the context observed in a raw event (value copy, not a pointer
    /// into callback storage).
    pub fn note_context(&self, namespace_context: isize) {
        if self.sealed.load(Ordering::SeqCst) {
            return;
        }
        self.context
            .store(namespace_context as usize, Ordering::SeqCst);
        self.active.store(true, Ordering::SeqCst);
    }

    /// Prevent further API use (call before stopping the provider).
    pub fn seal(&self) {
        self.sealed.store(true, Ordering::SeqCst);
        self.active.store(false, Ordering::SeqCst);
    }

    /// True once a real namespace context has been captured.
    pub fn is_armed(&self) -> bool {
        self.active.load(Ordering::SeqCst) && self.context.load(Ordering::SeqCst) != 0
    }

    /// The captured context while armed; None before the first event or
    /// after `seal`. Callers must not retain it beyond the mount lifetime.
    fn context(&self) -> Option<*mut c_void> {
        if !self.active.load(Ordering::SeqCst) {
            return None;
        }
        let ctx = self.context.load(Ordering::SeqCst);
        (ctx != 0).then_some(ctx as *mut c_void)
    }
}

/// Evicts the ProjFS-hydrated copy of a file via `PrjDeleteFile` — the
/// "provider-aware" removal PRD R4 requires. Ordinary `DeleteFile` is
/// forbidden here (a tombstone would hide the projected ROM).
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
    /// `rel_path` is `platform_dir/file_name` (forward slashes).
    /// Benign not-found is Ok; any real failure defers the whole eviction.
    fn remove_hydrated(&self, rel_path: &str) -> io::Result<()> {
        let Some(context) = self.handle.context() else {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "projfs namespace context not active (unmounted or not yet armed)",
            ));
        };
        let rel = rel_path.replace('/', "\\");
        let wide: Vec<u16> = rel.encode_utf16().chain(std::iter::once(0)).collect();
        let mut causes: PRJ_UPDATE_FAILURE_CAUSES = PRJ_UPDATE_FAILURE_CAUSE_NONE;
        // SAFETY: `context` is the live namespace context captured during a
        // callback while `active` held — sealed before PrjStopVirtualizing.
        // `wide` is a valid null-terminated UTF-16 buffer for this call.
        let hr = unsafe { PrjDeleteFile(context, wide.as_ptr(), PRJ_UPDATE_NONE, &mut causes) };
        if hr >= 0 {
            return Ok(());
        }
        // HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND):
        // the entry was never hydrated or is already gone — nothing to remove.
        let hru = hr as u32;
        if (hru & 0xffff_0000) == 0x8007_0000
            && matches!(hru & 0xffff, ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND)
        {
            return Ok(());
        }
        tracing::warn!(
            root = %self.root.display(),
            rel_path,
            hresult = format_args!("0x{hru:08x}"),
            causes,
            "PrjDeleteFile failed; eviction deferred"
        );
        Err(io::Error::other(format!(
            "PrjDeleteFile({rel_path}) failed with HRESULT 0x{hru:08x} (causes {causes})"
        )))
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
    pub fn mount(
        fs: Arc<RommFs>,
        root: impl AsRef<Path>,
    ) -> anyhow::Result<(Self, Arc<ProjfsHandle>)> {
        Self::mount_with_handle(fs, root, Arc::new(ProjfsHandle::default()))
    }

    /// Mount with a caller-supplied handle. Needed because the cache
    /// `Evictor` (built inside `RommFs`) wants a `ProjfsRemover` bound to the
    /// mount's handle *before* `mount` can return it: create the
    /// `Arc<ProjfsHandle>` first, give a clone to the remover, pass the same
    /// one here.
    pub fn mount_with_handle(
        fs: Arc<RommFs>,
        root: impl AsRef<Path>,
        handle: Arc<ProjfsHandle>,
    ) -> anyhow::Result<(Self, Arc<ProjfsHandle>)> {
        let root = root.as_ref().to_path_buf();
        let adapter = RommFsk {
            core: fs,
            handle: Arc::clone(&handle),
            opens: parking_lot::Mutex::new(HashMap::new()),
        };
        let session = fsk::MountSession::mount(adapter, &root)
            .map_err(|e| anyhow::anyhow!("ProjFS mount at {} failed: {e}", root.display()))?;

        // The namespace context only ever travels inside ProjFS callbacks;
        // a directory read on the fresh root forces one so `PrjDeleteFile`
        // can run as soon as the first eviction does. Poll briefly — the
        // callback is synchronous enough that this normally lands instantly.
        let deadline = Instant::now() + Duration::from_secs(2);
        while !handle.is_armed() && Instant::now() < deadline {
            let _ = std::fs::read_dir(&root).map(|it| it.count());
            std::thread::sleep(Duration::from_millis(25));
        }
        if !handle.is_armed() {
            tracing::warn!(
                root = %root.display(),
                "projfs namespace context not yet captured; hydrated eviction will defer until the first callback"
            );
        }
        Ok((
            Self {
                session,
                root,
                handle: Arc::clone(&handle),
            },
            handle,
        ))
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
    /// Per-open `FileId` (GUID bytes) -> core active-use guard; released on
    /// the matching handle-closed notification (PRD R4 access tracking).
    opens: parking_lot::Mutex<HashMap<[u8; 16], ActiveGuard>>,
}

impl RommFsk {
    /// `PRJ_CALLBACK_DATA.FilePathName` -> ROM inode, for notifications that
    /// carry no inode of their own. Root/unknown paths yield None.
    fn inode_for_callback(&self, data: &PRJ_CALLBACK_DATA) -> Option<u64> {
        if data.FilePathName.is_null() {
            return None;
        }
        // SAFETY: FilePathName is a callback-scoped null-terminated string.
        let mut len = 0usize;
        unsafe {
            while *data.FilePathName.add(len) != 0 {
                len += 1;
            }
        }
        let units = unsafe { std::slice::from_raw_parts(data.FilePathName, len) };
        let path = String::from_utf16_lossy(units);
        let components: Vec<&str> = path.split(['\\', '/']).filter(|s| !s.is_empty()).collect();
        self.core.inode_for_path(&components)
    }
}

/// `PRJ_CALLBACK_DATA.FileId` as a map key.
fn file_id_bytes(data: &PRJ_CALLBACK_DATA) -> [u8; 16] {
    let g = &data.FileId;
    let mut out = [0u8; 16];
    out[..4].copy_from_slice(&g.data1.to_le_bytes());
    out[4..6].copy_from_slice(&g.data2.to_le_bytes());
    out[6..8].copy_from_slice(&g.data3.to_le_bytes());
    out[8..].copy_from_slice(&g.data4);
    out
}

fn file_type_of(kind: EntryKind) -> FileType {
    match kind {
        EntryKind::Directory => FileType::Directory,
        EntryKind::File => FileType::File,
    }
}

impl Filesystem for RommFsk {
    fn statfs(&self) -> fsk::Result<StatFs> {
        // Fixed plausible geometry; the catalogue is the real size source.
        Ok(StatFs {
            block_size: 4096,
            io_size: 1 << 20,
            total_bytes: 1 << 40,
            free_bytes: 1 << 39,
            available_bytes: 1 << 39,
            files: self
                .core
                .read_dir(rommfs_core::tree::ROOT_INODE, 0, usize::MAX)
                .map(|(e, _, _)| e.len() as u64)
                .unwrap_or(0),
            free_files: u64::MAX,
        })
    }

    fn metadata(&self, inode: u64) -> fsk::Result<Metadata> {
        let meta = self.core.metadata(inode).ok_or(ENOENT)?;
        Ok(Metadata {
            inode: meta.inode,
            parent: meta.parent,
            size: meta.size,
            allocated_size: 0,
            generation: meta.generation,
            created_ns: 0,
            modified_ns: 0,
            accessed_ns: 0,
            mode: match meta.kind {
                EntryKind::Directory => 0o555,
                EntryKind::File => 0o444,
            },
            uid: 0,
            gid: 0,
            link_count: 1,
            kind: file_type_of(meta.kind),
        })
    }

    fn lookup(&self, parent: u64, name: &[u8]) -> fsk::Result<(u64, FileType)> {
        // Adapter names arrive as WTF-8 produced by our own UTF-8 names; the
        // lossy path only affects names that would not match anyway.
        let name = String::from_utf8_lossy(name);
        let meta = self.core.lookup(parent, &name).ok_or(ENOENT)?;
        Ok((meta.inode, file_type_of(meta.kind)))
    }

    fn read_directory(
        &self,
        inode: u64,
        cookie: u64,
        _verifier: u64,
        sink: &mut dyn DirectorySink,
    ) -> fsk::Result<ReadDirectoryResult> {
        // fsk collects every entry on StartDirectoryEnumeration and drives
        // pagination itself; our job is correct continuation. The core's
        // cookie is the next child index — pass it straight through.
        const PAGE: usize = 512;
        let (entries, next_cookie, eof) = self.core.read_dir(inode, cookie, PAGE).ok_or(EINVAL)?;
        for entry in &entries {
            let pushed = sink.push(DirectoryEntry {
                name: entry.name.as_bytes(),
                inode: entry.inode,
                kind: file_type_of(entry.kind),
                next_cookie: entry.cookie,
            });
            if !pushed {
                return Err(FskError::IO);
            }
        }
        Ok(ReadDirectoryResult {
            verifier: 1, // catalogue is immutable for the mount session
            next_cookie,
            eof,
        })
    }

    /// The only content path (fsk GetFileData). `read_at` performs the lazy
    /// single-flight download on first access and may block for it (R3).
    fn read(&self, inode: u64, offset: u64, output: &mut [u8]) -> fsk::Result<usize> {
        self.core.read_at(inode, offset, output).map_err(|e| {
            tracing::warn!(inode, offset, error = %e, "content read failed");
            FskError::IO
        })
    }

    fn raw_event_mask(&self) -> RawEventMask {
        // Notification drives vetoes + open tracking; the rest exist so the
        // namespace context is captured on any early access path (a stat,
        // an enum, a lookup, a read all deliver it).
        RawEventMask::operation(Operation::Notification)
            .union(RawEventMask::operation(Operation::ReadDirectory))
            .union(RawEventMask::operation(Operation::Lookup))
            .union(RawEventMask::operation(Operation::Metadata))
            .union(RawEventMask::operation(Operation::Read))
    }

    fn raw_event(&self, event: RawEvent<'_>) -> RawOutcome {
        let NativeRequest::Windows {
            callback_data,
            namespace_context,
            flags,
            ..
        } = event.native
        else {
            return RawOutcome::Continue;
        };
        self.handle.note_context(namespace_context);
        if std::env::var_os("ROMMFS_FSK_DEBUG").is_some() {
            eprintln!("[raw_event] op={:?} flags=0x{flags:08x}", event.operation);
        }
        if event.operation != Operation::Notification {
            return RawOutcome::Continue;
        }

        // Read-only enforcement (MountOptions.read_only is ignored by fsk on
        // Windows): veto every mutation-preview notification ProjFS offers.
        if flags & MUTATION_VETO_MASK != 0 {
            return RawOutcome::Reject(EACCES);
        }

        // Open/close tracking: FilePathName+FileId are borrowed for this
        // call only — resolve and copy what we need, retain nothing.
        if flags & (PRJ_NOTIFY_FILE_OPENED | CLOSE_MASK) != 0 {
            // SAFETY: callback_data points at a valid PRJ_CALLBACK_DATA for
            // the duration of this synchronous callback.
            let data = unsafe { &*callback_data.cast::<PRJ_CALLBACK_DATA>().as_ptr() };
            if (data.Size as usize) < std::mem::size_of::<PRJ_CALLBACK_DATA>() {
                return RawOutcome::Continue;
            }
            if flags & PRJ_NOTIFY_FILE_OPENED != 0 {
                if let Some(inode) = self.inode_for_callback(data) {
                    if let Some(guard) = self.core.note_open(inode) {
                        self.opens.lock().insert(file_id_bytes(data), guard);
                    }
                }
            } else {
                self.opens.lock().remove(&file_id_bytes(data));
            }
        }
        RawOutcome::Continue
    }
}
