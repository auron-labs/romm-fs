//! Linux FUSE adapter for desktop-mode testing on non-Windows hosts.
//!
//! It projects the same read-only `RommFs` the Cloud Files driver serves:
//! enumeration and stat stay metadata-only, and the first data read hydrates
//! the original bytes through the shared download manager. The Windows CFAPI
//! mount remains the product driver; this exists so the mount lifecycle —
//! prepare, claim, start, read, stop — is exercisable on a Linux desktop.

use anyhow::{Context, Result};
use fuser::{
    AccessFlags, Config, Errno, FileAttr, FileHandle, FileType, Filesystem, FopenFlags, Generation,
    INodeNo, MountOption, OpenAccMode, OpenFlags, ReplyAttr, ReplyData, ReplyDirectory, ReplyEmpty,
    ReplyEntry, ReplyOpen, Request, SessionACL,
};
use rommfs_core::cache::ActiveGuard;
use rommfs_core::filesystem::RommFs;
use rommfs_core::tree::{EntryKind, NodeMeta};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// Kernel-side caching of projected names and attributes. The mounted tree is
/// an immutable snapshot, but a short TTL keeps reclamation of open handles
/// and stale dentries bounded without making listing chatty.
const ATTR_TTL: Duration = Duration::from_secs(5);

/// One mounted read-only FUSE session. `stop` unmounts and joins the request
/// threads; in-flight downloads are cancelled by the caller beforehand.
pub struct FuseMount {
    session: fuser::BackgroundSession,
}

impl FuseMount {
    pub fn stop(self) {
        // Errors here are unmount-time diagnostics only; the mount point is
        // already detached or the session is already gone.
        let _ = self.session.umount_and_join();
    }
}

/// Mount `fs` at `root` (an existing, app-owned empty directory prepared and
/// claimed by the caller). Returns once the kernel handshake succeeded.
pub fn mount(fs: Arc<RommFs>, root: &std::path::Path) -> Result<FuseMount> {
    // The claimed mount directory's owner is the mounting user; report it as
    // the owner of every projected node so access checks behave like a
    // user-owned tree.
    let meta = std::fs::metadata(root).context("stat mount root for uid/gid")?;
    use std::os::unix::fs::MetadataExt;
    let backend = RommFuseFs {
        fs,
        open: Mutex::new(HashMap::new()),
        next_fh: AtomicU64::new(1),
        uid: meta.uid(),
        gid: meta.gid(),
        mounted_at: SystemTime::now(),
    };
    let mut config = Config::default();
    config.mount_options = vec![
        MountOption::RO,
        MountOption::FSName("rommfs".into()),
        MountOption::Subtype("rommfs".into()),
        MountOption::NoDev,
        MountOption::NoSuid,
        MountOption::NoExec,
        MountOption::DefaultPermissions,
    ];
    config.acl = SessionACL::Owner;
    config.n_threads = Some(4);
    let session =
        fuser::spawn_mount(backend, root, &config).context("start FUSE session on mount root")?;
    Ok(FuseMount { session })
}

struct RommFuseFs {
    fs: Arc<RommFs>,
    /// Live open guards keyed by file handle, so a sweep cannot evict bytes a
    /// reader still has open (same contract as `RommFs::read_at`).
    open: Mutex<HashMap<u64, ActiveGuard>>,
    next_fh: AtomicU64,
    uid: u32,
    gid: u32,
    mounted_at: SystemTime,
}

impl RommFuseFs {
    fn attr(&self, meta: NodeMeta) -> FileAttr {
        let (kind, perm) = match meta.kind {
            EntryKind::Directory => (FileType::Directory, 0o555),
            EntryKind::File => (FileType::RegularFile, 0o444),
        };
        FileAttr {
            ino: INodeNo(meta.inode),
            size: meta.size,
            blocks: meta.size.div_ceil(512),
            atime: self.mounted_at,
            mtime: self.mounted_at,
            ctime: self.mounted_at,
            crtime: self.mounted_at,
            kind,
            perm,
            nlink: if meta.kind == EntryKind::Directory {
                2
            } else {
                1
            },
            uid: self.uid,
            gid: self.gid,
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
    }
}

fn fuse_kind(kind: EntryKind) -> FileType {
    match kind {
        EntryKind::Directory => FileType::Directory,
        EntryKind::File => FileType::RegularFile,
    }
}

fn errno_of(error: &rommfs_core::Error) -> Errno {
    match error {
        rommfs_core::Error::Io(io) => io.raw_os_error().map(Errno::from_i32).unwrap_or(Errno::EIO),
        _ => Errno::EIO,
    }
}

impl Filesystem for RommFuseFs {
    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let Some(name) = name.to_str() else {
            reply.error(Errno::ENOENT);
            return;
        };
        match self.fs.lookup(parent.0, name) {
            Some(meta) => reply.entry(&ATTR_TTL, &self.attr(meta), Generation(meta.generation)),
            None => reply.error(Errno::ENOENT),
        }
    }

