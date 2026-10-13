use crate::root::{self, Manifest, OwnedEntry, OwnedKind};
use crate::security;
use anyhow::Context;
use rommfs_core::cache::HydratedRemover;
use rommfs_core::filesystem::RommFs;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::mem::{offset_of, size_of};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::Duration;
use windows_sys::Win32::Foundation::{
    ERROR_CLOUD_FILE_NOT_UNDER_SYNC_ROOT, ERROR_INVALID_FUNCTION, STATUS_CLOUD_FILE_ACCESS_DENIED,
    STATUS_CLOUD_FILE_REQUEST_ABORTED, STATUS_CLOUD_FILE_UNSUCCESSFUL,
};
use windows_sys::Win32::Storage::CloudFilters::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;

fn wide(path: &Path) -> anyhow::Result<Vec<u16>> {
    let text: Vec<_> = path.as_os_str().encode_wide().collect();
    anyhow::ensure!(!text.contains(&0), "path contains NUL");
    Ok(text.into_iter().chain(Some(0)).collect())
}

fn hr(result: i32, action: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        result >= 0,
        "{action} failed (HRESULT 0x{:08x})",
        result as u32
    );
    Ok(())
}

/// Check the built-in Cloud Files platform and the chosen NTFS volume.
/// Does not elevate, enable optional components, or install a driver.
pub fn check_prerequisites(root: &Path) -> anyhow::Result<()> {
    let canonical = std::fs::canonicalize(root)
        .with_context(|| format!("resolve mount directory {}", root.display()))?;
    anyhow::ensure!(
        matches!(canonical.components().next(), Some(std::path::Component::Prefix(prefix))
        if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_))),
        "Cloud Files requires a local drive, not a network share"
    );
    let mut platform = CF_PLATFORM_INFO::default();
    // SAFETY: platform is writable for the duration of the call.
    hr(
        unsafe { CfGetPlatformInfo(&mut platform) },
        "Windows Cloud Files platform (Windows 10 1709+ required)",
    )?;
    let directory = open_metadata(root, false)?;
    let mut name = [0u16; 32];
    // SAFETY: directory is live and name is a writable buffer.
    if unsafe {
        GetVolumeInformationByHandleW(
            directory.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            name.as_mut_ptr(),
            name.len() as u32,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error()).context("inspect mount volume");
    }
    let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    anyhow::ensure!(
        String::from_utf16_lossy(&name[..end]) == "NTFS",
        "Cloud Files requires a local NTFS mount directory"
    );
    Ok(())
}

fn open_metadata(path: &Path, exclusive: bool) -> anyhow::Result<File> {
    OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES | READ_CONTROL | WRITE_DAC)
        .share_mode(if exclusive {
            0
        } else {
            FILE_SHARE_READ | FILE_SHARE_WRITE
        })
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .with_context(|| format!("open {} for file attributes and ACL access (FILE_READ_ATTRIBUTES | READ_CONTROL | WRITE_DAC, exclusive={exclusive})", path.display()))
}

pub(crate) fn check_root_directory(root: &Path) -> anyhow::Result<()> {
    let file = open_metadata(root, false)?;
    anyhow::ensure!(file.metadata()?.is_dir(), "mount root must be a directory");
    let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: live handle to the final object, correctly sized writable output.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileAttributeTagInfo,
            (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    if info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        // SAFETY: matching FileAttributeTagInfo structure.
        let state = unsafe {
            CfGetPlaceholderStateFromFileInfo(
                (&info as *const FILE_ATTRIBUTE_TAG_INFO).cast(),
                FileAttributeTagInfo,
            )
        };
        anyhow::ensure!(
            state != CF_PLACEHOLDER_STATE_INVALID && state & CF_PLACEHOLDER_STATE_SYNC_ROOT != 0,
            "mount root is a link or an unsupported reparse point"
        );
        let manifest = root::read_manifest(root)?;
        registration_flags(root, &format!("RomMFS-CFAPI-v1:{}", manifest.server_id))?;
    }
    Ok(())
}

fn placeholder_info(file: &File) -> anyhow::Result<(CF_PLACEHOLDER_STANDARD_INFO, Vec<u8>)> {
    let mut buffer = vec![0u64; (size_of::<CF_PLACEHOLDER_STANDARD_INFO>() + 4096).div_ceil(8)];
    let mut returned = 0;
    // SAFETY: aligned allocation is large enough for the header and maximum identity.
    hr(
        unsafe {
            CfGetPlaceholderInfo(
                file.as_raw_handle(),
                CF_PLACEHOLDER_INFO_STANDARD,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * 8) as u32,
                &mut returned,
            )
        },
        "inspect owned placeholder",
    )?;
    let start = offset_of!(CF_PLACEHOLDER_STANDARD_INFO, FileIdentity);
    anyhow::ensure!(
        returned as usize >= size_of::<CF_PLACEHOLDER_STANDARD_INFO>(),
        "truncated placeholder metadata"
    );
    // SAFETY: returned size covers the complete header in aligned storage.
    let info = unsafe { *buffer.as_ptr().cast::<CF_PLACEHOLDER_STANDARD_INFO>() };
    let end = start
        .checked_add(info.FileIdentityLength as usize)
        .context("invalid identity length")?;
    anyhow::ensure!(
        end <= returned as usize && end <= buffer.len() * 8,
        "truncated placeholder identity"
    );
    // SAFETY: bounds checked above.
    let identity = unsafe {
        std::slice::from_raw_parts(
            buffer.as_ptr().cast::<u8>().add(start),
            info.FileIdentityLength as usize,
        )
    }
    .to_vec();
    Ok((info, identity))
}

