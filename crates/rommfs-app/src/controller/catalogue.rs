//! Catalogue loading, projection, persistence, and refresh operations.
use super::*;

impl Worker {
    pub(super) fn project_snapshot(&self, snapshot: &CatalogueSnapshot) -> Result<Catalogue> {
        let systems = match self.preferences.as_ref() {
            Some(preferences) if preferences.integration == IntegrationMode::RetroBat => {
                let root = preferences
                    .retrobat_root
                    .as_deref()
                    .ok_or_else(|| Error::Unsupported("select a RetroBat installation".into()))?;
                Some(rommfs_core::layout::RetroBatSystems::from_installation(
                    root,
                )?)
            }
            _ => None,
        };
        let mut warnings = Vec::new();
        let result = rommfs_core::catalogue::build_catalogue_for_layout(
            self.server_id.as_deref().unwrap_or_default(),
            &snapshot.platforms,
            &snapshot.roms,
            systems.as_ref(),
            |warning| warnings.push(warning),
        );
        emit_catalogue_warnings(&self.sink, warnings);
        result
    }

    pub(super) fn publish_snapshot(
        &mut self,
        snapshot: CatalogueSnapshot,
        cached: bool,
    ) -> Result<Catalogue> {
        let cat = self.project_snapshot(&snapshot)?;
        self.sink.emit(AppEvent::CatalogueLoaded {
            platforms: cat.platforms.len(),
            games: cat.games(),
            files: cat.entries.len(),
            skipped_unsupported: cat.skipped_unsupported,
        });
        self.sink.emit(AppEvent::CatalogueSnapshot {
            refreshed_at: snapshot.refreshed_at,
            cached,
            pending_remount: self
                .fs
                .as_ref()
                .is_some_and(|mounted| !same_projected_tree(mounted.catalogue(), &cat)),
        });
        self.source_snapshot = Some(snapshot);
        Ok(cat)
    }

    pub(super) fn persist_snapshot(&self, snapshot: &CatalogueSnapshot) {
        if let (Some(store), Some(server), Some(account)) = (
            &self.catalogue_store,
            &self.server_id,
            &self.catalogue_account,
        ) {
            if let Err(error) = store.save(server, account, snapshot) {
                self.log(
                    Level::Warn,
                    "catalogue",
                    format!("Catalogue could not be persisted: {error}"),
                );
            }
        }
    }

    pub(super) fn restore_catalogue(&mut self) -> bool {
        let snapshot = match (
            &self.catalogue_store,
            &self.server_id,
            &self.catalogue_account,
        ) {
            (Some(store), Some(server), Some(account)) => store.load(server, account),
            _ => return false,
        };
        match snapshot {
            Ok(Some(snapshot)) => match self.publish_snapshot(snapshot, true) {
                Ok(cat) => {
                    self.store_catalogue(cat);
                    true
                }
                Err(error) => {
                    self.log(Level::Warn, "catalogue", error.to_string());
                    false
                }
            },
            Err(error) => {
                self.log(Level::Warn, "catalogue", error.to_string());
                false
            }
            Ok(None) => false,
        }
    }

    /// At most one refresh in flight. The mounted tree remains immutable; the
    /// complete replacement projection is used only by the next mount.
    pub(super) fn refresh_catalogue(&mut self) {
        if self.refresh_result.is_some() {
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        self.catalogue_operation_id += 1;
        let operation_id = self.catalogue_operation_id;
        let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let sink = self.sink.clone().scoped(active.clone());
        self.refresh_active = Some(active);
        let (tx, rx) = mpsc::channel();
        self.refresh_result = Some(rx);
        let prior = self.source_snapshot.clone();
        self.refresh_handle = Some(std::thread::spawn(move || {
            let result = fetch_catalogue(&client, &sink, operation_id, prior);
            let _ = tx.send((Instant::now(), result));
        }));
    }

    pub(super) fn poll_catalogue_refresh(&mut self) {
        let Some(rx) = &self.refresh_result else {
            return;
        };
        let (finished_at, result) = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => (
                Instant::now(),
                Err(Error::Transport("catalogue refresh worker stopped".into())),
            ),
        };
        // A finished result should be picked up on the next loop turn. A long
        // wait means the loop was blocked; make that visible in user logs.
        let waited = finished_at.elapsed();
        if waited > Duration::from_secs(5) {
            self.log(
                Level::Warn,
                "catalogue",
                format!(
                    "Refresh result waited {}s before the worker polled it",
                    waited.as_secs()
                ),
            );
        }
        self.refresh_result = None;
        self.refresh_active = None;
        // The result has already been sent, so this thread is done.
        if let Some(handle) = self.refresh_handle.take() {
            let _ = handle.join();
        }
        self.finish_catalogue_refresh(result);
    }

