use rommfs_core::events::{AppEvent, EventSink, Level};
use rommfs_core::retrobat::{
    describe_launch_failure, discover_installer, download_installer, launch_installer,
    InstallerRelease,
};
use rommfs_core::{Error, Result};
use std::path::PathBuf;
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
    /// Exit code of the launched wizard, delivered by its waiting thread.
    running: Option<mpsc::Receiver<Result<u32>>>,
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
        self.download.is_some() || self.running.is_some()
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
    /// Returns true when a launched installer just exited with code 0.
    pub(super) fn poll(&mut self, sink: &EventSink) -> bool {
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
                        Level::Info,
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
        let mut exited_ok = false;
        if let Some(rx) = &self.running {
            let outcome = match rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(Error::Transport(
                    "Installer wait thread stopped unexpectedly".into(),
                ))),
                Err(mpsc::TryRecvError::Empty) => None,
            };
            match outcome {
                None => {}
                Some(Ok(code)) => {
                    self.running = None;
                    exited_ok = code == 0;
                    // A zero exit code is not proof of completed installation: the
                    // wizard can exit after cancellation. Probe the selected folder
                    // through the existing setup workflow instead of guessing.
                    emit(sink, false, self.ready.is_some(), "Installer closed. Select the installed folder and run Set up RetroBat; existing files will be checked before linking.", (!exited_ok).then(|| format!("Installer exited with code {code}; partial files were left untouched")));
                }
                Some(Err(error)) => {
                    self.running = None;
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
        exited_ok
    }
    pub(super) fn is_ready(&self) -> bool {
        self.ready.is_some()
    }
    /// Open Explorer with the downloaded installer selected so it can be run by hand.
    pub(super) fn show_file(&self, sink: &EventSink) {
        let Some((_, path)) = &self.ready else {
            return;
        };
        #[cfg(windows)]
        let result = {
            use std::os::windows::process::CommandExt;
            std::process::Command::new("explorer.exe")
                .raw_arg(format!("/select,\"{}\"", path.display()))
                .spawn()
                .map(|_| ())
        };
        #[cfg(not(windows))]
        let result: std::io::Result<()> = Err(std::io::Error::other(format!(
            "showing {} is available on Windows only",
            path.display()
        )));
        if let Err(error) = result {
            emit(
                sink,
                false,
                true,
                "Couldn't open the installer's folder",
                Some(error.to_string()),
            );
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
            Ok(process) => {
                let (tx, rx) = mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(process.wait());
                });
                self.running = Some(rx);
                emit(sink, true, false, "Official installer is running. Use Cancel inside its wizard to cancel installation. RomMFS does not terminate an installer while it writes files.", None);
            }
            Err(error) => {
                let failure = describe_launch_failure(&error);
                if failure.cancelled {
                    sink.emit(AppEvent::log(
                        Level::Info,
                        "retrobat-installer",
                        "Installer launch cancelled at the Windows permission prompt",
                    ));
                    emit(sink, false, true, failure.detail, None);
                } else {
                    sink.emit(AppEvent::log(
                        Level::Error,
                        "retrobat-installer",
                        format!("Installer launch failed: {error}"),
                    ));
                    emit(sink, false, true, failure.detail, failure.explanation);
                }
            }
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
        } else if self.running.is_some() {
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
        // Dropping the wait receiver leaves the independent wizard running. Never kill an
        // installer (or its prerequisite installers) as part of app shutdown.
    }
}
