//! Headless app controller: the real state machine the GPUI window renders.
//! Drives workers via commands, consumes `AppEvent`s into `UiState`.
//! Tests exercise this path — no duplicate state machine in the UI (PRD §6).

use rommfs_core::cache::clock::{Clock, SystemClock, DEFAULT_EVICTION_THRESHOLD_SECS};
use rommfs_core::cache::{CacheIndex, Evictor, LiveState, NoopHydratedRemover};
use rommfs_core::catalog::{build_catalogue, server_id_of, Catalogue, RomKey};
use rommfs_core::download::{ContentSource, DownloadManager};
use rommfs_core::error::{Error, Result};
use rommfs_core::events::{AppEvent, EventSink, Level, LogBuffer};
use rommfs_core::fscore::RommFs;
use rommfs_core::romm::{Credentials, RommClient};
use rommfs_core::tree::RommTree;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnState {
    Idle,
    Connecting,
    Connected,
    Failed,
    SignInRequired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountState {
    NotMounted,
    Mounting,
    Mounted,
    Failed,
}

#[derive(Clone, Debug)]
pub struct DownloadView {
    pub rom_id: u64,
    pub file_name: String,
    pub received: u64,
    pub total: Option<u64>,
    /// None = in flight; Some(Ok) done; Some(Err) terminal failure state.
    pub finished: Option<std::result::Result<(), String>>,
}

/// How many download rows are kept on screen; oldest finished rows are
/// dropped first (the log carries the durable record anyway).
const MAX_DOWNLOAD_ROWS: usize = 128;

/// Everything the window draws — derived ONLY from real worker events.
pub struct UiState {
    pub conn: ConnState,
    pub conn_error: Option<String>,
    pub mount: MountState,
    pub mount_path: Option<String>,
    pub mount_error: Option<String>,
    pub downloads: Vec<DownloadView>,
    pub catalogue: Option<(usize, usize, usize)>, // platforms, roms, skipped
    pub log: LogBuffer,
}

impl UiState {
    pub fn new(log_cap: usize) -> Self {
        Self {
            conn: ConnState::Idle,
            conn_error: None,
            mount: MountState::NotMounted,
            mount_path: None,
            mount_error: None,
            downloads: Vec::new(),
            catalogue: None,
            log: LogBuffer::new(log_cap),
        }
    }

    /// Apply one worker event. This is THE transition table the UI uses.
    pub fn apply(&mut self, event: &AppEvent) {
        match event {
            AppEvent::Connecting => {
                self.conn = ConnState::Connecting;
                self.conn_error = None;
                // A new connection attempt supersedes any prior session's
                // catalogue/downloads — never show stale data for a server
                // we may no longer be talking to.
                self.catalogue = None;
                self.downloads.clear();
            }
            AppEvent::Connected => {
                self.conn = ConnState::Connected;
                self.conn_error = None;
            }
            AppEvent::ConnectFailed { reason } => {
                self.conn = ConnState::Failed;
                self.conn_error = Some(reason.clone());
            }
            AppEvent::SignInRequired => {
                self.conn = ConnState::SignInRequired;
                self.conn_error = Some("sign-in required".to_string());
            }

            AppEvent::CatalogueLoading => {}
            AppEvent::CatalogueLoaded {
                platforms,
                roms,
                skipped_unsupported,
            } => {
                self.catalogue = Some((*platforms, *roms, *skipped_unsupported));
            }
            AppEvent::CatalogueFailed { reason } => {
                // Auth may have succeeded while the catalogue fetch failed:
                // stay Connected but surface the error — a failed catalogue
                // must never render as a successful (empty) library (R1).
                self.conn_error = Some(reason.clone());
            }

            AppEvent::MountStarting { path } => {
                self.mount = MountState::Mounting;
                self.mount_path = Some(path.clone());
                self.mount_error = None;
            }
            AppEvent::MountStarted { path } => {
                self.mount = MountState::Mounted;
                self.mount_path = Some(path.clone());
                self.mount_error = None;
            }
            AppEvent::MountFailed { reason } => {
                self.mount = MountState::Failed;
                self.mount_error = Some(reason.clone());
            }
            AppEvent::MountStopping => {}
            AppEvent::MountStopped => {
                self.mount = MountState::NotMounted;
                // In-flight transfers died with the mount; don't leave rows
                // looking like they are still downloading.
                for d in &mut self.downloads {
                    if d.finished.is_none() {
                        d.finished = Some(Err("mount stopped".to_string()));
                    }
                }
            }

            AppEvent::DownloadStarted {
                rom_id,
                file_name,
                total,
            } => {
                let row = self.download_row(*rom_id, file_name);
                row.received = 0;
                row.total = *total;
                row.finished = None;
            }
            AppEvent::DownloadProgress {
                rom_id,
                received,
                total,
            } => {
                let file_name = String::new();
                let row = self.download_row(*rom_id, &file_name);
                row.received = *received;
                if total.is_some() {
                    row.total = *total;
                }
            }
            AppEvent::DownloadFinished { rom_id, file_name } => {
                let row = self.download_row(*rom_id, file_name);
                row.finished = Some(Ok(()));
            }
            AppEvent::DownloadFailed {
                rom_id,
                file_name,
                reason,
            } => {
                let row = self.download_row(*rom_id, file_name);
                row.finished = Some(Err(reason.clone()));
            }

            AppEvent::Evicted { rom_id, file_name } => {
                self.log.push(log_line(
                    Level::Info,
                    "evict",
                    format!("evicted {file_name} (rom {rom_id})"),
                ));
            }
            AppEvent::EvictFailed { rom_id, reason } => {
                self.log.push(log_line(
                    Level::Error,
                    "evict",
                    format!("eviction failed for rom {rom_id}: {reason}"),
                ));
            }

            AppEvent::Log(line) => self.log.push(line.clone()),
        }
        self.prune_downloads();
    }

    fn download_row(&mut self, rom_id: u64, file_name: &str) -> &mut DownloadView {
        if let Some(pos) = self.downloads.iter().position(|d| d.rom_id == rom_id) {
            let row = &mut self.downloads[pos];
            if !file_name.is_empty() {
                row.file_name = file_name.to_string();
            }
            return row;
        }
        self.downloads.push(DownloadView {
            rom_id,
            file_name: file_name.to_string(),
            received: 0,
            total: None,
            finished: None,
        });
        self.downloads.last_mut().unwrap()
    }

    fn prune_downloads(&mut self) {
        while self.downloads.len() > MAX_DOWNLOAD_ROWS {
            let drop_at = self
                .downloads
                .iter()
                .position(|d| d.finished.is_some())
                .unwrap_or(0);
            self.downloads.remove(drop_at);
        }
    }
}

/// Commands the window sends to the controller.
#[derive(Debug)]
pub enum Command {
    Connect {
        url: String,
        username: String,
        password: String,
    },
    StartMount {
        path: String,
    },
    StopMount,
    Shutdown,
}

/// Owns the worker session (client + catalogue + mount + evict timer).
/// Created on Connect; events flow back over the `EventSink` channel.
pub struct Controller {
    sink: EventSink,
    cmd_tx: mpsc::Sender<Command>,
    // worker thread handle held for shutdown joining
    worker: Option<JoinHandle<()>>,
}

impl Controller {
    /// Spawn the controller + worker loop. Returns (controller, event rx).
    pub fn spawn() -> (Self, mpsc::Receiver<AppEvent>) {
        let (sink, event_rx) = rommfs_core::events::channel();
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();
        let worker_sink = sink.clone();
        let worker = std::thread::Builder::new()
            .name("rommfs-worker".to_string())
            .spawn(move || worker_loop(cmd_rx, worker_sink))
            .expect("spawn worker thread");
        (
            Self {
                sink,
                cmd_tx,
                worker: Some(worker),
            },
            event_rx,
        )
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.cmd_tx.send(cmd);
    }

    pub fn sink(&self) -> EventSink {
        self.sink.clone()
    }
}

impl Drop for Controller {
    /// Closing the app stops the worker and mount (PRD: no background
    /// process survives the window).
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

// ---------------------------------------------------------------------------
// Worker side (all network/filesystem work off the UI thread, PRD R5)
// ---------------------------------------------------------------------------

/// What the worker holds about the active server session. One server, one
/// mount — rebuilt on Connect/StartMount, never shared across servers.
struct Worker {
    sink: EventSink,
    client: Option<Arc<RommClient>>,
    server_id: Option<String>,
    /// Latest built catalogue; consumed into `RommTree` on mount.
    catalogue: Option<Catalogue>,
    /// ROM file names by key (content URL + log/evict context).
    names: HashMap<RomKey, String>,
    fs: Option<Arc<RommFs>>,
    mount: Option<ActiveMount>,
}

impl Worker {
    fn new(sink: EventSink) -> Self {
        Self {
            sink,
            client: None,
            server_id: None,
            catalogue: None,
            names: HashMap::new(),
            fs: None,
            mount: None,
        }
    }

    fn log(&self, level: Level, op: &'static str, message: impl Into<String>) {
        self.sink.emit(AppEvent::log(level, op, message));
    }

    fn connect(&mut self, url: &str, username: &str, password: &str) {
        // Credentials are used to authenticate and then dropped from this
        // stack frame; they are never stored in logs/events (R1).
        self.sink.emit(AppEvent::Connecting);
        if self.mount.is_some() {
            self.fail_connect(&Error::Unsupported(
                "stop the mount before connecting elsewhere".into(),
            ));
            return;
        }

        let client = match RommClient::new(url) {
            Ok(c) => Arc::new(c),
            Err(e) => {
                self.fail_connect(&e);
                return;
            }
        };
        if let Err(e) = client.authenticate(Credentials { username, password }) {
            self.fail_connect(&e);
            return;
        }

        self.client = Some(Arc::clone(&client));
        self.server_id = Some(server_id_of(url));
        self.sink.emit(AppEvent::Connected);
        self.log(Level::Info, "connect", format!("connected as {username}"));

        // The catalogue primes the UI (counts) but a failure here must not
        // fake an empty library — it surfaces as CatalogueFailed (R1).
        match self.load_catalogue(&client) {
            Ok(cat) => self.store_catalogue(cat),
            Err(e) => {
                if e.needs_sign_in() {
                    self.sink.emit(AppEvent::SignInRequired);
                } else {
                    self.sink.emit(AppEvent::CatalogueFailed {
                        reason: e.to_string(),
                    });
                }
                self.log(Level::Error, "catalogue", e.to_string());
            }
        }
    }

    /// Fetch platforms + all ROM pages and build the catalogue.
    /// `stop/start reloads it` (R1) — called on Connect and on every
    /// StartMount so the mounted snapshot is fresh.
    fn load_catalogue(&mut self, client: &RommClient) -> Result<Catalogue> {
        self.sink.emit(AppEvent::CatalogueLoading);
        let platforms = client.platforms()?;
        let roms = client.all_roms()?;
        let server_id = self.server_id.clone().unwrap_or_default();
        let sink = self.sink.clone();
        let cat = build_catalogue(&server_id, &platforms, &roms, move |warning| {
            sink.emit(AppEvent::log(Level::Warn, "catalogue", warning));
        })?;
        self.sink.emit(AppEvent::CatalogueLoaded {
            platforms: cat.platforms.len(),
            roms: cat.entries.len(),
            skipped_unsupported: cat.skipped_unsupported,
        });
        self.log(
            Level::Info,
            "catalogue",
            format!(
                "catalogue loaded: {} platforms, {} roms, {} skipped",
                cat.platforms.len(),
                cat.entries.len(),
                cat.skipped_unsupported
            ),
        );
        Ok(cat)
    }

    fn store_catalogue(&mut self, cat: Catalogue) {
        self.names = cat
            .entries
            .iter()
            .map(|e| (e.key.clone(), e.file_name.clone()))
            .collect();
        self.catalogue = Some(cat);
    }

    fn fail_connect(&self, e: &Error) {
        if e.needs_sign_in() {
            self.sink.emit(AppEvent::SignInRequired);
        } else {
            self.sink.emit(AppEvent::ConnectFailed {
                reason: e.to_string(),
            });
        }
        self.log(Level::Error, "connect", e.to_string());
    }

    fn fail_mount(&self, e: &Error) {
        self.sink.emit(AppEvent::MountFailed {
            reason: e.to_string(),
        });
        self.log(Level::Error, "mount", e.to_string());
    }

    fn start_mount(&mut self, path: &str) {
        let path = path.trim();
        if self.mount.is_some() {
            self.fail_mount(&Error::Unsupported("already mounted".into()));
            return;
        }
        let Some(client) = self.client.clone() else {
            self.fail_mount(&Error::Auth(
                "connect to a RomM server before mounting".into(),
            ));
            return;
        };
        let Some(server_id) = self.server_id.clone() else {
            self.fail_mount(&Error::Auth(
                "connect to a RomM server before mounting".into(),
            ));
            return;
        };

        self.sink.emit(AppEvent::MountStarting {
            path: path.to_string(),
        });
        let root = PathBuf::from(path);

        // Validate the root BEFORE claiming it: only empty dirs or roots we
        // previously marked for THIS server (PRD §5 — never over an existing
        // ROM library, never recursively cleared).
        if let Err(e) = check_mount_root(&root, &server_id) {
            self.fail_mount(&e);
            return;
        }

        // Mounts re-read the catalogue so stop/start never serves stale data.
        match self.load_catalogue(&client) {
            Ok(cat) => self.store_catalogue(cat),
            Err(e) if e.needs_sign_in() => {
                self.sink.emit(AppEvent::SignInRequired);
                self.fail_mount(&e);
                return;
            }
            Err(e) => {
                self.sink.emit(AppEvent::CatalogueFailed {
                    reason: e.to_string(),
                });
                self.fail_mount(&e);
                return;
            }
        }

        if let Err(e) = claim_mount_root(&root, &server_id) {
            self.fail_mount(&e);
            return;
        }

        let fs = match self.build_fs(&client, &root) {
            Ok(fs) => fs,
            Err(e) => {
                self.fail_mount(&e);
                return;
            }
        };

        match start_mount_backend(Arc::clone(&fs), &root) {
            Ok(mount) => {
                let mounted_path = mount.root.display().to_string();
                self.mount = Some(mount);
                self.fs = Some(fs);
                self.sink
                    .emit(AppEvent::MountStarted { path: mounted_path });
                self.log(Level::Info, "mount", format!("mounted at {path}"));
                // One immediate sweep so expired entries from previous runs
                // are reclaimed (PRD R4); per-ROM outcomes become events.
                self.evict_once();
            }
            Err(e) => self.fail_mount(&e),
        }
    }

    /// Assemble the portable core objects the platform adapter mounts.
    fn build_fs(&mut self, client: &Arc<RommClient>, _root: &Path) -> Result<Arc<RommFs>> {
        let server_id = self.server_id.clone().unwrap_or_default();
        let index = CacheIndex::open(cache_dir_for(&server_id))?;
        let live = Arc::new(LiveState::default());
        let source = Arc::new(ClientSource {
            client: Arc::clone(client),
            names: self.names.clone(),
        });
        let cat = self.catalogue.take();
        let cat = match cat {
            Some(c) => c,
            None => return Err(Error::InvalidCatalogue("no catalogue loaded".into())),
        };
        let expected: HashMap<RomKey, u64> = cat
            .entries
            .iter()
            .map(|e| (e.key.clone(), e.size))
            .collect();
        let versions: HashMap<RomKey, Option<String>> = cat
            .entries
            .iter()
            .map(|e| (e.key.clone(), e.version.as_ref().map(|v| v.0.clone())))
            .collect();
        let downloads = Arc::new(DownloadManager::new(
            index,
            Arc::clone(&live),
            source,
            self.sink.clone(),
            expected,
            versions,
        ));
        // NOTE: with the mount backend unlinked (see `start_mount_backend`)
        // the hydrated remover is the portable no-op; once rommfs-fsk is a
        // dependency this becomes `ProjfsRemover` built from the mount's
        // captured `ProjfsHandle` so eviction also removes ProjFS copies.
        let evictor = Evictor::new(
            DEFAULT_EVICTION_THRESHOLD_SECS,
            live,
            Arc::new(NoopHydratedRemover),
        );
        let tree = RommTree::new(cat);
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        Ok(Arc::new(RommFs::new(tree, downloads, evictor, clock)))
    }

    fn evict_once(&mut self) {
        let Some(fs) = self.fs.clone() else { return };
        match fs.evict_stale() {
            Ok(outcome) => {
                for key in outcome.evicted {
                    let name = self
                        .names
                        .get(&key)
                        .cloned()
                        .unwrap_or_else(|| key.cache_stem());
                    self.sink.emit(AppEvent::Evicted {
                        rom_id: key.rom_id.max(0) as u64,
                        file_name: name,
                    });
                }
                for key in outcome.deferred_failed {
                    self.sink.emit(AppEvent::EvictFailed {
                        rom_id: key.rom_id.max(0) as u64,
                        reason: "cleanup deferred (busy or removal failed)".into(),
                    });
                }
                if !outcome.retained_active.is_empty() {
                    self.log(
                        Level::Info,
                        "evict",
                        format!(
                            "eviction retained {} in-use entr{}",
                            outcome.retained_active.len(),
                            if outcome.retained_active.len() == 1 {
                                "y"
                            } else {
                                "ies"
                            }
                        ),
                    );
                }
            }
            Err(e) => {
                self.log(Level::Error, "evict", e.to_string());
            }
        }
    }

    fn stop_mount(&mut self) {
        self.sink.emit(AppEvent::MountStopping);
        if let Some(mount) = self.mount.take() {
            mount.stop();
            self.log(Level::Info, "mount", "mount stopped".to_string());
        }
        self.fs = None;
        self.sink.emit(AppEvent::MountStopped);
    }
}

fn worker_loop(rx: mpsc::Receiver<Command>, sink: EventSink) {
    let mut worker = Worker::new(sink);
    while let Ok(cmd) = rx.recv() {
        let shutdown = matches!(cmd, Command::Shutdown);
        // Commands run one at a time on the worker thread; UI stays
        // responsive while network/filesystem work blocks here.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match &cmd {
            Command::Connect {
                url,
                username,
                password,
            } => worker.connect(url, username, password),
            Command::StartMount { path } => worker.start_mount(path),
            Command::StopMount => worker.stop_mount(),
            Command::Shutdown => worker.stop_mount(),
        }));
        if let Err(payload) = result {
            // Mid-flight a sibling crate's todo!() (or any panic) must
            // surface as a visible failure, not a dead worker (PRD: errors
            // become real AppEvents). The message is a code location, never
            // a credential.
            let msg = panic_message(&payload);
            worker.log(Level::Error, "worker", format!("operation panicked: {msg}"));
            match &cmd {
                Command::Connect { .. } => worker.sink.emit(AppEvent::ConnectFailed {
                    reason: format!("internal error: {msg}"),
                }),
                Command::StartMount { .. } => worker.sink.emit(AppEvent::MountFailed {
                    reason: format!("internal error: {msg}"),
                }),
                Command::StopMount => {}
                Command::Shutdown => {}
            }
        }
        if shutdown {
            break;
        }
    }
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