fn verify_handle(file: &File, entry: &OwnedEntry) -> anyhow::Result<()> {
    let meta = file.metadata()?;
    if entry.is_local() {
        anyhow::ensure!(
            !rommfs_core::save_sync::path_is_reparse_point(&meta)
                && meta.is_dir() == entry.directory
                && (entry.directory || meta.is_file()),
            "ordinary frontend entry changed type or is redirected: {}",
            entry.path
        );
        return Ok(());
    }
    anyhow::ensure!(
        meta.is_dir() == entry.directory && (entry.directory || meta.len() == entry.size),
        "owned entry changed type or size: {}",
        entry.path
    );
    let (info, identity) = placeholder_info(file)?;
    anyhow::ensure!(
        identity == entry.identity
            && info.ModifiedDataSize == 0
            && info.InSyncState == CF_IN_SYNC_STATE_IN_SYNC,
        "{} is not an unmodified RomMFS placeholder; preserve it and choose a fresh root",
        entry.path
    );
    Ok(())
}

pub(crate) fn verify_owned(path: &Path, entry: &OwnedEntry) -> anyhow::Result<()> {
    verify_handle(&open_metadata(path, false)?, entry)
}

pub(crate) fn make_directory_writable(path: &Path) -> anyhow::Result<()> {
    security::set(&open_metadata(path, false)?, security::MIXED_DIRECTORY)
}

pub(crate) fn make_local_writable(path: &Path) -> anyhow::Result<()> {
    security::set(&open_metadata(path, false)?, security::LOCAL_WRITABLE)
}

/// Unlike generic save paths, the managed root may itself be a verified CFAPI
/// sync-root reparse point. Permit that root only; pin ordinary child directories
/// without FILE_SHARE_DELETE while local metadata is being published.
pub(crate) fn hold_local_directory_chain(
    root: &Path,
    directory: &Path,
) -> anyhow::Result<Vec<File>> {
    let open_directory = |path: &Path| -> anyhow::Result<File> {
        Ok(OpenOptions::new()
            // List access, not attributes alone, pins the namespace on
            // Windows Server too. Omit delete sharing to prevent substitution.
            .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?)
    };
    let mut handles = vec![open_directory(root)?];
    // Validate after pinning so a root swapped between check/open is refused.
    check_root_directory(root)?;
    let mut path = root.to_path_buf();
    for part in directory.strip_prefix(root)?.components() {
        anyhow::ensure!(
            matches!(part, std::path::Component::Normal(_)),
            "invalid metadata directory path"
        );
        path.push(part);
        let file = open_directory(&path)?;
        let metadata = file.metadata()?;
        anyhow::ensure!(
            metadata.is_dir() && !rommfs_core::save_sync::path_is_reparse_point(&metadata),
            "redirected metadata directory was preserved"
        );
        handles.push(file);
    }
    Ok(handles)
}

/// Exclusive share-nothing handles prevent dehydration while readers are open.
/// Native dehydration failures retain the private cache for a later sweep.
pub struct WindowsHydratedRemover {
    root: PathBuf,
}
impl WindowsHydratedRemover {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }
}
impl HydratedRemover for WindowsHydratedRemover {
    fn remove_hydrated(&self, relative: &str) -> std::io::Result<()> {
        let result = (|| -> anyhow::Result<()> {
            let parts: Vec<_> = relative.split('/').collect();
            anyhow::ensure!(parts.len() >= 2, "invalid cache path");
            for part in parts {
                rommfs_core::save_sync::validate_windows_path_component(part)
                    .map_err(anyhow::Error::msg)?;
            }
            let manifest = root::read_manifest(&self.root)?;
            let entry = manifest
                .entries
                .iter()
                .find(|e| e.path == relative && !e.directory && !e.is_local());
            let path = self.root.join(relative);
            // A missing placeholder already has no hydrated bytes to reclaim.
            let file = match OpenOptions::new()
                .access_mode(FILE_READ_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | WRITE_DAC)
                .share_mode(0)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&path)
            {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(e) => return Err(e.into()),
            };
            let entry = entry.context("cache path has no owned placeholder")?;
            verify_handle(&file, entry)?;
            // SAFETY: exclusive live handle; synchronous operation; no partial deletion.
            hr(
                unsafe {
                    CfDehydratePlaceholder(
                        file.as_raw_handle(),
                        0,
                        -1,
                        CF_DEHYDRATE_FLAG_NONE,
                        std::ptr::null_mut(),
                    )
                },
                "dehydrate ROM",
            )
        })();
        result.map_err(std::io::Error::other)
    }
}

