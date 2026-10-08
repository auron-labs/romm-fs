use rommfs_core::events::{AppEvent, EventSink};
use rommfs_core::retrobat::{
    discover_installer, download_installer, launch_installer, InstallerRelease,
};
use rommfs_core::{Error, Result};
use std::path::PathBuf;
use std::process::Child;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct InstallerJob {
    download: Option<DownloadJob>,
    ready: Option<(InstallerRelease, PathBuf)>,
    child: Option<Child>,
}
struct DownloadJob {
    active: Arc<AtomicBool>,
    handle: JoinHandle<()>,
    result: mpsc::Receiver<Result<(InstallerRelease, PathBuf)>>,
}
fn emit(
    sink: &EventSink,
    running: bool,
    ready: bool,
    detail: impl Into<String>,
    error: Option<String>,
) {
    sink.emit(AppEvent::RetroBatInstaller {
        running,
        ready,
        received: 0,
        total: None,
        detail: detail.into(),
        error,
    });
}
impl InstallerJob {
    pub(super) fn is_running(&self) -> bool {
        self.download.is_some() || self.child.is_some()
    }
    pub(super) fn download(&mut self, directory: Option<PathBuf>, sink: &EventSink) {
        if self.is_running() {
            return;
        }
        let Some(directory) = directory else {
            emit(
                sink,
                false,
                false,
                "Installer download unavailable",
                Some("No application data folder is available".into()),
            );
            return;
        };
        self.ready = None;
        emit(
            sink,
            true,
            false,
            "Finding the official stable RetroBat installer",
            None,
        );
        let active = Arc::new(AtomicBool::new(true));
        let flag = active.clone();
        let events = sink.clone();
        let (tx, rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            let result = (|| {
                let release = discover_installer()?;
                let detail = format!("Downloading RetroBat {}", release.version());
                let mut last = Instant::now() - Duration::from_secs(1);
                let path = download_installer(&release, &directory, &flag, |received, total| {
                    if last.elapsed() >= Duration::from_millis(200) {
                        last = Instant::now();
                        events.emit(AppEvent::RetroBatInstaller {
                            running: true,
                            ready: false,
                            received,
                            total: Some(total),
                            detail: detail.clone(),
                            error: None,
                        });
                    }
                })?;
                Ok((release, path))
            })();
            let _ = tx.send(result);
        });
        self.download = Some(DownloadJob {
            active,
            handle,
            result: rx,
        });
    }
    pub(super) fn poll(&mut self, sink: &EventSink) {
        let result = self
            .download
            .as_ref()
            .and_then(|job| match job.result.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(Error::Transport(
                    "Installer download worker stopped unexpectedly".into(),
                ))),
                Err(mpsc::TryRecvError::Empty) => None,
            });
        if let Some(result) = result {
            if let Some(job) = self.download.take() {
                let _ = job.handle.join();
            }
            match result {
                Ok((release, path)) => {
                    sink.emit(AppEvent::log(
                        rommfs_core::events::Level::Info,
                        "retrobat-installer",
                        format!(
                            "Official RetroBat {} package verified: {} bytes, SHA-256 {}",
                            release.version(),
                            release.size(),
                            release.sha256()
                        ),
                    ));
                    emit(sink, false, true, format!("RetroBat {} installer verified. Launch its wizard, then select the fresh installation folder for setup.", release.version()), None);
                    self.ready = Some((release, path));
                }
                Err(Error::Cancelled) => {
                    emit(sink, false, false, "Installer download cancelled", None)
                }
                Err(error) => emit(
                    sink,
                    false,
                    false,
                    "Installer download failed; retry when ready",
                    Some(error.to_string()),
                ),
            }
        }
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.child = None;
                    // A zero exit code is not proof of completed installation: the
                    // wizard can exit after cancellation. Probe the selected folder
                    // through the existing setup workflow instead of guessing.
                    emit(sink, false, self.ready.is_some(), "Installer closed. Select the installed folder and run Set up RetroBat; existing files will be checked before linking.", (!status.success()).then(|| format!("Installer exited with {status}; partial files were left untouched")));
                }
                Ok(None) => {}
                Err(error) => {
                    self.child = None;
                    emit(
                        sink,
                        false,
                        self.ready.is_some(),
                        "Could not inspect installer process; inspect the wizard before retrying",
                        Some(error.to_string()),
                    );
                }
            }
        }
    }
    pub(super) fn launch(&mut self, sink: &EventSink) {
        if self.is_running() {
            return;
        }
        let Some((release, path)) = &self.ready else {
            return;
        };
        emit(
            sink,
            true,
            false,
            "Rechecking installer integrity before launch",
            None,
        );
        match launch_installer(release, path) {
            Ok(child) => {
                self.child = Some(child);
                emit(sink, true, false, "Official installer is running. Use Cancel inside its wizard to cancel installation. RomMFS does not terminate an installer while it writes files.", None);
            }
            Err(error) => emit(
                sink,
                false,
                true,
                "Installer could not be launched",
                Some(error.to_string()),
            ),
        }
    }
    pub(super) fn cancel(&mut self, sink: &EventSink) {
        if let Some(job) = self.download.take() {
            job.active.store(false, Ordering::Release);
            let _ = job.handle.join();
            emit(
                sink,
                false,
                false,
                "Installer download cancelled; partial download removed",
                None,
            );
        } else if self.child.is_some() {
            emit(sink, true, false, "Cancel installation inside the official wizard; partial installation files will not be removed by RomMFS", None);
        }
    }
}
impl Drop for InstallerJob {
    fn drop(&mut self) {
        if let Some(job) = self.download.take() {
            job.active.store(false, Ordering::Release);
            let _ = job.handle.join();
        }
        // Dropping Child leaves the independent wizard running. Never kill an
        // installer (or its prerequisite installers) as part of app shutdown.
    }
}
