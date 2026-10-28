use super::*;
use rommfs_core::events::AuxiliarySummary;
use rommfs_core::retrobat::AuxiliaryReport;
use std::sync::atomic::{AtomicBool, Ordering};

/// A materialization job belongs to the current account/session, not a view.
pub(super) struct AuxiliaryJob {
    active: Arc<AtomicBool>,
    handle: JoinHandle<()>,
    result: mpsc::Receiver<anyhow::Result<AuxiliaryReport>>,
}

impl Worker {
    pub(super) fn materialize_auxiliary(&mut self) {
        if self.auxiliary_job.is_some() {
            return;
        }
        let result = (|| -> anyhow::Result<AuxiliaryJob> {
            anyhow::ensure!(
                self.mount.is_none()
                    && self.mount_preparation.is_none()
                    && self.pending_mount_start.is_none(),
                "Stop the games folder before installing support files"
            );
            anyhow::ensure!(
                !self.installer.is_running(),
                "Finish the RetroBat installation before installing support files"
            );
            let installation = self
                .preferences
                .as_ref()
                .and_then(|p| p.retrobat_root.clone())
                .context("Choose a RetroBat installation and set it up first")?;
            let client = self
                .client
                .clone()
                .context("Connect to RomM before installing support files")?;
            let source = self
                .source_snapshot
                .clone()
                .context("Load the catalogue first")?;
            let server = self.server_id.clone().context("Connect first")?;
            let account = self
                .save_sync_identity
                .as_ref()
                .context("Sign in to RomM before installing support files")?
                .account_id;
            let active = Arc::new(AtomicBool::new(true));
            let flag = active.clone();
            let sink = self.sink.clone().scoped(flag.clone());
            let (tx, rx) = mpsc::channel();
            let handle = std::thread::spawn(move || {
                let result = (|| -> anyhow::Result<AuxiliaryReport> {
                    if !flag.load(Ordering::Acquire) {
                        anyhow::bail!("support file install cancelled");
                    }
                    Ok(rommfs_core::retrobat::materialize_auxiliaries(
                        &client,
                        &server,
                        account,
                        &installation,
                        &source.platforms,
                        &source.roms,
                        &flag,
                        |completed, total| {
                            sink.emit(AppEvent::AuxiliaryStatus {
                                running: true,
                                detail: format!("Support files: {completed} / {total}"),
                                error: None,
                                summary: None,
                            });
                        },
                    )?)
                })();
                let _ = tx.send(result);
            });
            Ok(AuxiliaryJob {
                active,
                handle,
                result: rx,
            })
        })();
        match result {
            Ok(job) => {
                self.auxiliary_job = Some(job);
                self.sink.emit(AppEvent::AuxiliaryStatus {
                    running: true,
                    detail: "Checking support file destinations and ownership".into(),
                    error: None,
                    summary: None,
                });
            }
            Err(error) => self.auxiliary_failed(error),
        }
    }
    pub(super) fn poll_auxiliary(&mut self) {
        let result = self
            .auxiliary_job
            .as_ref()
            .and_then(|job| match job.result.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(anyhow::anyhow!(
                    "Auxiliary worker stopped unexpectedly"
                ))),
            });
        if let Some(result) = result {
            if let Some(job) = self.auxiliary_job.take() {
                let _ = job.handle.join();
            }
            match result {
                Ok(report) => {
                    for warning in &report.warnings {
                        self.log(Level::Warn, "auxiliary", &warning.message);
                    }
                    let issues = report.warnings.iter().filter(|w| w.is_issue()).count();
                    self.sink.emit(AppEvent::AuxiliaryStatus {
                        running: false,
                        detail: format!(
                            "Support files: {} installed, {} unchanged, {} need attention",
                            report.published, report.unchanged, issues
                        ),
                        error: None,
                        summary: Some(AuxiliarySummary {
                            published: report.published,
                            unchanged: report.unchanged,
                            warnings: report.warnings,
                        }),
                    });
                }
                Err(error) => self.auxiliary_failed(error),
            }
        }
    }
    pub(super) fn cancel_auxiliary(&mut self) {
        if let Some(job) = self.auxiliary_job.take() {
            job.active.store(false, Ordering::Release);
            self.draining.push(job.handle);
            self.sink.emit(AppEvent::AuxiliaryStatus { running:false,detail:"Support file install cancelled; completed files were kept and partial files were not published".into(),error:None,summary:None });
        }
    }
    fn auxiliary_failed(&self, error: anyhow::Error) {
        self.sink.emit(AppEvent::AuxiliaryStatus {
            running: false,
            detail: "Support files weren't installed; existing files were left alone".into(),
            error: Some(format!("{error:#}")),
            summary: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_invalidates_late_events_and_drains_the_account_owned_job() {
        let (sink, events) = rommfs_core::events::channel();
        let mut worker = Worker::new(sink.clone());
        let active = Arc::new(AtomicBool::new(true));
        let flag = active.clone();
        let scoped = sink.scoped(flag.clone());
        let (tx, rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            while flag.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            scoped.emit(AppEvent::AuxiliaryStatus {
                running: true,
                detail: "stale progress".into(),
                error: None,
                summary: None,
            });
            let _ = tx.send(Ok(AuxiliaryReport::default()));
        });
        worker.auxiliary_job = Some(AuxiliaryJob {
            active: active.clone(),
            handle,
            result: rx,
        });
        worker.clear_active_connection();
        assert!(!active.load(Ordering::Acquire));
        assert!(worker.auxiliary_job.is_none());
        let events: Vec<_> = events.try_iter().collect();
        assert!(events
            .iter()
            .any(|e| matches!(e, AppEvent::AuxiliaryStatus { running: false, .. })));
        assert!(!events
            .iter()
            .any(|e| matches!(e, AppEvent::AuxiliaryStatus { running: true, .. })));
        let deadline = Instant::now() + Duration::from_secs(2);
        while !worker.draining.iter().all(|handle| handle.is_finished())
            && Instant::now() < deadline
        {
            std::thread::yield_now();
        }
        worker.reap_draining();
        assert!(worker.draining.is_empty());
    }
}