struct CallbackContext {
    core: Arc<RommFs>,
    inodes: HashMap<Vec<u8>, u64>,
    stopping: AtomicBool,
    callbacks: Mutex<usize>,
    idle: Condvar,
}
struct CallbackGuard<'a>(&'a CallbackContext);
impl Drop for CallbackGuard<'_> {
    fn drop(&mut self) {
        let mut count = self.0.callbacks.lock().unwrap_or_else(|e| e.into_inner());
        *count -= 1;
        self.0.idle.notify_all();
    }
}
impl CallbackContext {
    fn enter(&self) -> CallbackGuard<'_> {
        *self.callbacks.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        CallbackGuard(self)
    }
    fn inode(&self, info: &CF_CALLBACK_INFO) -> Option<u64> {
        if info.FileIdentity.is_null() || info.FileIdentityLength != 32 {
            return None;
        }
        // SAFETY: CFAPI owns identity bytes until this callback returns; length is bounded.
        let identity = unsafe { std::slice::from_raw_parts(info.FileIdentity.cast::<u8>(), 32) };
        self.inodes.get(identity).copied()
    }
}

/// One registered, connected NTFS sync root. No WinFsp/ProjFS runtime is used.
pub struct WindowsMount {
    connection: Option<CF_CONNECTION_KEY>,
    registered: bool,
    context: Box<CallbackContext>,
    root: PathBuf,
    manifest: Manifest,
    _lock: File,
    _root_handle: File,
    _directories: rommfs_core::save_sync::SaveDirectoryChainGuard,
}

