use anyhow::Context;
use rommfs_core::cache::ActiveGuard;
use rommfs_core::fscore::RommFs;
use rommfs_core::tree::{EntryKind, NodeMeta, ROOT_INODE};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use windows_sys::Win32::Foundation::{
    LocalFree, ERROR_INSUFFICIENT_BUFFER, STATUS_END_OF_FILE, STATUS_IO_DEVICE_ERROR,
    STATUS_MEDIA_WRITE_PROTECTED, STATUS_NAME_TOO_LONG, STATUS_NOT_A_DIRECTORY,
    STATUS_OBJECT_NAME_INVALID, STATUS_OBJECT_NAME_NOT_FOUND,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_APPEND_DATA, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_READONLY, FILE_DELETE_CHILD,
    FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, FILE_WRITE_EA, WRITE_DAC, WRITE_OWNER,
};
use winfsp::filesystem::{
    DirBuffer, DirInfo, DirMarker, FileInfo, FileSecurity, FileSystemContext, OpenFileInfo,
    VolumeInfo, WideNameInfo,
};
use winfsp::host::{FileSystemHost, VolumeParams};
use winfsp::{FspError, U16CStr};

/// A live read-only WinFsp volume. Dropping it stops dispatch and unmounts.
pub struct WindowsMount {
    host: Option<FileSystemHost<RommWinFsp>>,
    root: PathBuf,
}

impl WindowsMount {
    /// Mount an empty, claimed directory. Never remove files from a former mount.
    pub fn mount(fs: Arc<RommFs>, root: impl AsRef<Path>) -> anyhow::Result<Self> {
        winfsp::winfsp_init().map_err(|e| {
            anyhow::anyhow!("WinFsp runtime unavailable ({e:?}); install WinFsp 2.1 or later")
        })?;
        crate::root::check_empty_root(root.as_ref())?;
        let root = std::fs::canonicalize(root.as_ref())?;
        let mut params = VolumeParams::new();
        params
            .sector_size(4096)
            .sectors_per_allocation_unit(1)
            .max_component_length(255)
            .filesystem_name("RomMFS")
            .case_sensitive_search(false)
            .case_preserved_names(true)
            .unicode_on_disk(true)
            .read_only_volume(true)
            .persistent_acls(true)
            .post_cleanup_when_modified_only(false)
            .flush_and_purge_on_cleanup(true)
            .file_info_timeout(1000);
        let adapter = RommWinFsp {
            core: fs,
            security: read_only_security()?,
        };
        let mut host =
            FileSystemHost::<RommWinFsp>::new(params, adapter).context("create WinFsp volume")?;
        // WinFsp creates a delete-on-close mount directory. remove_dir fails
        // safely if another process put anything into the directory meanwhile.
        std::fs::remove_dir(&root).context("prepare empty WinFsp mount directory")?;
        if let Err(e) = host.mount(&root).and_then(|()| host.start()) {
            drop(host);
            let _ = std::fs::create_dir(&root);
            anyhow::bail!("WinFsp mount at {} failed: {e:?}", root.display());
        }
        Ok(Self {
            host: Some(host),
            root,
        })
    }