// ---------------------------------------------------------------------------
// Download wiring: RomM client -> ContentSource for the DownloadManager.
// ---------------------------------------------------------------------------

struct ClientSource {
    client: Arc<RommClient>,
    /// `RomKey -> content_name` (the `file_name` used in the content URL).
    names: HashMap<RomKey, String>,
}

impl ContentSource for ClientSource {
    fn fetch(
        &self,
        key: &RomKey,
        writer: &mut dyn std::io::Write,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<u64> {
        let name = self.names.get(key).ok_or_else(|| {
            Error::InvalidCatalogue(format!("rom {} not in catalogue", key.rom_id))
        })?;
        self.client
            .download_file(key.rom_id, name, writer, progress)
    }
}

// ---------------------------------------------------------------------------
// Mount root + backend seam.
// ---------------------------------------------------------------------------

/// Marker file written into managed roots so re-mounts are recognized as
/// app-owned (mirrors the adapter contract in `crates/rommfs-fsk`; keep the
/// names identical so switching to `rommfs_fsk::{check_mount_root,
/// claim_mount_root}` is a drop-in change).
const ROOT_MARKER: &str = ".rommfs-root";

/// `root` may be mounted when it is an empty directory or a directory we
/// previously claimed for this exact `server_id`.
fn check_mount_root(root: &Path, server_id: &str) -> Result<()> {
    if !root.exists() {
        return Err(Error::Unsupported(format!(
            "mount root {} does not exist",
            root.display()
        )));
    }
    if !root.is_dir() {
        return Err(Error::Unsupported(format!(
            "mount root {} is not a directory",
            root.display()
        )));
    }
    let marker = root.join(ROOT_MARKER);
    if marker.is_file() {
        let owner = std::fs::read_to_string(&marker).unwrap_or_default();
        if owner.trim() == server_id {
            return Ok(());
        }
        return Err(Error::Unsupported(format!(
            "{} is a managed root for a different server",
            root.display()
        )));
    }
    let mut entries = std::fs::read_dir(root)?;
    if entries.next().is_none() {
        return Ok(());
    }
    Err(Error::Unsupported(format!(
        "{} is not empty and is not a RomMFS-managed root",
        root.display()
    )))
}

/// Claim `root` for this server by writing the marker file (idempotent).
fn claim_mount_root(root: &Path, server_id: &str) -> Result<()> {
    std::fs::write(root.join(ROOT_MARKER), server_id)?;
    Ok(())
}

/// A live mount: opaque handle whose `stop` releases the provider.
struct ActiveMount {
    root: PathBuf,
    stop_fn: Box<dyn FnOnce() + Send>,
}

impl ActiveMount {
    fn stop(self) {
        (self.stop_fn)();
    }
}

/// Start the platform mount over `fs` at `root`.
///
/// INTEGRATION SEAM — `rommfs-app` does not declare `rommfs-fsk` in its
/// Cargo.toml (manifests may not be edited), so the adapter type cannot be
/// named here and the call currently fails with `Unsupported` — surfaced to
/// the UI as `MountFailed`. Once `rommfs-fsk = { workspace = true }` is added
/// to `crates/rommfs-app/Cargo.toml`, this becomes:
///
/// ```rust,ignore
/// let (mount, _handle) = rommfs_fsk::WindowsMount::mount(fs, root)?;
/// let root = root.to_path_buf();
/// Ok(ActiveMount { root, stop_fn: Box::new(move || mount.stop()) })
/// ```
///
/// and `build_fs` should then construct the `Evictor` with
/// `ProjfsRemover::new(root.clone(), handle)` instead of `NoopHydratedRemover`
/// so eviction also removes the ProjFS-hydrated copies (PRD R4).
fn start_mount_backend(_fs: Arc<RommFs>, _root: &Path) -> Result<ActiveMount> {
    Err(Error::Unsupported(
        "mount backend not linked: add the rommfs-fsk dependency to rommfs-app \
         (see start_mount_backend in controller.rs)"
            .into(),
    ))
}

/// Private cache location, outside the projected tree (PRD §5) and scoped
/// per server so different servers never share entries (R4).
fn cache_dir_for(server_id: &str) -> PathBuf {
    let safe: String = server_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("rommfs").join("cache").join(if safe.is_empty() {
        "default".into()
    } else {
        safe
    })
}

fn log_line(level: Level, op: &'static str, message: String) -> rommfs_core::events::LogLine {
    rommfs_core::events::LogLine {
        unix_secs: rommfs_core::cache::now_unix_secs(),
        level,
        op,
        message,
    }
}
