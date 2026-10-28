//! Games-folder validation, mount lifecycle, and cache eviction.
use super::*;

impl Worker {
    pub(super) fn fail_connect(&self, e: &Error) {
        if e.needs_sign_in() {
            self.sink.emit(AppEvent::SignInRequired {
                reason: Some(e.to_string()),
            });
        } else {
            self.sink.emit(AppEvent::ConnectFailed {
                reason: e.to_string(),
            });
        }
        self.log(Level::Error, "connect", e.to_string());
    }

    pub(super) fn fail_mount(&self, stage: &str, path: &str, e: &Error) {
        let reason = format!("{stage} for mount {path:?} failed: {e}");
        self.sink.emit(AppEvent::MountFailed {
            reason: reason.clone(),
        });
        self.log(Level::Error, "mount", reason);
    }

    /// The classified foreign entries under a directory the app does not own —
    /// the remediation list the UI renders with per-kind actions.
    pub(super) fn foreign_entry_list(root: &Path) -> Vec<ForeignEntry> {
        rommfs_cfapi::foreign_entries(root)
            .into_iter()
            .map(|e| ForeignEntry {
                path: e.path,
                kind: match e.kind {
                    rommfs_cfapi::UnownedKind::File => ForeignKind::File,
                    rommfs_cfapi::UnownedKind::Directory => ForeignKind::Directory,
                    rommfs_cfapi::UnownedKind::Link => ForeignKind::Link,
                },
            })
            .collect()
    }

    /// After a mount/setup refusal, publish what's actually in the way so the
    /// UI can offer "remove leftover links" / "open folder" instead of a bare
    /// path dump.
    pub(super) fn emit_foreign_entries(&self, root: &Path) {
        let entries = Self::foreign_entry_list(root);
        if !entries.is_empty() {
            self.sink.emit(AppEvent::ForeignRootEntries {
                root: root.display().to_string(),
                entries,
                removal: None,
            });
        }
    }

    /// Remove only foreign symlinks/junctions under `root` — never real
    /// files or directories — then republish the remaining list.
    pub(super) fn remove_foreign_links(&mut self, root: &Path) {
        let removal = rommfs_cfapi::remove_foreign_links(root).map_err(|e| format!("{e:#}"));
        match &removal {
            Ok(0) => self.log(Level::Info, "cleanup", "no leftover links found"),
            Ok(n) => self.log(
                Level::Info,
                "cleanup",
                format!("removed {n} leftover links under {}", root.display()),
            ),
            Err(e) => self.log(Level::Error, "cleanup", format!("link cleanup failed: {e}")),
        }
        self.sink.emit(AppEvent::ForeignRootEntries {
            root: root.display().to_string(),
            entries: Self::foreign_entry_list(root),
            removal: Some(removal),
        });
    }

    /// Apply the Settings games-folder edit to this session's remembered
    /// config. Validation stays with start_mount — applying only records
    /// the chosen folder so every screen uses it.
    pub(super) fn set_mount_root(&mut self, path: &str) {
        let path = path.trim();
        let Some(preferences) = self.preferences.as_ref() else {
            self.log(
                Level::Warn,
                "settings",
                "Games folder change ignored: no remembered session",
            );
            return;
        };
        let old_root = preferences.mount_root.clone();
        let server_id = server_id_of(&preferences.server);
        let new_root = PathBuf::from(path);
        if old_root.as_ref().is_some_and(|old| old != &new_root) {
            if let Some(job) = &self.mount_preparation {
                job.active
                    .store(false, std::sync::atomic::Ordering::Release);
            } else if self.mount.is_none() {
                if let Some(old) = &old_root {
                    if let Err(error) = cleanup_replaced_mount_root(old, &server_id) {
                        self.log(
                            Level::Error,
                            "mount",
                            format!(
                                "could not replace old games folder {}: {error:#}",
                                old.display()
                            ),
                        );
                    }
                }
            }
        }
        let preferences = self.preferences.as_mut().expect("checked above");
        preferences.mount_root = Some(new_root);
        let server = preferences.server.clone();
        if let Some(settings) = &self.connection_settings {
            if let Err(error) = settings.save(preferences) {
                self.log(Level::Warn, "settings", error.to_string());
            }
        }
        self.sink.emit(AppEvent::MountRootConfigured {
            server,
            mount_root: path.to_string(),
        });
    }