    pub fn stop(self) {
        drop(self);
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for WindowsMount {
    fn drop(&mut self) {
        // Host Drop stops the dispatcher, removes the junction and releases
        // every file context before we restore the empty directory.
        drop(self.host.take());
        if let Err(e) = std::fs::create_dir(&self.root) {
            if e.kind() != std::io::ErrorKind::AlreadyExists {
                tracing::warn!(error = %e, root = %self.root.display(), "could not restore mount directory");
            }
        }
    }
}

fn read_only_security() -> anyhow::Result<Vec<u8>> {
    let sddl: Vec<u16> = "D:P(A;;FRFX;;;WD)".encode_utf16().chain(Some(0)).collect();
    let mut descriptor = std::ptr::null_mut();
    let mut size = 0;
    // SAFETY: sddl is terminated and both outputs remain valid for this call.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            &mut size,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: successful conversion allocated size bytes, freed after copying.
    let bytes =
        unsafe { std::slice::from_raw_parts(descriptor.cast::<u8>(), size as usize) }.to_vec();
    // SAFETY: descriptor was allocated by the conversion API with LocalAlloc.
    unsafe {
        LocalFree(descriptor);
    }
    Ok(bytes)
}

struct RommWinFsp {
    core: Arc<RommFs>,
    security: Vec<u8>,
}

struct OpenContext {
    inode: u64,
    // Each open owns its own guard, including concurrent opens of one ROM.
    _active: Option<ActiveGuard>,
    directory: DirBuffer,
}

impl RommWinFsp {
    fn resolve(&self, name: &U16CStr) -> winfsp::Result<u64> {
        let name = name
            .to_string()
            .map_err(|_| FspError::NTSTATUS(STATUS_OBJECT_NAME_INVALID))?;
        let parts: Vec<_> = name.split(['\\', '/']).filter(|p| !p.is_empty()).collect();
        self.core
            .inode_for_path(&parts)
            .ok_or(FspError::NTSTATUS(STATUS_OBJECT_NAME_NOT_FOUND))
    }

    fn info(&self, inode: u64) -> winfsp::Result<FileInfo> {
        let meta = self
            .core
            .metadata(inode)
            .ok_or(FspError::NTSTATUS(STATUS_OBJECT_NAME_NOT_FOUND))?;
        Ok(file_info(meta))
    }

    fn copy_security(&self, out: Option<&mut [c_void]>) -> winfsp::Result<u64> {
        if let Some(out) = out {
            if out.len() < self.security.len() {
                return Err(FspError::WIN32(ERROR_INSUFFICIENT_BUFFER));
            }
            // SAFETY: c_void has byte size; out is writable for out.len()
            // bytes and does not overlap the owned descriptor.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    self.security.as_ptr(),
                    out.as_mut_ptr().cast::<u8>(),
                    self.security.len(),
                );
            }
        }
        Ok(self.security.len() as u64)
    }
}

fn file_info(meta: NodeMeta) -> FileInfo {
    FileInfo {
        file_attributes: if meta.kind == EntryKind::Directory {
            FILE_ATTRIBUTE_DIRECTORY
        } else {
            FILE_ATTRIBUTE_READONLY
        },
        file_size: meta.size,
        allocation_size: meta.size.div_ceil(4096) * 4096,
        index_number: meta.inode,
        ..Default::default()
    }
}

impl FileSystemContext for RommWinFsp {
    type FileContext = OpenContext;

    fn get_security_by_name(
        &self,
        name: &U16CStr,
        out: Option<&mut [c_void]>,
        _resolve: impl FnOnce(&U16CStr) -> Option<FileSecurity>,
    ) -> winfsp::Result<FileSecurity> {
        let info = self.info(self.resolve(name)?)?;
        Ok(FileSecurity {
            reparse: false,
            attributes: info.file_attributes,
            sz_security_descriptor: self.copy_security(out)?,
        })
    }

    fn open(
        &self,
        name: &U16CStr,
        options: u32,
        access: u32,
        info: &mut OpenFileInfo,
    ) -> winfsp::Result<OpenContext> {
        // Deny content/metadata writes, delete, ACL changes and delete-on-close.
        if access
            & (FILE_WRITE_DATA
                | FILE_APPEND_DATA
                | FILE_WRITE_EA
                | FILE_WRITE_ATTRIBUTES
                | FILE_DELETE_CHILD
                | DELETE
                | WRITE_DAC
                | WRITE_OWNER)
            != 0
            || options & 0x1000 != 0
        {
            return Err(FspError::NTSTATUS(STATUS_MEDIA_WRITE_PROTECTED)); // media write protected
        }
        let inode = self.resolve(name)?;
        *info.as_mut() = self.info(inode)?;
        // WinFsp needs the catalogue's spelling when callers use another case.
        // Include the root separator and bound the byte length before the
        // binding writes into WinFsp's callback-owned response buffer.
        let path = self
            .core
            .path_of(inode)
            .ok_or(FspError::NTSTATUS(STATUS_OBJECT_NAME_NOT_FOUND))?;
        let normalized: Vec<u16> = format!("\\{}", path.replace('/', "\\"))
            .encode_utf16()
            .collect();
        let bytes = normalized.len() * std::mem::size_of::<u16>();
        if bytes > info.normalized_name_size() as usize
            || bytes >= winfsp::constants::FSP_FSCTL_TRANSACT_RSP_BUFFER_SIZEMAX
        {
            return Err(FspError::NTSTATUS(STATUS_NAME_TOO_LONG));
        }
        info.set_normalized_name(&normalized, None);
        Ok(OpenContext {
            inode,
            _active: self.core.note_open(inode),
            directory: DirBuffer::new(),
        })
    }

