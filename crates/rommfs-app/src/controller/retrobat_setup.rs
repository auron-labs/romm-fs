//! RetroBat integration setup and its metadata publication lifecycle.
use super::*;

impl Worker {
    pub(super) fn emit_integration(&self, error: Option<String>) {
        let preferences = self.preferences.as_ref();
        let setup_complete = preferences.is_some_and(|p| {
            match (
                p.integration == IntegrationMode::RetroBat,
                p.retrobat_root.as_deref(),
                p.mount_root.as_deref(),
            ) {
                (true, Some(install), Some(managed)) => rommfs_core::retrobat::verify_setup_link(
                    install,
                    managed,
                    &server_id_of(&p.server),
                )
                .is_ok(),
                _ => false,
            }
        });
        self.sink.emit(AppEvent::IntegrationUpdated {
            setup_complete,
            mode: preferences.map(|p| p.integration).unwrap_or_default(),
            retrobat_root: preferences
                .and_then(|p| p.retrobat_root.as_ref())
                .map(|p| p.display().to_string()),
            error,
        });
    }

    pub(super) fn metadata_status(
        &self,
        pending: bool,
        detail: impl Into<String>,
        error: Option<String>,
    ) {
        emit_metadata_status(&self.sink, pending, detail, error);
    }

    pub(super) fn cancel_metadata(&mut self) {
        if let Some(active) = self.metadata_active.take() {
            active.store(false, std::sync::atomic::Ordering::Release);
        }
        self.metadata_job = None;
        // Never join here: an in-flight artwork request only notices
        // cancellation while writing a body, so a join would stall the worker
        // loop for a whole request timeout. The handle is reaped instead.
        if let Some(handle) = self.metadata_handle.take() {
            self.draining.push(handle);
        }
        if let Some(handle) = self.metadata_publish_handle.take() {
            self.draining.push(handle);
        }
        self.metadata_publish = None;
        self.metadata_ready = None;
        self.metadata_source = None;
        self.metadata_wait_reported = false;
    }

    pub(super) fn queue_metadata(&mut self) {
        self.cancel_metadata();
        let Some(preferences) = self
            .preferences
            .as_ref()
            .filter(|p| p.integration == IntegrationMode::RetroBat)
        else {
            return;
        };
        let (Some(fs), Some(snapshot), Some(client)) =
            (&self.fs, &self.source_snapshot, &self.client)
        else {
            return;
        };
        let catalogue = fs.catalogue().clone();
        // A refreshed structure is staged for the next mount. Never generate
        // launch paths for files that are absent from this immutable native tree.
        if self.catalogue.as_ref().is_some_and(|next| {
            next.entries.len() != catalogue.entries.len()
                || next.entries.iter().zip(&catalogue.entries).any(|(a, b)| {
                    a.key != b.key
                        || a.platform_dir != b.platform_dir
                        || a.file_name != b.file_name
                        || next.is_launchable(&a.key) != catalogue.is_launchable(&b.key)
                })
        }) {
            self.metadata_status(
                true,
                "Frontend metadata retained; library changes apply on the next remount",
                None,
            );
            return;
        }
        let snapshot = snapshot.clone();
        let roms = snapshot.roms.clone();
        let client = client.clone();
        let cache = cache_dir_for(self.server_id.as_deref().unwrap_or_default())
            .join("artwork")
            .join(preferences.account_id.to_string());
        let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
        self.metadata_active = Some(active.clone());
        self.metadata_source = Some((catalogue.clone(), snapshot));
        // Publish gamelists straight away rather than waiting for artwork.
        // Entries keep the artwork they already show, so this never strips
        // images; a second publication adds new artwork once it is cached.
        self.metadata_ready = Some(rommfs_core::retrobat::ArtworkBatch::default());
        let (tx, rx) = mpsc::channel();
        self.metadata_job = Some(rx);
        self.metadata_status(
            true,
            "Caching frontend artwork without downloading ROMs",
            None,
        );
        self.metadata_handle = Some(std::thread::spawn(move || {
            let _ = tx.send(rommfs_core::retrobat::cache_artwork_cancellable(
                &client, &catalogue, &roms, &cache, &active,
            ));
        }));
    }