    pub(super) fn start_mount(&mut self, path: &str) {
        let path = path.trim();
        if self.auxiliary_job.is_some() {
            self.fail_mount(
                "wait for auxiliary downloads",
                path,
                &Error::Unsupported(
                    "Finish or cancel the support file install before starting the games folder"
                        .into(),
                ),
            );
            return;
        }
        if self.save_sync_scope.as_ref().is_some_and(|scope| {
            rommfs_core::save_sync::paths_overlap(Path::new(path), &scope.effective_saves_root)
        }) {
            self.fail_mount(
                "check save-folder separation",
                path,
                &Error::Unsupported("ROM and save folders must not overlap".into()),
            );
            return;
        }
        if self.mount.is_some() {
            // Duplicate starts can already be queued when the UI receives
            // MountStarting. Keep the real mounted state authoritative.
            if self.save_sync_enabled
                && !self
                    .save_sync_gate
                    .enabled_for(self.current_save_sync_epoch)
            {
                self.refresh_save_sync();
            }
            return;
        }
        if self.mount_preparation.is_some() {
            return;
        }
        let Some(client) = self.client.clone() else {
            self.fail_mount(
                "check connection",
                path,
                &Error::Auth("connect to a RomM server before mounting".into()),
            );
            self.refresh_save_sync();
            return;
        };
        if !client.has_token() && self.catalogue.is_some() {
            self.sink.emit(AppEvent::SignInRequired {
                reason: Some("sign in before mounting the cached library".into()),
            });
            self.fail_mount(
                "check authentication",
                path,
                &Error::Auth("sign in before mounting cached metadata".into()),
            );
            return;
        }
        let Some(server_id) = self.server_id.clone() else {
            self.fail_mount(
                "check connection",
                path,
                &Error::Auth("connect to a RomM server before mounting".into()),
            );
            self.refresh_save_sync();
            return;
        };

        self.sink.emit(AppEvent::MountStarting {
            path: path.to_string(),
        });
        let root = PathBuf::from(path);
        let retrobat_install = self
            .preferences
            .as_ref()
            .filter(|p| p.integration == IntegrationMode::RetroBat)
            .map(|p| p.retrobat_root.clone());
        self.log(
            Level::Info,
            "mount",
            format!("checking and preparing mount directory {path:?}"),
        );

        if self.catalogue.is_none() {
            self.pending_mount_start = Some(path.to_string());
            if self.refresh_result.is_none() {
                self.refresh_catalogue();
            }
            return;
        }
        let started = Instant::now();
        let catalogue = self.catalogue.clone().expect("checked above");
        let names = self.names.clone();
        let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let sink = self.sink.clone().scoped(active.clone());
        let thread_active = active.clone();
        let (tx, rx) = mpsc::channel();
        let thread_root = root.clone();
        let thread_server = server_id.clone();
        let thread_retrobat_install = retrobat_install;
        let remover = hydrated_remover(&root);
        #[cfg(test)]
        let test_cache_directory = self.rom_cache_directory.clone();
        let handle = std::thread::spawn(move || {
            let result = (|| {
                if let Some(install) = thread_retrobat_install {
                    let install = install.ok_or_else(|| {
                        Error::Unsupported("select a RetroBat installation".into())
                    })?;
                    let outcome = rommfs_core::retrobat::inspect_setup_with_root_check(
                        &install,
                        &thread_root,
                        &thread_server,
                        |managed| {
                            check_mount_root(managed, &thread_server).map_err(anyhow::Error::from)
                        },
                    )
                    .map_err(|error| Error::Unsupported(format!("{error:#}")))?;
                    if !outcome.skipped.is_empty()
                        || outcome.linked.len() != outcome.record.links.len()
                    {
                        return Err(Error::Unsupported(
                            "one or more RetroBat system links are missing".into(),
                        ));
                    }
                } else {
                    check_mount_root(&thread_root, &thread_server)?;
                }
                claim_mount_root(&thread_root, &thread_server)?;
                if !thread_active.load(std::sync::atomic::Ordering::Acquire) {
                    return Err(Error::Cancelled);
                }
                let cache_directory = cache_dir_for(&thread_server);
                #[cfg(test)]
                let cache_directory = test_cache_directory.unwrap_or(cache_directory);
                let fs = Self::build_fs_for_mount(
                    &client,
                    catalogue,
                    names,
                    cache_directory,
                    sink,
                    remover,
                )?;
                if !thread_active.load(std::sync::atomic::Ordering::Acquire) {
                    return Err(Error::Cancelled);
                }
                let mount = start_mount_backend(Arc::clone(&fs), &thread_root)?;
                if !thread_active.load(std::sync::atomic::Ordering::Acquire) {
                    mount.stop();
                    return Err(Error::Cancelled);
                }
                Ok((fs, mount))
            })();
            if let Err(error) = tx.send(result) {
                if let Ok((_, mount)) = error.0 {
                    mount.stop();
                }
            }
        });
        self.mount_preparation = Some(MountPreparation {
            root,
            server_id,
            path: path.to_string(),
            started,
            active,
            result: rx,
            handle,
        });
    }