    fn close(&self, context: OpenContext) {
        drop(context);
    }

    fn get_file_info(&self, context: &OpenContext, out: &mut FileInfo) -> winfsp::Result<()> {
        *out = self.info(context.inode)?;
        Ok(())
    }

    fn get_security(
        &self,
        _context: &OpenContext,
        out: Option<&mut [c_void]>,
    ) -> winfsp::Result<u64> {
        self.copy_security(out)
    }

    fn read(&self, context: &OpenContext, buffer: &mut [u8], offset: u64) -> winfsp::Result<u32> {
        let n = self
            .core
            .read_at(context.inode, offset, buffer)
            .map_err(|e| {
                tracing::warn!(inode = context.inode, offset, error = %e, "ROM read failed");
                FspError::NTSTATUS(STATUS_IO_DEVICE_ERROR)
            })?;
        if n == 0 && !buffer.is_empty() {
            return Err(FspError::NTSTATUS(STATUS_END_OF_FILE));
        }
        Ok(n as u32)
    }

    fn read_directory(
        &self,
        context: &OpenContext,
        _pattern: Option<&U16CStr>,
        marker: DirMarker,
        buffer: &mut [u8],
    ) -> winfsp::Result<u32> {
        // WinFsp sorts and paginates its directory buffer by Windows markers.
        match context.directory.acquire(marker.is_none(), None) {
            Ok(lock) => {
                if context.inode != ROOT_INODE {
                    let meta = self
                        .core
                        .metadata(context.inode)
                        .ok_or(FspError::NTSTATUS(STATUS_OBJECT_NAME_NOT_FOUND))?;
                    for (name, inode) in [(".", context.inode), ("..", meta.parent)] {
                        let mut entry = DirInfo::<255>::new();
                        entry.set_name_raw(name.encode_utf16().collect::<Vec<_>>().as_slice())?;
                        *entry.file_info_mut() = self.info(inode)?;
                        lock.write(&mut entry)?;
                    }
                }
                let mut cookie = 0;
                loop {
                    let (entries, next, eof) = self
                        .core
                        .read_dir(context.inode, cookie, 512)
                        .ok_or(FspError::NTSTATUS(STATUS_NOT_A_DIRECTORY))?;
                    for child in entries {
                        let mut entry = DirInfo::<255>::new();
                        entry.set_name_raw(
                            child.name.encode_utf16().collect::<Vec<_>>().as_slice(),
                        )?;
                        *entry.file_info_mut() = self.info(child.inode)?;
                        lock.write(&mut entry)?;
                    }
                    if eof {
                        break;
                    }
                    cookie = next;
                }
            }
            // Already populated: acquire returns STATUS_SUCCESS without a lock.
            Err(FspError::NTSTATUS(0)) => {}
            Err(e) => return Err(e),
        }
        Ok(context.directory.read(marker, buffer))
    }

    fn get_volume_info(&self, info: &mut VolumeInfo) -> winfsp::Result<()> {
        info.total_size = 1 << 40;
        info.free_size = 0;
        info.set_volume_label("RomMFS");
        Ok(())
    }
}
