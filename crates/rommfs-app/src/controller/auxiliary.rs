use super::*;
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
                self.mount.is_none(),
                "Stop the mount before materializing auxiliary files"
            );
            anyhow::ensure!(
                !self.installer.is_running(),
                "Finish installation before materializing auxiliary files"
            );
            let installation = self
                .preferences
                .as_ref()
                .and_then(|p| p.retrobat_root.clone())
                .context("Select and set up a fresh RetroBat installation first")?;
            let client = self
                .client
                .clone()
                .context("Connect before materializing auxiliary files")?;
            let source = self
                .source_snapshot
                .clone()
                .context("Load the catalogue first")?;
            let server = self.server_id.clone().context("Connect first")?;
            let account = self
                .save_sync_identity
                .as_ref()
                .context("Verify the account before materializing files")?
                .account_id;
            let active = Arc::new(AtomicBool::new(true));
            let flag = active.clone();
            let sink = self.sink.clone().scoped(flag.clone());
            let (tx, rx) = mpsc::channel();
            let handle = std::thread::spawn(move || {
                let result = (|| -> anyhow::Result<AuxiliaryReport> {
                    let _stopped = stopped_retrobat_guard(&installation)?;
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
                                detail: format!("Auxiliary files: {completed} / {total}"),
                                error: None,
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
                    detail: "Checking auxiliary destinations and ownership".into(),
                    error: None,
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
                        self.log(Level::Warn, "auxiliary", warning);
                    }
                    self.sink.emit(AppEvent::AuxiliaryStatus { running:false,detail:format!("Auxiliary files: {} published, {} unchanged, {} issues. See Activity for unsupported or preserved files.",report.published,report.unchanged,report.warnings.len()),error:None });
                }
                Err(error) => self.auxiliary_failed(error),
            }
        }
    }
    pub(super) fn cancel_auxiliary(&mut self) {
        if let Some(job) = self.auxiliary_job.take() {
            job.active.store(false, Ordering::Release);
            let _ = job.handle.join();
            self.sink.emit(AppEvent::AuxiliaryStatus { running:false,detail:"Auxiliary download cancelled; completed files remain owned and partial files were not published".into(),error:None });
        }
    }
    fn auxiliary_failed(&self, error: anyhow::Error) {
        self.sink.emit(AppEvent::AuxiliaryStatus {
            running: false,
            detail: "Auxiliary setup failed; existing files were preserved".into(),
            error: Some(format!("{error:#}")),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_invalidates_late_events_and_joins_the_account_owned_job() {
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
    }
}