    /// Assemble the portable filesystem from immutable job inputs.
    fn build_fs_for_mount(
        client: &Arc<RommClient>,
        cat: Catalogue,
        names: HashMap<RomKey, String>,
        cache_directory: PathBuf,
        sink: EventSink,
        remover: Arc<dyn HydratedRemover>,
    ) -> Result<Arc<RommFs>> {
        let index = CacheIndex::open(cache_directory)?;
        let live = Arc::new(LiveState::default());
        let source = Arc::new(ClientSource {
            client: Arc::clone(client),
            names,
        });
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
            sink,
            expected,
            versions,
        ));
        let evictor = Evictor::new(DEFAULT_EVICTION_THRESHOLD_SECS, live, remover);
        let tree = RommTree::new(cat);
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        Ok(Arc::new(RommFs::new(tree, downloads, evictor, clock)))
    }

    #[cfg(test)]
    pub(super) fn build_fs(
        &mut self,
        client: &Arc<RommClient>,
        _root: &Path,
        remover: Arc<dyn HydratedRemover>,
    ) -> Result<Arc<RommFs>> {
        let server = self.server_id.clone().unwrap_or_default();
        let catalogue = self
            .catalogue
            .clone()
            .ok_or_else(|| Error::InvalidCatalogue("no catalogue loaded".into()))?;
        let cache = self
            .rom_cache_directory
            .clone()
            .unwrap_or_else(|| cache_dir_for(&server));
        Self::build_fs_for_mount(
            client,
            catalogue,
            self.names.clone(),
            cache,
            self.sink.clone(),
            remover,
        )
    }

    pub(super) fn poll_mount_preparation(&mut self) {
        let Some(job) = &self.mount_preparation else {
            return;
        };
        let result = match job.result.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err(Error::Transport(
                "mount preparation stopped unexpectedly".into(),
            )),
        };
        let job = self.mount_preparation.take().expect("job was present");
        let _ = job.handle.join();
        if !job.active.load(std::sync::atomic::Ordering::Acquire) {
            if let Ok((_, mount)) = result {
                mount.stop();
            }
            self.cleanup_if_replaced(&job.root, &job.server_id);
            return;
        }
        match result {
            Ok((fs, mount)) if job.active.load(std::sync::atomic::Ordering::Acquire) => {
                let mounted_path = mount.root.display().to_string();
                self.mount = Some(mount);
                self.fs = Some(fs);
                if let Some(preferences) = &mut self.preferences {
                    preferences.mount_root = Some(job.root);
                    if let Some(settings) = &self.connection_settings {
                        if let Err(error) = settings.save(preferences) {
                            self.log(Level::Warn, "settings", error.to_string());
                        }
                    }
                }
                self.sink
                    .emit(AppEvent::MountStarted { path: mounted_path });
                if self.startup_requires_mount {
                    self.startup_requires_mount = false;
                    self.sink.emit(AppEvent::StartupPreference {
                        enabled: self.preferences.as_ref().is_some_and(|p| p.start_at_login),
                        error: None,
                    });
                }
                self.queue_metadata();
                self.log(
                    Level::Info,
                    "mount",
                    format!(
                        "mounted at {} in {:.2}s",
                        job.path,
                        job.started.elapsed().as_secs_f64()
                    ),
                );
                self.evict_once();
            }
            Ok((_, mount)) => {
                mount.stop();
                self.cleanup_if_replaced(&job.root, &job.server_id);
            }
            Err(Error::Cancelled) => self.cleanup_if_replaced(&job.root, &job.server_id),
            Err(error) => {
                self.emit_foreign_entries(&job.root);
                self.fail_mount(
                    &format!(
                        "prepare and start filesystem backend after {} ms",
                        job.started.elapsed().as_millis()
                    ),
                    &job.path,
                    &error,
                );
                self.refresh_save_sync();
                self.cleanup_if_replaced(&job.root, &job.server_id);
            }
        }
    }

    fn cleanup_if_replaced(&mut self, root: &Path, server_id: &str) {
        if self
            .preferences
            .as_ref()
            .and_then(|p| p.mount_root.as_deref())
            .is_some_and(|configured| configured != root)
        {
            if let Err(error) = cleanup_replaced_mount_root(root, server_id) {
                self.log(
                    Level::Error,
                    "mount",
                    format!(
                        "could not replace old games folder {}: {error:#}",
                        root.display()
                    ),
                );
            }
        }
    }

    pub(super) fn cancel_mount_preparation(&mut self) {
        if let Some(job) = self.mount_preparation.take() {
            job.active
                .store(false, std::sync::atomic::Ordering::Release);
            self.cancelled_mount_preparations.push(job);
        }
        self.pending_mount_start = None;
    }

    pub(super) fn poll_cancelled_mount_preparations(&mut self) {
        let mut pending = Vec::new();
        for job in std::mem::take(&mut self.cancelled_mount_preparations) {
            let result = match job.result.try_recv() {
                Ok(result) => result,
                Err(mpsc::TryRecvError::Empty) => {
                    pending.push(job);
                    continue;
                }
                Err(mpsc::TryRecvError::Disconnected) => Err(Error::Transport(
                    "cancelled mount preparation stopped unexpectedly".into(),
                )),
            };
            let _ = job.handle.join();
            if let Ok((_, mount)) = result {
                mount.stop();
            }
            self.cleanup_if_replaced(&job.root, &job.server_id);
        }
        self.cancelled_mount_preparations = pending;

        let pending_cleanup = std::mem::take(&mut self.pending_root_cleanup);
        for (root, server_id) in pending_cleanup {
            if self
                .cancelled_mount_preparations
                .iter()
                .any(|job| job.root == root)
            {
                self.pending_root_cleanup.push((root, server_id));
            } else if let Err(error) = cleanup_replaced_mount_root(&root, &server_id) {
                self.log(
                    Level::Error,
                    "mount",
                    format!(
                        "could not replace previous games folder {}: {error:#}",
                        root.display()
                    ),
                );
            }
        }
    }

    pub(super) fn drain_cancelled_mount_preparations(&mut self) {
        for job in std::mem::take(&mut self.cancelled_mount_preparations) {
            let _ = job.handle.join();
            if let Ok(Ok((_, mount))) = job.result.try_recv() {
                mount.stop();
            }
            self.cleanup_if_replaced(&job.root, &job.server_id);
        }
        self.poll_cancelled_mount_preparations();
    }

    pub(super) fn evict_once(&mut self) {
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

    /// Stop and immediately re-start, so a pending catalogue refresh is
    /// published in one worker step (phase 2b "Update ready").
    pub(super) fn restart_mount(&mut self, path: &str) {
        if self.mount.is_some() {
            self.stop_mount();
        }
        self.start_mount(path);
    }

    pub(super) fn stop_mount(&mut self) {
        let cancel_cold_load = self.pending_mount_start.take().is_some();
        self.cancel_mount_preparation();
        if cancel_cold_load {
            self.cancel_catalogue_refresh();
        }
        let had_metadata = self.metadata_active.is_some()
            || self
                .preferences
                .as_ref()
                .is_some_and(|p| p.integration == IntegrationMode::RetroBat);
        self.cancel_metadata();
        if had_metadata {
            self.log(
                Level::Info,
                "metadata",
                "Frontend files are retained while the mount is stopped",
            );
        }
        self.sink.emit(AppEvent::MountStopping);
        if let Some(fs) = &self.fs {
            fs.cancel_transfers();
        }
        let stopped_root = self.mount.as_ref().map(|mount| mount.root.clone());
        if let Some(mount) = self.mount.take() {
            mount.stop();
            self.log(Level::Info, "mount", "mount stopped".to_string());
        }
        if let (Some(root), Some(server)) = (stopped_root, self.server_id.clone()) {
            self.cleanup_if_replaced(&root, &server);
        }
        self.fs = None;
        self.sink.emit(AppEvent::MountStopped);
    }
}