    fn getattr(&self, _req: &Request, ino: INodeNo, _fh: Option<FileHandle>, reply: ReplyAttr) {
        match self.fs.metadata(ino.0) {
            Some(meta) => reply.attr(&ATTR_TTL, &self.attr(meta)),
            None => reply.error(Errno::ENOENT),
        }
    }

    fn access(&self, _req: &Request, ino: INodeNo, mask: AccessFlags, reply: ReplyEmpty) {
        if self.fs.metadata(ino.0).is_none() {
            reply.error(Errno::ENOENT);
            return;
        }
        if mask.contains(AccessFlags::W_OK) {
            reply.error(Errno::EACCES);
        } else {
            reply.ok();
        }
    }

    fn open(&self, _req: &Request, ino: INodeNo, flags: OpenFlags, reply: ReplyOpen) {
        let Some(meta) = self.fs.metadata(ino.0) else {
            reply.error(Errno::ENOENT);
            return;
        };
        if meta.kind == EntryKind::Directory {
            reply.error(Errno::EISDIR);
            return;
        }
        if flags.acc_mode() != OpenAccMode::O_RDONLY {
            reply.error(Errno::EROFS);
            return;
        }
        let Some(guard) = self.fs.note_open(ino.0) else {
            reply.error(Errno::ENOENT);
            return;
        };
        let fh = self.next_fh.fetch_add(1, Ordering::Relaxed);
        self.open.lock().unwrap().insert(fh, guard);
        reply.opened(FileHandle(fh), FopenFlags::empty());
    }

    fn read(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<fuser::LockOwner>,
        reply: ReplyData,
    ) {
        // The first read hydrates through the download manager; later reads
        // come from the private cache. An at/past-EOF read returns empty.
        let mut buf = vec![0u8; size as usize];
        match self.fs.read_at(ino.0, offset, &mut buf) {
            Ok(filled) => reply.data(&buf[..filled]),
            Err(error) => reply.error(errno_of(&error)),
        }
    }

    fn release(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        _flags: OpenFlags,
        _lock_owner: Option<fuser::LockOwner>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        self.open.lock().unwrap().remove(&fh.0);
        reply.ok();
    }

    fn opendir(&self, _req: &Request, ino: INodeNo, _flags: OpenFlags, reply: ReplyOpen) {
        match self.fs.metadata(ino.0) {
            Some(meta) if meta.kind == EntryKind::Directory => {
                reply.opened(FileHandle(0), FopenFlags::empty())
            }
            Some(_) => reply.error(Errno::ENOTDIR),
            None => reply.error(Errno::ENOENT),
        }
    }

    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        let Some(meta) = self.fs.metadata(ino.0) else {
            reply.error(Errno::ENOENT);
            return;
        };
        // The logical entry stream is ".", "..", then the projected children
        // starting at index 2. The offset handed to the kernel for an entry is
        // the index the next request resumes from.
        let mut index = offset;
        if index == 0 {
            if reply.add(INodeNo(ino.0), 1, FileType::Directory, ".") {
                reply.ok();
                return;
            }
            index = 1;
        }
        if index == 1 {
            let parent = if meta.parent == 0 { ino.0 } else { meta.parent };
            if reply.add(INodeNo(parent), 2, FileType::Directory, "..") {
                reply.ok();
                return;
            }
            index = 2;
        }
        let start = index - 2;
        let Some((entries, _, _)) = self.fs.read_dir(ino.0, start, usize::MAX) else {
            reply.error(Errno::ENOENT);
            return;
        };
        for (position, entry) in entries.iter().enumerate() {
            let next = start + 3 + position as u64;
            if reply.add(
                INodeNo(entry.inode),
                next,
                fuse_kind(entry.kind),
                &entry.name,
            ) {
                break;
            }
        }
        reply.ok();
    }

    fn statfs(&self, _req: &Request, _ino: INodeNo, reply: fuser::ReplyStatfs) {
        let files = self.fs.catalogue().entries.len() as u64;
        let total: u64 = self.fs.catalogue().entries.iter().map(|e| e.size).sum();
        // Read-only projection: no free space and no reservable inodes, but the
        // real file count so `df -i` output stays honest.
        reply.statfs(total.div_ceil(512), 0, 0, files, 0, 512, 255, 0);
    }
}

impl Drop for RommFuseFs {
    fn drop(&mut self) {
        self.open.lock().unwrap().clear();
    }
}