    pub(super) fn cancel_catalogue_refresh(&mut self) {
        self.pending_mount_start = None;
        if let Some(active) = self.refresh_active.take() {
            active.store(false, std::sync::atomic::Ordering::Release);
        }
        self.refresh_result = None;
        // Never join here: an in-flight fetch only notices cancellation at its
        // next page, so a join would stall the loop. Reap the handle instead.
        if let Some(handle) = self.refresh_handle.take() {
            self.draining.push(handle);
        }
    }

    /// The real-time fallback: no socket.io credential exists for a bearer-only
    /// session (the server resolves socket identity from the web session cookie
    /// only), so poll an incremental refresh on the session's own timer. Owned
    /// by the connection: cleared on sign-out and drained by
    /// `cancel_catalogue_refresh`.
    pub(super) fn poll_catalogue_refresh_timer(&mut self) {
        let Some(deadline) = self.next_catalogue_poll else {
            return;
        };
        if Instant::now() < deadline {
            return;
        }
        self.next_catalogue_poll = Some(Instant::now() + CATALOGUE_POLL_INTERVAL);
        // A refresh already in flight is left alone; the next poll is measured
        // from now, so the timer cannot stack requests behind a slow refresh.
        if self.refresh_result.is_some() {
            return;
        }
        self.refresh_catalogue();
    }

    pub(super) fn finish_catalogue_refresh(&mut self, result: Result<CatalogueSnapshot>) {
        // The next real-time poll is measured from this completed refresh.
        self.next_catalogue_poll = self
            .next_catalogue_poll
            .map(|_| Instant::now() + CATALOGUE_POLL_INTERVAL);
        // Projection and persistence can be slow on a large library; show the
        // final stage instead of a frozen 100% page bar.
        self.sink.emit(AppEvent::CatalogueProgress {
            operation_id: self.catalogue_operation_id,
            stage: "Finalising library".into(),
            completed: 0,
            total: None,
            elapsed_ms: 0,
        });
        match result {
            Ok(snapshot) => match self.publish_snapshot(snapshot.clone(), false) {
                Ok(cat) => {
                    self.persist_snapshot(&snapshot);
                    self.store_catalogue(cat);
                    if self.mount.is_none() {
                        self.refresh_save_sync();
                    } else {
                        self.queue_metadata();
                    }
                }
                Err(error) => self.catalogue_refresh_failed(error),
            },
            Err(error) => self.catalogue_refresh_failed(error),
        }
        if let Some(path) = self.pending_mount_start.take() {
            self.start_mount(&path);
        }
    }

    pub(super) fn catalogue_refresh_failed(&mut self, error: Error) {
        // Never replace the last successful catalogue with an empty library.
        if error.needs_sign_in() {
            self.stop_save_sync_agent();
            self.stop_mount();
            self.sink.emit(AppEvent::SignInRequired {
                reason: Some(error.to_string()),
            });
        }
        self.sink.emit(AppEvent::CatalogueFailed {
            reason: error.to_string(),
        });
        self.log(
            Level::Error,
            "catalogue",
            format!("Refresh failed; retaining last complete snapshot: {error}"),
        );
        if let Some(path) = self.pending_mount_start.take() {
            self.fail_mount("load catalogue", &path, &error);
        }
    }

    pub(super) fn store_catalogue(&mut self, cat: Catalogue) {
        self.save_sync_catalogue = Some(cat.clone());
        self.names = cat
            .entries
            .iter()
            // content_name is the API-side name; the projected file_name may
            // carry a folder prefix for multi-file ROMs and would 404 here.
            .map(|e| (e.key.clone(), e.content_name.clone()))
            .collect();
        self.catalogue = Some(cat);
    }
}

fn same_projected_tree(a: &Catalogue, b: &Catalogue) -> bool {
    a.platforms == b.platforms
        && a.entries.len() == b.entries.len()
        && a.entries.iter().zip(&b.entries).all(|(a, b)| {
            a.key == b.key
                && a.platform_dir == b.platform_dir
                && a.file_name == b.file_name
                && a.size == b.size
                && a.version == b.version
                && a.content_name == b.content_name
        })
}