impl WindowsMount {
    pub fn mount(core: Arc<RommFs>, root: impl AsRef<Path>) -> anyhow::Result<Self> {
        let started = std::time::Instant::now();
        core.events()
            .emit(rommfs_core::events::AppEvent::MountProgress {
                stage: "Root preparation".into(),
                completed: 0,
                total: None,
                elapsed_ms: 0,
            });
        root::check_directory(root.as_ref())?;
        let root = std::fs::canonicalize(root.as_ref())?;
        check_prerequisites(&root)?;
        let lock_path = root::sidecar_path(&root, ".rommfs-lock")?;
        std::fs::create_dir_all(lock_path.parent().context("missing parent")?)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .open(&lock_path)
            .with_context(|| {
                format!(
                    "open exclusive mount lock {}; another session may hold it",
                    lock_path.display()
                )
            })?;
        let directories = rommfs_core::save_sync::hold_save_directory_chain(
            root.parent().context("missing root parent")?,
        )?;
        let mut previous = root::read_manifest(&root).context("claim the root before mounting")?;
        // Adopt the files a frontend legitimately wrote between mounts; report
        // every other foreign entry at once instead of failing on the first.
        let scan = root::scan_tree(&root, &previous, verify_owned)?;
        let unowned = root::adopt_entries(&root, &mut previous, scan.unowned)?;
        if !unowned.is_empty() {
            return Err(anyhow::Error::new(root::UnownedEntries(unowned)));
        }
        anyhow::ensure!(
            scan.failures.is_empty(),
            "owned entries failed verification: {}",
            scan.failures.join("; ")
        );
        let mut manifest = manifest_for(&core, &previous)?;
        let root_handle = open_metadata(&root, false)?;
        manifest.root_security = if previous.root_security.is_empty() {
            security::snapshot(&root_handle)
                .with_context(|| format!("read original ACL for {}", root.display()))?
        } else {
            previous.root_security.clone()
        };
        // Persist the original ACL before protecting the root, including crash recovery.
        previous.root_security = manifest.root_security.clone();
        root::write_manifest(&root, &previous)?;
        let inodes = manifest
            .entries
            .iter()
            .filter(|entry| !entry.is_local() && !entry.directory)
            .map(|entry| {
                let parts: Vec<_> = entry.path.split('/').collect();
                let inode = core
                    .inode_for_path(&parts)
                    .expect("manifest constructed from core tree");
                (entry.identity.clone(), inode)
            })
            .collect();
        let mut mount = Self {
            connection: None,
            registered: false,
            context: Box::new(CallbackContext {
                core,
                inodes,
                stopping: AtomicBool::new(false),
                callbacks: Mutex::new(0),
                idle: Condvar::new(),
            }),
            root,
            manifest: previous,
            _lock: lock,
            _root_handle: root_handle.try_clone()?,
            _directories: directories,
        };
        register_root(&mount.root, &manifest.server_id)?;
        mount.registered = true;
        security::set(&root_handle, security::MIXED_DIRECTORY).with_context(|| {
            format!(
                "apply protected mixed-content directory ACL to {}",
                mount.root.display()
            )
        })?;
        let scan = root::scan_tree(&mount.root, &mount.manifest, verify_owned)?;
        let unowned = root::adopt_entries(&mount.root, &mut mount.manifest, scan.unowned)?;
        // ponytail: rebuilds owned placeholders on start; reconcile unchanged entries
        // if catalogue startup costs become significant. Private bytes survive.
        let mut failures = scan.failures;
        failures.extend(remove_entries(&mount.root, &mount.manifest));
        if !unowned.is_empty() {
            failures.push(format!("{}", root::UnownedEntries(unowned)));
        }
        anyhow::ensure!(failures.is_empty(), "{}", failures.join("; "));
        root::write_manifest(&mount.root, &manifest)?;
        mount.manifest = manifest;
        let callbacks = [
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_FETCH_DATA,
                Callback: Some(fetch_data),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NOTIFY_FILE_OPEN_COMPLETION,
                Callback: Some(opened),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NOTIFY_DELETE,
                Callback: Some(deny_delete),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NOTIFY_RENAME,
                Callback: Some(deny_rename),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NONE,
                Callback: None,
            },
        ];
        let path = wide(&mount.root)?;
        let mut connection = 0;
        // SAFETY: context allocation stays fixed until disconnect and callbacks drain.
        // CFAPI copies the terminated registration table during this call.
        hr(
            unsafe {
                CfConnectSyncRoot(
                    path.as_ptr(),
                    callbacks.as_ptr(),
                    (&*mount.context as *const CallbackContext).cast(),
                    CF_CONNECT_FLAG_REQUIRE_PROCESS_INFO,
                    &mut connection,
                )
            },
            "connect Cloud Files sync root",
        )?;
        mount.connection = Some(connection);
        let total = mount.manifest.entries.len() as u64;
        mount
            .context
            .core
            .events()
            .emit(rommfs_core::events::AppEvent::MountProgress {
                stage: "Placeholder creation".into(),
                completed: 0,
                total: Some(total),
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
        for (index, entry) in mount.manifest.entries.iter().enumerate() {
            create_placeholder(&mount.root, entry).with_context(|| {
                format!(
                    "create placeholder {}",
                    mount.root.join(&entry.path).display()
                )
            })?;
            if (index + 1) % 64 == 0 || index + 1 == total as usize {
                mount
                    .context
                    .core
                    .events()
                    .emit(rommfs_core::events::AppEvent::MountProgress {
                        stage: "Placeholder creation".into(),
                        completed: (index + 1) as u64,
                        total: Some(total),
                        elapsed_ms: started.elapsed().as_millis() as u64,
                    });
            }
        }
        Ok(mount)
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
        self.context.stopping.store(true, Ordering::Release);
        self.context.core.cancel_transfers();
        if let Some(connection) = self.connection.take() {
            // SAFETY: context remains allocated until callbacks have drained.
            let result = unsafe { CfDisconnectSyncRoot(connection) };
            if result < 0 {
                // A live native connection must never outlive its callback context.
                tracing::error!(
                    result,
                    "Cloud Files disconnect failed; terminating to avoid dangling callbacks"
                );
                std::process::abort();
            }
        }
        let mut count = self
            .context
            .callbacks
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        while *count != 0 {
            count = self
                .context
                .idle
                .wait(count)
                .unwrap_or_else(|e| e.into_inner());
        }
        drop(count);
        if !self.registered {
            return;
        }
        let cleanup = (|| -> anyhow::Result<()> {
            let latest = root::read_manifest(&self.root)?;
            anyhow::ensure!(
                latest.server_id == self.manifest.server_id,
                "root ownership changed while mounted"
            );
            self.manifest = latest;
            let scan = root::scan_tree(&self.root, &self.manifest, verify_owned)?;
            // A frontend may have written its own files (gamelist.xml) while
            // mounted; claim those so everything RomMFS owns can still come
            // down. Unknown foreign entries stay untouched and reported.
            let unowned = root::adopt_entries(&self.root, &mut self.manifest, scan.unowned)?;
            let mut problems = scan.failures;
            problems.extend(remove_entries(&self.root, &self.manifest));
            let leftovers = self
                .manifest
                .entries
                .iter()
                .filter(|entry| {
                    !entry.is_local()
                        && std::fs::symlink_metadata(self.root.join(&entry.path)).is_ok()
                })
                .count();
            if leftovers != 0 {
                problems.push(format!(
                    "{leftovers} owned placeholders could not be removed"
                ));
            }
            if !unowned.is_empty() {
                problems.push(format!("{}", root::UnownedEntries(unowned)));
            }
            anyhow::ensure!(
                problems.is_empty(),
                "partial cleanup; registration preserved for remount: {}",
                problems.join("; ")
            );
            let path = wide(&self.root)?;
            // SAFETY: no connection or placeholders remain; never traverse user files.
            hr(
                unsafe { CfUnregisterSyncRoot(path.as_ptr()) },
                "unregister sync root without ROM placeholders",
            )?;
            security::restore(
                &open_metadata(&self.root, false)?,
                &self.manifest.root_security,
            )?;
            root::write_manifest(
                &self.root,
                &Manifest {
                    server_id: self.manifest.server_id.clone(),
                    entries: self
                        .manifest
                        .entries
                        .iter()
                        .filter(|entry| entry.is_local())
                        .cloned()
                        .collect(),
                    ..Manifest::default()
                },
            )
        })();
        if let Err(error) = cleanup {
            self.context
                .core
                .events()
                .emit(rommfs_core::events::AppEvent::log(
                    rommfs_core::events::Level::Warn,
                    "native-stop",
                    format!(
                        "Cloud Files cleanup deferred at {}: {error:#}",
                        self.root.display()
                    ),
                ));
            tracing::warn!(error = %error, root = %self.root.display(), "Cloud Files cleanup deferred; owned placeholders preserved for restart");
        }
    }
}

fn manifest_for(core: &RommFs, previous: &Manifest) -> anyhow::Result<Manifest> {
    let server = &previous.server_id;
    let catalogue = core.catalogue();
    anyhow::ensure!(
        catalogue.entries.iter().all(|e| e.key.server_id == *server),
        "catalogue belongs to a different server"
    );
    // Ordinary frontend content survives projection changes and remounts.
    // Retain its ancestor directories, including removed platforms, so history
    // is not lost when a game disappears from the server.
    let mut entries: Vec<_> = previous
        .entries
        .iter()
        .filter(|e| e.is_local() || e.directory)
        .cloned()
        .map(|mut entry| {
            if entry.directory {
                entry.kind = OwnedKind::ManagedDirectory;
            }
            entry
        })
        .collect();
    for directory in catalogue.directories() {
        if entries
            .iter()
            .any(|entry| entry.path.eq_ignore_ascii_case(directory))
        {
            continue;
        }
        entries.push(OwnedEntry {
            path: directory.clone(),
            identity: Sha256::digest(format!("RomMFS-dir:{server}:{directory}")).to_vec(),
            directory: true,
            size: 0,
            kind: OwnedKind::ManagedDirectory,
        });
    }
    for entry in &catalogue.entries {
        anyhow::ensure!(
            entry.size <= i64::MAX as u64,
            "ROM size exceeds Cloud Files limit"
        );
        let path = format!("{}/{}", entry.platform_dir, entry.file_name);
        anyhow::ensure!(
            !entries.iter().any(|e| e.path.eq_ignore_ascii_case(&path)),
            "ROM projection collides with persistent frontend content: {path}"
        );
        let identity = Sha256::digest(serde_json::to_vec(&(
            server,
            entry.key.rom_id,
            entry.key.file_id,
            entry.version.as_ref().map(|v| &v.0),
            entry.size,
            &path,
        ))?)
        .to_vec();
        entries.push(OwnedEntry {
            path,
            identity,
            directory: false,
            size: entry.size,
            kind: OwnedKind::Placeholder,
        });
    }
    Ok(Manifest {
        server_id: server.clone(),
        entries,
        ..Manifest::default()
    })
}

fn registration_flags(root: &Path, identity: &str) -> anyhow::Result<CF_REGISTER_FLAGS> {
    let path = wide(root)?;
    let mut buffer = vec![0u64; (size_of::<CF_SYNC_ROOT_STANDARD_INFO>() + 65536).div_ceil(8)];
    let mut returned = 0;
    // SAFETY: writable, aligned buffer and terminated path.
    let existing = unsafe {
        CfGetSyncRootInfoByPath(
            path.as_ptr(),
            CF_SYNC_ROOT_INFO_STANDARD,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * 8) as u32,
            &mut returned,
        )
    };
    let mut flags = CF_REGISTER_FLAG_DISABLE_ON_DEMAND_POPULATION_ON_ROOT
        | CF_REGISTER_FLAG_MARK_IN_SYNC_ON_ROOT;
    if existing >= 0 {
        anyhow::ensure!(
            returned as usize >= size_of::<CF_SYNC_ROOT_STANDARD_INFO>(),
            "truncated sync root metadata"
        );
        // SAFETY: checked header size in aligned storage.
        let info = unsafe { *buffer.as_ptr().cast::<CF_SYNC_ROOT_STANDARD_INFO>() };
        let start = offset_of!(CF_SYNC_ROOT_STANDARD_INFO, SyncRootIdentity);
        let end = start + info.SyncRootIdentityLength as usize;
        anyhow::ensure!(
            end <= returned as usize && end <= buffer.len() * 8,
            "truncated sync root identity"
        );
        // SAFETY: checked bounds above.
        let stored = unsafe {
            std::slice::from_raw_parts(
                buffer.as_ptr().cast::<u8>().add(start),
                info.SyncRootIdentityLength as usize,
            )
        };
        anyhow::ensure!(
            stored == identity.as_bytes(),
            "directory is already managed by another sync provider"
        );
        let file = open_metadata(root, false)?;
        let mut file_info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: live handle and writable output.
        anyhow::ensure!(
            unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut file_info) } != 0,
            "cannot inspect root file ID"
        );
        let file_id = ((file_info.nFileIndexHigh as u64) << 32) | file_info.nFileIndexLow as u64;
        anyhow::ensure!(
            file_id == info.SyncRootFileId as u64,
            "directory is nested inside another sync root"
        );
        flags |= CF_REGISTER_FLAG_UPDATE;
    } else {
        // Windows Server reports ERROR_INVALID_FUNCTION where client Windows
        // reports ERROR_CLOUD_FILE_NOT_UNDER_SYNC_ROOT for a directory that is
        // not registered; both mean "not under a sync root".
        let not_registered = (0x80070000u32 | ERROR_CLOUD_FILE_NOT_UNDER_SYNC_ROOT) as i32;
        let server_not_registered = (0x80070000u32 | ERROR_INVALID_FUNCTION) as i32;
        if existing != not_registered && existing != server_not_registered {
            hr(existing, "inspect existing sync root")?;
        }
    }
    Ok(flags)
}