    pub(super) fn poll_metadata(&mut self) {
        self.poll_metadata_publication();
        if let Some(rx) = &self.metadata_job {
            let result = match rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err(Error::Transport("artwork worker stopped".into())))
                }
            };
            if let Some(result) = result {
                self.metadata_job = None;
                // The result has already been sent, so this thread is done.
                if let Some(handle) = self.metadata_handle.take() {
                    let _ = handle.join();
                }
                match result {
                    Ok(artwork) => self.metadata_ready = Some(artwork),
                    Err(error) => {
                        self.cancel_metadata();
                        if error.needs_sign_in() {
                            self.stop_save_sync_agent();
                            self.stop_mount();
                            self.sink.emit(AppEvent::SignInRequired {
                                reason: Some(error.to_string()),
                            });
                        }
                        self.metadata_status(
                            false,
                            "Artwork could not be cached; gamelists keep their existing artwork",
                            Some(error.to_string()),
                        );
                        return;
                    }
                }
            }
        }
        self.start_metadata_publication();
    }

    /// Generation and publication read and merge every RetroBat gamelist, so
    /// they run on their own thread. The worker only polls the result.
    fn start_metadata_publication(&mut self) {
        if self.metadata_publish.is_some() || self.metadata_ready.is_none() {
            return;
        }
        let Some(installation) = self
            .preferences
            .as_ref()
            .and_then(|p| p.retrobat_root.clone())
        else {
            self.cancel_metadata();
            return;
        };
        let guard = match stopped_retrobat_guard(&installation) {
            Ok(guard) => guard,
            Err(error)
                if !error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|io| matches!(io.raw_os_error(), Some(5 | 32 | 33))) =>
            {
                self.cancel_metadata();
                self.metadata_status(
                    false,
                    "Frontend metadata cannot be published safely",
                    Some(format!("{error:#}")),
                );
                return;
            }
            Err(error) => {
                if !self.metadata_wait_reported {
                    self.metadata_status(
                        true,
                        "Gamelists are ready; waiting for RetroBat and its emulators to close",
                        Some(format!("{error:#}")),
                    );
                    self.metadata_wait_reported = true;
                }
                return;
            }
        };
        let (Some(root), Some(server), Some(account), Some((catalogue, snapshot)), Some(artwork)) = (
            self.mount.as_ref().map(|mount| mount.root.clone()),
            self.server_id.clone(),
            self.preferences.as_ref().map(|p| p.account_id),
            self.metadata_source.clone(),
            self.metadata_ready.take(),
        ) else {
            self.cancel_metadata();
            return;
        };
        // A running artwork job owns the cancellation flag; a publication with
        // no artwork job publishes under a live flag.
        let active = self
            .metadata_active
            .clone()
            .unwrap_or_else(|| Arc::new(std::sync::atomic::AtomicBool::new(true)));
        self.spawn_metadata_publication(
            active,
            guard,
            installation,
            root,
            server,
            account,
            catalogue,
            snapshot,
            artwork,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_metadata_publication(
        &mut self,
        active: Arc<std::sync::atomic::AtomicBool>,
        guard: Vec<std::fs::File>,
        installation: PathBuf,
        root: PathBuf,
        server: String,
        account: i64,
        catalogue: Catalogue,
        snapshot: CatalogueSnapshot,
        artwork: rommfs_core::retrobat::ArtworkBatch,
    ) {
        let sink = self.sink.clone();
        let (tx, rx) = mpsc::channel();
        self.metadata_publish = Some(rx);
        self.metadata_publish_handle = Some(std::thread::spawn(move || {
            let _ = tx.send(publish_metadata(
                &sink,
                &active,
                guard,
                installation,
                root,
                server,
                account,
                catalogue,
                snapshot,
                artwork,
            ));
        }));
    }

    fn poll_metadata_publication(&mut self) {
        let Some(rx) = &self.metadata_publish else {
            return;
        };
        let publication = match rx.try_recv() {
            Ok(publication) => publication,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => MetadataPublication {
                changed: Err("metadata publisher stopped".into()),
                problems: Vec::new(),
            },
        };
        self.metadata_publish = None;
        if let Some(handle) = self.metadata_publish_handle.take() {
            let _ = handle.join();
        }
        self.finish_metadata_publication(publication);
    }

    fn finish_metadata_publication(&mut self, publication: MetadataPublication) {
        for (system, reason) in &publication.problems {
            self.log(Level::Warn, "metadata", format!("{system}: {reason}"));
        }
        let left_as_is = |text: &str| match publication.problems.len() {
            0 => text.to_string(),
            n => format!("{text}; {n} systems left as they were (see the log)"),
        };
        // A newer artwork batch or a still-running artwork job means this was
        // not the final publication; keep the job alive for the next pass.
        match publication.changed {
            Ok(changed) if self.metadata_job.is_some() || self.metadata_ready.is_some() => {
                // RetroBat may be started again before artwork finishes.
                self.metadata_wait_reported = false;
                self.log(
                    Level::Info,
                    "metadata",
                    format!(
                        "Published {changed} gamelists; frontend favourites and history preserved"
                    ),
                );
                self.metadata_status(
                    true,
                    left_as_is("Gamelists published; caching frontend artwork"),
                    None,
                );
            }
            Ok(_) => {
                self.cancel_metadata();
                self.metadata_status(false, left_as_is("Frontend metadata is up to date"), None);
            }
            Err(error) => {
                self.cancel_metadata();
                self.metadata_status(
                    false,
                    "Frontend metadata was not fully published; existing state was retained",
                    Some(error),
                );
            }
        }
    }

    pub(super) fn setup_retrobat(&mut self, installation: &Path, managed_root: &Path) {
        self.cancel_auxiliary();
        self.sink.emit(AppEvent::RetroBatSetup {
            running: true,
            detail: "Checking installation and ROM ownership".into(),
            error: None,
        });
        let result = (|| -> anyhow::Result<Vec<String>> {
            anyhow::ensure!(
                self.mount.is_none(),
                "stop the games folder before RetroBat setup"
            );
            anyhow::ensure!(
                installation.is_absolute() && managed_root.is_absolute(),
                "choose absolute RetroBat and managed ROM folder paths"
            );
            let server = self
                .server_id
                .as_deref()
                .context("connect before setting up RetroBat")?;
            let result = crate::retrobat_setup::setup(installation, managed_root, server);
            #[cfg(windows)]
            let result = crate::retrobat_setup::retry_elevated(result, || {
                self.sink.emit(AppEvent::RetroBatSetup {
                    running: true,
                    detail: "Waiting for Windows permission to finish RetroBat setup".into(),
                    error: None,
                });
                crate::retrobat_setup::windows::run_elevated(installation, managed_root, server)
            });
            if result.is_err() {
                self.emit_foreign_entries(managed_root);
            }
            result
        })();
        match result {
            Ok(messages) => {
                let detail = format!(
                    "RetroBat roms folder set up for {}: {}",
                    managed_root.display(),
                    messages.join(". ")
                );
                self.log(Level::Info, "retrobat-setup", detail.clone());
                self.set_integration(IntegrationMode::RetroBat, Some(installation.to_path_buf()));
                self.sink.emit(AppEvent::RetroBatSetup {
                    running: false,
                    detail,
                    error: None,
                });
            }
            Err(error) => {
                let error = format!("{error:#}");
                self.log(Level::Error, "retrobat-setup", error.clone());
                self.sink.emit(AppEvent::RetroBatSetup {
                    running: false,
                    detail: "RetroBat setup did not complete".into(),
                    error: Some(error),
                });
            }
        }
    }

    /// After the installer exits successfully: when discovery finds exactly one
    /// RetroBat installation and none is selected, select it like Browse does.
    pub(super) fn preselect_installed_retrobat(&mut self) {
        let selected = self
            .preferences
            .as_ref()
            .is_some_and(|p| p.retrobat_root.is_some());
        if selected || self.preferences.is_none() || self.mount.is_some() {
            return;
        }
        let report = discover_installations(save_sync_discovery_input());
        let [candidate] = report.candidates.as_slice() else {
            return;
        };
        let root = candidate.info.install_root.clone();
        self.set_integration(IntegrationMode::RetroBat, Some(root.clone()));
        if self
            .preferences
            .as_ref()
            .and_then(|p| p.retrobat_root.as_ref())
            == Some(&root)
        {
            self.sink.emit(AppEvent::RetroBatInstaller {
                running: false,
                ready: self.installer.is_ready(),
                received: 0,
                total: None,
                detail: format!(
                    "RetroBat installed at {}. Next: Set up RetroBat.",
                    root.display()
                ),
                error: None,
            });
        }
    }
    pub(super) fn set_integration(&mut self, mode: IntegrationMode, root: Option<PathBuf>) {
        self.cancel_auxiliary();
        let previous = self.preferences.clone();
        let result: Result<()> = (|| {
            if self.mount.is_some() {
                return Err(Error::Unsupported(
                    "stop the games folder before changing the layout".into(),
                ));
            }
            let preferences = self.preferences.as_mut().ok_or_else(|| {
                Error::Auth("connect and verify an account before choosing the layout".into())
            })?;
            if mode == IntegrationMode::RetroBat {
                let root = root
                    .as_deref()
                    .ok_or_else(|| Error::Unsupported("select a RetroBat installation".into()))?;
                rommfs_core::layout::RetroBatSystems::from_installation(root)?;
            }
            preferences.integration = mode;
            preferences.retrobat_root = root;
            let catalogue = self
                .source_snapshot
                .as_ref()
                .map(|s| self.project_snapshot(s))
                .transpose()?;
            if let (Some(settings), Some(preferences)) =
                (&self.connection_settings, &self.preferences)
            {
                settings.save(preferences)?;
            }
            self.stop_save_sync_agent();
            self.advance_save_sync_session();
            if let Some(catalogue) = catalogue {
                self.sink.emit(AppEvent::CatalogueLoaded {
                    platforms: catalogue.platforms.len(),
                    roms: catalogue.entries.len(),
                    skipped_unsupported: catalogue.skipped_unsupported,
                });
                self.store_catalogue(catalogue);
            }
            self.refresh_save_sync();
            Ok(())
        })();
        if let Err(error) = result {
            self.preferences = previous;
            self.log(Level::Error, "integration", error.to_string());
            self.emit_integration(Some(error.to_string()));
        } else {
            self.emit_integration(None);
        }
    }
}

