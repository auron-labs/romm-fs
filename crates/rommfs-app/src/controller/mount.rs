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
        let Some(preferences) = &mut self.preferences else {
            self.log(
                Level::Warn,
                "settings",
                "Games folder change ignored: no remembered session",
            );
            return;
        };
        preferences.mount_root = Some(PathBuf::from(path));
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
        if let Some(preferences) = self
            .preferences
            .as_ref()
            .filter(|p| p.integration == IntegrationMode::RetroBat)
        {
            let verified = preferences
                .retrobat_root
                .as_deref()
                .context("select a RetroBat installation")
                .and_then(|install| {
                    rommfs_core::retrobat::verify_setup_link(install, &root, &server_id)
                });
            if let Err(error) = verified {
                self.fail_mount(
                    "verify RetroBat setup",
                    path,
                    &Error::Unsupported(format!("{error:#}")),
                );
                return;
            }
        }
        let started = Instant::now();
        self.log(
            Level::Info,
            "mount",
            format!("checking and preparing mount directory {path:?}"),
        );

        // Validate the root BEFORE claiming it: only empty dirs or roots we
        // previously marked for THIS server (PRD §5 — never over an existing
        // ROM library, never recursively cleared).
        if let Err(e) = check_mount_root(&root, &server_id) {
            self.emit_foreign_entries(&root);
            self.fail_mount("prepare and validate directory", path, &e);
            self.refresh_save_sync();
            return;
        }

        if self.catalogue.is_none() {
            // Reuse Connect's stable snapshot; failed cold loads may be retried.
            self.log(
                Level::Info,
                "mount",
                format!("loading catalogue for {path:?}"),
            );
            match self.load_catalogue(&client) {
                Ok(cat) => {
                    self.store_catalogue(cat);
                    self.refresh_save_sync();
                }
                Err(e) if e.needs_sign_in() => {
                    self.sink.emit(AppEvent::SignInRequired {
                        reason: Some(e.to_string()),
                    });
                    self.fail_mount("load catalogue", path, &e);
                    self.refresh_save_sync();
                    return;
                }
                Err(e) => {
                    self.sink.emit(AppEvent::CatalogueFailed {
                        reason: e.to_string(),
                    });
                    self.fail_mount("load catalogue", path, &e);
                    self.refresh_save_sync();
                    return;
                }
            }
        }
        self.log(
            Level::Info,
            "mount",
            format!("claiming ownership of {path:?}"),
        );
        if let Err(e) = claim_mount_root(&root, &server_id) {
            self.emit_foreign_entries(&root);
            self.fail_mount("claim ownership", path, &e);
            return;
        }

        let remover = hydrated_remover(&root);
        self.log(
            Level::Info,
            "mount",
            format!(
                "opening private cache {}",
                cache_dir_for(&server_id).display()
            ),
        );
        let fs = match self.build_fs(&client, &root, remover) {
            Ok(fs) => fs,
            Err(e) => {
                self.fail_mount("open cache and build filesystem", path, &e);
                return;
            }
        };

        self.log(
            Level::Info,
            "mount",
            format!("starting Windows Cloud Files mount at {path:?}"),
        );
        match start_mount_backend(Arc::clone(&fs), &root) {
            Ok(mount) => {
                let mounted_path = mount.root.display().to_string();
                self.mount = Some(mount);
                if self.startup_requires_mount {
                    self.startup_requires_mount = false;
                    self.sink.emit(AppEvent::StartupPreference {
                        enabled: self.preferences.as_ref().is_some_and(|p| p.start_at_login),
                        error: None,
                    });
                }
                self.fs = Some(fs);
                if let Some(preferences) = &mut self.preferences {
                    preferences.mount_root = Some(root.clone());
                    if let Some(settings) = &self.connection_settings {
                        if let Err(error) = settings.save(preferences) {
                            self.log(Level::Warn, "settings", error.to_string());
                        }
                    }
                }
                self.sink
                    .emit(AppEvent::MountStarted { path: mounted_path });
                self.queue_metadata();
                self.log(
                    Level::Info,
                    "mount",
                    format!(
                        "mounted at {path} in {:.2}s",
                        started.elapsed().as_secs_f64()
                    ),
                );
                // One immediate sweep so expired entries from previous runs
                // are reclaimed (PRD R4); per-ROM outcomes become events.
                self.evict_once();
            }
            Err(e) => {
                // Backend-start failures can carry UnownedEntries (foreign
                // files under the mount root); publish the remediation list.
                self.emit_foreign_entries(&root);
                self.fail_mount("start filesystem backend", path, &e)
            }
        }
    }

    /// Assemble the portable core objects the platform adapter mounts.
    /// CFAPI eviction dehydrates the NTFS copy before removing private bytes.
    pub(super) fn build_fs(
        &mut self,
        client: &Arc<RommClient>,
        _root: &Path,
        remover: Arc<dyn HydratedRemover>,
    ) -> Result<Arc<RommFs>> {
        let server_id = self.server_id.clone().unwrap_or_default();
        let cache_directory = cache_dir_for(&server_id);
        #[cfg(test)]
        let cache_directory = self.rom_cache_directory.clone().unwrap_or(cache_directory);
        let index = CacheIndex::open(cache_directory)?;
        let live = Arc::new(LiveState::default());
        let source = Arc::new(ClientSource {
            client: Arc::clone(client),
            names: self.names.clone(),
        });
        let cat = self.catalogue.clone();
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
        let evictor = Evictor::new(DEFAULT_EVICTION_THRESHOLD_SECS, live, remover);
        let tree = RommTree::new(cat);
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        Ok(Arc::new(RommFs::new(tree, downloads, evictor, clock)))
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

    pub(super) fn stop_mount(&mut self) {
        let had_metadata = self.metadata_active.is_some()
            || self
                .preferences
                .as_ref()
                .is_some_and(|p| p.integration == IntegrationMode::RetroBat);
        self.cancel_metadata();
        if had_metadata {
            self.metadata_status(
                false,
                "Frontend files are retained while the mount is stopped",
                None,
            );
        }
        self.sink.emit(AppEvent::MountStopping);
        if let Some(fs) = &self.fs {
            fs.cancel_transfers();
        }
        if let Some(mount) = self.mount.take() {
            mount.stop();
            self.log(Level::Info, "mount", "mount stopped".to_string());
        }
        self.fs = None;
        self.sink.emit(AppEvent::MountStopped);
    }
}