fn register_root(root: &Path, server: &str) -> anyhow::Result<()> {
    let path = wide(root)?;
    let identity = format!("RomMFS-CFAPI-v1:{server}");
    let flags = registration_flags(root, &identity)?;
    let name: Vec<u16> = "RomMFS\0".encode_utf16().collect();
    let version: Vec<u16> = "0.1.0\0".encode_utf16().collect();
    let registration = CF_SYNC_REGISTRATION {
        StructSize: size_of::<CF_SYNC_REGISTRATION>() as u32,
        ProviderName: name.as_ptr(),
        ProviderVersion: version.as_ptr(),
        SyncRootIdentity: identity.as_ptr().cast(),
        SyncRootIdentityLength: identity.len() as u32,
        ..CF_SYNC_REGISTRATION::default()
    };
    let policies = CF_SYNC_POLICIES {
        StructSize: size_of::<CF_SYNC_POLICIES>() as u32,
        Hydration: CF_HYDRATION_POLICY {
            Primary: CF_HYDRATION_POLICY_FULL,
            Modifier: CF_HYDRATION_POLICY_MODIFIER_NONE,
        },
        Population: CF_POPULATION_POLICY {
            Primary: CF_POPULATION_POLICY_ALWAYS_FULL,
            Modifier: CF_POPULATION_POLICY_MODIFIER_NONE,
        },
        InSync: CF_INSYNC_POLICY_NONE,
        HardLink: CF_HARDLINK_POLICY_NONE,
        PlaceholderManagement: CF_PLACEHOLDER_MANAGEMENT_POLICY_DEFAULT,
    };
    // SAFETY: all buffers/structures remain valid through this synchronous call.
    hr(
        unsafe { CfRegisterSyncRoot(path.as_ptr(), &registration, &policies, flags) },
        "register Cloud Files sync root",
    )
}