/// Outcome of an off-loop gamelist generation + publication pass.
pub(super) struct MetadataPublication {
    /// Gamelists changed on success, or the failure text.
    pub(super) changed: std::result::Result<usize, String>,
    /// `(system, reason)` for systems left as they were.
    pub(super) problems: Vec<(String, String)>,
}

fn emit_metadata_status(
    sink: &EventSink,
    pending: bool,
    detail: impl Into<String>,
    error: Option<String>,
) {
    let detail = detail.into();
    // One log line per status: a failure carries its own error text.
    match &error {
        Some(error) if pending => sink.emit(AppEvent::log(
            Level::Warn,
            "metadata",
            format!("{detail}: {error}"),
        )),
        Some(error) => sink.emit(AppEvent::log(
            Level::Error,
            "metadata",
            format!("{detail}: {error}"),
        )),
        None => sink.emit(AppEvent::log(Level::Info, "metadata", detail.clone())),
    }
    sink.emit(AppEvent::FrontendMetadata {
        pending,
        detail,
        error,
    });
}

/// Read the frontend state, merge the RomM entries and publish both gamelists
/// and artwork. Runs on the metadata thread; `_guard` holds RetroBat closed
/// for the whole pass.
#[allow(clippy::too_many_arguments)]
fn publish_metadata(
    sink: &EventSink,
    active: &std::sync::atomic::AtomicBool,
    _guard: Vec<std::fs::File>,
    installation: PathBuf,
    root: PathBuf,
    server: String,
    account: i64,
    catalogue: Catalogue,
    snapshot: CatalogueSnapshot,
    artwork: rommfs_core::retrobat::ArtworkBatch,
) -> MetadataPublication {
    let mut problems = Vec::new();
    for warning in artwork.warnings() {
        sink.emit(AppEvent::log(Level::Warn, "artwork", warning.clone()));
    }
    let result = (|| -> anyhow::Result<usize> {
        let linked = rommfs_core::retrobat::linked_systems(&installation, &root, &server)?;
        let frontend = rommfs_core::retrobat::read_gamelists(&installation, &linked);
        let (index, legacy) = rommfs_cfapi::read_frontend_metadata(&root, &server)?;
        let publication = rommfs_core::retrobat::generate_gamelists(
            &catalogue,
            &snapshot.roms,
            &server,
            account,
            &index,
            &frontend,
            &legacy,
            artwork.references(),
        )?;
        problems.extend(publication.skipped().iter().cloned());
        let changed = publication.gamelists().len();
        // A cancelled job (stop mount, sign-out) must not publish stale state.
        if !active.load(std::sync::atomic::Ordering::Acquire) {
            anyhow::bail!("metadata publication cancelled");
        }
        if changed > 0 || !artwork.files().is_empty() {
            emit_metadata_status(
                sink,
                true,
                format!(
                    "Updating {changed} gamelists and {} artwork files",
                    artwork.files().len()
                ),
                None,
            );
        }
        rommfs_cfapi::publish_frontend_metadata(
            &root,
            &server,
            publication.index(),
            artwork.files(),
            || {
                let failures =
                    rommfs_core::retrobat::publish_gamelists(&installation, &linked, &publication);
                let failed = failures.len();
                problems.extend(failures);
                // Keep the staged index if any XML is still at its old
                // paths; recapturing it with the new mapping loses history.
                anyhow::ensure!(
                    failed == 0,
                    "{failed} gamelists could not be written; identity state is staged for retry"
                );
                Ok(())
            },
        )?;
        Ok(changed)
    })();
    MetadataPublication {
        changed: result.map_err(|error| format!("{error:#}")),
        problems,
    }
}
