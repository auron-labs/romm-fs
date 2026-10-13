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
            roms: cat.entries.len(),
            skipped_unsupported: cat.skipped_unsupported,
        });
        self.sink.emit(AppEvent::CatalogueSnapshot {
            refreshed_at: snapshot.refreshed_at,
            cached,
            pending_remount: self.mount.is_some(),
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

    pub(super) fn load_catalogue(&mut self, client: &RommClient) -> Result<Catalogue> {
        self.sink.emit(AppEvent::CatalogueLoading);
        self.catalogue_operation_id += 1;
        let snapshot = fetch_catalogue(client, &self.sink, self.catalogue_operation_id)?;
        let cat = self.publish_snapshot(snapshot.clone(), false)?;
        self.persist_snapshot(&snapshot);
        Ok(cat)
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
        self.refresh_handle = Some(std::thread::spawn(move || {
            let _ = tx.send(fetch_catalogue(&client, &sink, operation_id));
        }));
    }

    pub(super) fn poll_catalogue_refresh(&mut self) {
        let Some(rx) = &self.refresh_result else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err(Error::Transport("catalogue refresh worker stopped".into()))
            }
        };
        self.refresh_result = None;
        self.refresh_active = None;
        if let Some(handle) = self.refresh_handle.take() {
            let _ = handle.join();
        }
        self.finish_catalogue_refresh(result);
    }

    pub(super) fn cancel_catalogue_refresh(&mut self) {
        if let Some(active) = self.refresh_active.take() {
            active.store(false, std::sync::atomic::Ordering::Release);
        }
        self.refresh_result = None;
        if let Some(handle) = self.refresh_handle.take() {
            let _ = handle.join();
        }
    }

    pub(super) fn finish_catalogue_refresh(&mut self, result: Result<CatalogueSnapshot>) {
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
    }

    pub(super) fn store_catalogue(&mut self, cat: Catalogue) {
        self.save_sync_catalogue = Some(cat.clone());
        self.names = cat
            .entries
            .iter()
            .map(|e| (e.key.clone(), e.file_name.clone()))
            .collect();
        self.catalogue = Some(cat);
    }
}