fn create_placeholder(root: &Path, entry: &OwnedEntry) -> anyhow::Result<()> {
    let target = root.join(&entry.path);
    if entry.directory && entry.kind == OwnedKind::ManagedDirectory {
        if !target.try_exists()? {
            std::fs::create_dir(&target)?;
        }
        verify_owned(&target, entry)?;
        return make_directory_writable(&target);
    }
    if entry.is_local() {
        return Ok(());
    }
    if entry.directory && target.try_exists()? {
        verify_owned(&target, entry)?;
        return make_directory_writable(&target);
    }
    let parent = wide(target.parent().context("missing parent")?)?;
    let name = wide(Path::new(target.file_name().context("missing filename")?))?;
    let mut info = CF_PLACEHOLDER_CREATE_INFO {
        RelativeFileName: name.as_ptr(),
        FsMetadata: CF_FS_METADATA {
            BasicInfo: FILE_BASIC_INFO {
                FileAttributes: if entry.directory {
                    FILE_ATTRIBUTE_DIRECTORY
                } else {
                    FILE_ATTRIBUTE_NORMAL
                },
                ..FILE_BASIC_INFO::default()
            },
            FileSize: entry.size as i64,
        },
        FileIdentity: entry.identity.as_ptr().cast(),
        FileIdentityLength: entry.identity.len() as u32,
        Flags: CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC
            | if entry.directory {
                CF_PLACEHOLDER_CREATE_FLAG_DISABLE_ON_DEMAND_POPULATION
            } else {
                0
            },
        ..CF_PLACEHOLDER_CREATE_INFO::default()
    };
    let mut processed = 0;
    // SAFETY: valid structures, buffers, identity and output for this call.
    hr(
        unsafe {
            CfCreatePlaceholders(
                parent.as_ptr(),
                &mut info,
                1,
                CF_CREATE_FLAG_STOP_ON_ERROR,
                &mut processed,
            )
        },
        "create placeholder",
    )?;
    anyhow::ensure!(processed == 1, "placeholder was not processed");
    hr(info.Result, "create placeholder entry")?;
    // Apply explicitly as well as inheriting from the protected root.
    security::set(
        &open_metadata(&target, false)?,
        if entry.directory {
            security::MIXED_DIRECTORY
        } else {
            security::READ_ONLY
        },
    )
}

/// Delete each verified owned entry, collecting failures instead of aborting
/// on the first so one blocked file cannot strand the rest of the tree. Every
/// entry is still re-verified before deleting; foreign content is never
/// touched, and directories holding foreign files stay because they are
/// nonempty, not because they are trusted.
fn remove_entries(root: &Path, manifest: &Manifest) -> Vec<String> {
    let _parent_guard = match root
        .parent()
        .context("mount root has no parent")
        .and_then(|parent| {
            rommfs_core::save_sync::hold_save_directory_chain(parent).map_err(anyhow::Error::from)
        }) {
        Ok(guard) => guard,
        Err(e) => return vec![format!("protect mount parent: {e:#}")],
    };
    let mut failures = Vec::new();
    for entry in manifest.entries.iter().rev() {
        if entry.is_local() && !entry.directory {
            continue;
        }
        let path = root.join(&entry.path);
        if std::fs::symlink_metadata(&path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        {
            continue;
        }
        let _directory_guard = match hold_local_directory_chain(
            root,
            path.parent()
                .expect("validated manifest entry has a parent"),
        ) {
            Ok(guard) => guard,
            Err(e) => {
                failures.push(format!("{}: {e:#}", entry.path));
                continue;
            }
        };
        let file = match OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES | READ_CONTROL | WRITE_DAC)
            .share_mode(FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(&path)
        {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                failures.push(format!("{}: {e}", entry.path));
                continue;
            }
        };
        if let Err(e) = verify_handle(&file, entry) {
            failures.push(format!("{}: {e:#}", entry.path));
            continue;
        }
        if entry.directory {
            match std::fs::read_dir(&path) {
                Ok(mut entries) => {
                    if entries.next().is_some() {
                        continue;
                    }
                }
                Err(e) => {
                    failures.push(format!("{}: {e}", entry.path));
                    continue;
                }
            }
        }
        if let Err(e) = security::allow_delete(&file) {
            failures.push(format!("{}: {e:#}", entry.path));
            continue;
        }
        let result = (|| -> anyhow::Result<()> {
            let deleting = OpenOptions::new()
                .access_mode(DELETE | FILE_READ_ATTRIBUTES | WRITE_DAC)
                .share_mode(0)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
                .open(&path)?;
            verify_handle(&deleting, entry)?;
            let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
            // SAFETY: exclusive handle to verified app data; nonempty dirs fail.
            if unsafe {
                SetFileInformationByHandle(
                    deleting.as_raw_handle(),
                    FileDispositionInfo,
                    (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                    size_of::<FILE_DISPOSITION_INFO>() as u32,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            Ok(())
        })();
        // Keep the first handle until permissions are restored on any failed delete.
        let restored = security::set(
            &file,
            if entry.directory {
                security::MIXED_DIRECTORY
            } else {
                security::READ_ONLY
            },
        );
        if let Err(error) = result {
            let message = match restored.context("restore ROM permissions after failed cleanup") {
                Ok(()) => format!("{}: {error:#}", entry.path),
                Err(e) => format!("{}: {error:#}; {e:#}", entry.path),
            };
            failures.push(message);
        }
    }
    failures
}

fn operation(info: &CF_CALLBACK_INFO, kind: CF_OPERATION_TYPE) -> CF_OPERATION_INFO {
    CF_OPERATION_INFO {
        StructSize: size_of::<CF_OPERATION_INFO>() as u32,
        Type: kind,
        ConnectionKey: info.ConnectionKey,
        TransferKey: info.TransferKey,
        CorrelationVector: info.CorrelationVector,
        RequestKey: info.RequestKey,
        ..CF_OPERATION_INFO::default()
    }
}

fn transfer(
    info: &CF_CALLBACK_INFO,
    offset: i64,
    length: i64,
    data: *const std::ffi::c_void,
    status: i32,
) -> anyhow::Result<()> {
    let op = operation(info, CF_OPERATION_TYPE_TRANSFER_DATA);
    let mut params = CF_OPERATION_PARAMETERS {
        ParamSize: (offset_of!(CF_OPERATION_PARAMETERS, Anonymous)
            + size_of::<CF_OPERATION_PARAMETERS_0_0>()) as u32,
        Anonymous: CF_OPERATION_PARAMETERS_0 {
            TransferData: CF_OPERATION_PARAMETERS_0_0 {
                Flags: CF_OPERATION_TRANSFER_DATA_FLAG_NONE,
                CompletionStatus: status,
                Buffer: data,
                Offset: offset,
                Length: length,
            },
        },
    };
    // SAFETY: callback keys are live; on success data covers length bytes through call.
    hr(unsafe { CfExecute(&op, &mut params) }, "transfer ROM data")
}

unsafe extern "system" fn fetch_data(
    info: *const CF_CALLBACK_INFO,
    params: *const CF_CALLBACK_PARAMETERS,
) {
    // SAFETY: CFAPI supplies callback-owned structures and our pinned context.
    let (info, params, context) = unsafe {
        (
            &*info,
            &*params,
            &*((*info).CallbackContext.cast::<CallbackContext>()),
        )
    };
    let _guard = context.enter();
    // SAFETY: FETCH_DATA is registered only for this function.
    let request = unsafe { params.Anonymous.FetchData };
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> anyhow::Result<()> {
            anyhow::ensure!(
                !context.stopping.load(Ordering::Acquire),
                "provider stopping"
            );
            let inode = context
                .inode(info)
                .context("unknown placeholder identity")?;
            let size = context.core.metadata(inode).context("missing inode")?.size;
            anyhow::ensure!(
                request.RequiredFileOffset >= 0
                    && request.RequiredFileOffset as u64 <= size
                    && request.RequiredLength >= -1,
                "invalid fetch range"
            );
            let _active = context.core.note_open(inode).context("not a ROM")?;
            let progress = AtomicI64::new(0);
            let (done, receiver) = mpsc::channel();
            let connection = info.ConnectionKey;
            let key = info.TransferKey;
            let progress = &progress;
            std::thread::scope(move |scope| {
                scope.spawn(move || {
                    loop {
                        // SAFETY: opaque keys are valid until this callback returns.
                        unsafe {
                            CfReportProviderProgress(
                                connection,
                                key,
                                size as i64,
                                progress.load(Ordering::Relaxed),
                            );
                        }
                        if receiver.recv_timeout(Duration::from_secs(10))
                            != Err(mpsc::RecvTimeoutError::Timeout)
                        {
                            break;
                        }
                    }
                });
                let result = (|| -> anyhow::Result<()> {
                    let mut buffer = vec![0u8; 1024 * 1024];
                    let mut offset = 0;
                    while offset < size {
                        anyhow::ensure!(
                            !context.stopping.load(Ordering::Acquire),
                            "provider stopping"
                        );
                        let want = buffer.len().min((size - offset) as usize);
                        let read = context.core.read_at(inode, offset, &mut buffer[..want])?;
                        anyhow::ensure!(read == want, "short read of verified cached ROM");
                        transfer(info, offset as i64, read as i64, buffer.as_ptr().cast(), 0)?;
                        offset += read as u64;
                        progress.store(offset as i64, Ordering::Relaxed);
                    }
                    Ok(())
                })();
                let _ = done.send(());
                result
            })
        }));
    if !matches!(result, Ok(Ok(()))) {
        match &result {
            Ok(Err(error)) => tracing::warn!(error = %error, "ROM hydration failed"),
            Err(_) => tracing::error!("ROM hydration callback panicked"),
            _ => {}
        }
        let status = if context.stopping.load(Ordering::Acquire) {
            STATUS_CLOUD_FILE_REQUEST_ABORTED
        } else {
            STATUS_CLOUD_FILE_UNSUCCESSFUL
        };
        let _ = transfer(
            info,
            request.RequiredFileOffset.max(0),
            if request.RequiredLength == -1 {
                (info.FileSize - request.RequiredFileOffset.max(0)).max(0)
            } else {
                request.RequiredLength.max(0)
            },
            std::ptr::null(),
            status,
        );
    }
}

unsafe extern "system" fn opened(
    info: *const CF_CALLBACK_INFO,
    _params: *const CF_CALLBACK_PARAMETERS,
) {
    // SAFETY: callback-owned info and pinned context; no pointers escape callback.
    let (info, context) = unsafe {
        (
            &*info,
            &*((*info).CallbackContext.cast::<CallbackContext>()),
        )
    };
    let _guard = context.enter();
    // CFAPI supplies ProcessInfo because REQUIRE_PROCESS_INFO was requested.
    // Provider metadata opens must not reenter the index during an eviction.
    if info.ProcessInfo.is_null() {
        return;
    }
    // SAFETY: callback-owned ProcessInfo; GetCurrentProcessId has no preconditions.
    if unsafe { (*info.ProcessInfo).ProcessId == GetCurrentProcessId() } {
        return;
    }
    if let Some(inode) = context.inode(info) {
        drop(context.core.note_open(inode));
    }
}

unsafe extern "system" fn deny_delete(
    info: *const CF_CALLBACK_INFO,
    _: *const CF_CALLBACK_PARAMETERS,
) {
    // SAFETY: callback-owned info is used synchronously.
    let info = unsafe { &*info };
    // SAFETY: the connection owns this context until every callback has drained.
    let context = unsafe { &*info.CallbackContext.cast::<CallbackContext>() };
    let _guard = context.enter();
    let op = operation(info, CF_OPERATION_TYPE_ACK_DELETE);
    let mut params = CF_OPERATION_PARAMETERS {
        ParamSize: (offset_of!(CF_OPERATION_PARAMETERS, Anonymous)
            + size_of::<CF_OPERATION_PARAMETERS_0_7>()) as u32,
        Anonymous: CF_OPERATION_PARAMETERS_0 {
            AckDelete: CF_OPERATION_PARAMETERS_0_7 {
                Flags: CF_OPERATION_ACK_DELETE_FLAG_NONE,
                // Ordinary frontend files carry no placeholder identity and
                // may be atomically replaced. ROMs remain denied; directory
                // namespace protection also comes from their DACL.
                CompletionStatus: if info.FileIdentityLength == 0 {
                    0
                } else {
                    STATUS_CLOUD_FILE_ACCESS_DENIED
                },
            },
        },
    };
    // SAFETY: all operation parameters are initialized and keys are live.
    unsafe {
        CfExecute(&op, &mut params);
    }
}
unsafe extern "system" fn deny_rename(
    info: *const CF_CALLBACK_INFO,
    _: *const CF_CALLBACK_PARAMETERS,
) {
    // SAFETY: callback-owned info is used synchronously.
    let info = unsafe { &*info };
    // SAFETY: the connection owns this context until every callback has drained.
    let context = unsafe { &*info.CallbackContext.cast::<CallbackContext>() };
    let _guard = context.enter();
    let op = operation(info, CF_OPERATION_TYPE_ACK_RENAME);
    let mut params = CF_OPERATION_PARAMETERS {
        ParamSize: (offset_of!(CF_OPERATION_PARAMETERS, Anonymous)
            + size_of::<CF_OPERATION_PARAMETERS_0_6>()) as u32,
        Anonymous: CF_OPERATION_PARAMETERS_0 {
            AckRename: CF_OPERATION_PARAMETERS_0_6 {
                Flags: CF_OPERATION_ACK_RENAME_FLAG_NONE,
                CompletionStatus: if info.FileIdentityLength == 0 {
                    0
                } else {
                    STATUS_CLOUD_FILE_ACCESS_DENIED
                },
            },
        },
    };
    // SAFETY: all operation parameters are initialized and keys are live.
    unsafe {
        CfExecute(&op, &mut params);
    }
}
