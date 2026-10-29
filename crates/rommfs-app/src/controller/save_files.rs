use super::*;
use rommfs_core::save_sync::{ensure_no_reparse_components, SaveFileRules};

impl Worker {
    pub(super) fn set_start_at_login(&mut self, enabled: bool) {
        let result = (|| -> Result<()> {
            let prefs = self.preferences.as_mut().ok_or_else(|| {
                Error::Unsupported("Connect before enabling start at login".into())
            })?;
            if enabled
                && rommfs_core::credential_store::load(
                    &server_id_of(&prefs.server),
                    prefs.account_id,
                )?
                .is_none()
            {
                return Err(Error::Unsupported(
                    "Use remembered browser sign-in before enabling login startup".into(),
                ));
            }
            let settings = self.connection_settings.as_ref().ok_or_else(|| {
                Error::Unsupported("Connection preferences are unavailable".into())
            })?;
            crate::desktop::set_start_at_login(enabled)?;
            let old = prefs.start_at_login;
            prefs.start_at_login = enabled;
            if let Err(e) = settings.save(prefs) {
                prefs.start_at_login = old;
                let _ = crate::desktop::set_start_at_login(old);
                return Err(e);
            }
            Ok(())
        })();
        self.sink.emit(AppEvent::StartupPreference {
            enabled: self.preferences.as_ref().is_some_and(|p| p.start_at_login),
            error: result.err().map(|e| e.to_string()),
        });
    }
    pub(super) fn select_save_folder(&mut self, path: &Path) {
        let result = (|| -> Result<()> {
            if !path.is_absolute() {
                return Err(Error::Unsupported(
                    "Choose an absolute save folder containing save data only".into(),
                ));
            }
            ensure_no_reparse_components(path)?;
            let prefs = self
                .preferences
                .as_mut()
                .ok_or_else(|| Error::Auth("Connect before choosing a save folder".into()))?;
            prefs.save_root = Some(path.to_path_buf());
            self.connection_settings
                .as_ref()
                .ok_or_else(|| Error::Unsupported("Save preferences are unavailable".into()))?
                .save(prefs)
        })();
        if let Err(e) = result {
            self.save_sync_runtime_problem = Some(e.to_string());
        }
        self.refresh_save_sync();
    }
    pub(super) fn associate_save_file(&mut self, path: &Path, rom_id: i64) {
        let result = (|| -> Result<()> {
            let scope = self
                .save_sync_scope
                .as_ref()
                .ok_or_else(|| Error::Unsupported("Choose a save folder first".into()))?;
            let rules = self.save_file_rules.as_ref().ok_or_else(|| {
                Error::Unsupported("Refresh the save files before mapping".into())
            })?;
            rules.mapping(rom_id, path, &scope.effective_saves_root)?;
            self.save_sync_settings
                .as_ref()
                .ok_or_else(|| Error::Unsupported("Save mapping settings are unavailable".into()))?
                .associate_file(scope, path, rom_id)
        })();
        if let Err(e) = result {
            self.save_sync_runtime_problem = Some(e.to_string());
        }
        self.refresh_save_sync();
    }
    /// Associate currently-unmatched files directly in `prefix` (a
    /// save-root-relative directory group) with one game. A file that fails
    /// validation stays unmatched; it never blocks the others.
    pub(super) fn associate_save_folder(&mut self, prefix: &Path, rom_id: i64) {
        let result = (|| -> Result<()> {
            let scope = self
                .save_sync_scope
                .as_ref()
                .ok_or_else(|| Error::Unsupported("Choose a save folder first".into()))?;
            let rules = self.save_file_rules.as_ref().ok_or_else(|| {
                Error::Unsupported("Refresh the save files before mapping".into())
            })?;
            let settings = self.save_sync_settings.as_ref().ok_or_else(|| {
                Error::Unsupported("Save mapping settings are unavailable".into())
            })?;
            let plan = rules.scan(&scope.effective_saves_root)?;
            let mut mapped = 0usize;
            let mut skipped = 0usize;
            for file in &plan.unmatched {
                // Match the immediate parent, exactly as the UI groups rows.
                // An empty parent is the save-root group, not the whole tree.
                let in_group = file.path.parent().unwrap_or(Path::new("")) == prefix;
                if !in_group {
                    continue;
                }
                // Some unmatched reasons (unsafe paths, shared dirs) must stay
                // unmatched even under a bulk association.
                if rules
                    .mapping(rom_id, &file.path, &scope.effective_saves_root)
                    .is_err()
                {
                    skipped += 1;
                    continue;
                }
                settings.associate_file(scope, &file.path, rom_id)?;
                mapped += 1;
            }
            if mapped == 0 {
                return Err(Error::Unsupported(format!(
                    "No unmatched files under {} could be associated",
                    prefix.display()
                )));
            }
            if skipped > 0 {
                self.sink.emit(AppEvent::log(
                    Level::Warn,
                    "save-sync",
                    format!(
                        "Mapped {mapped} files under {}; {skipped} were not eligible",
                        prefix.display()
                    ),
                ));
            }
            Ok(())
        })();
        if let Err(e) = result {
            self.save_sync_runtime_problem = Some(e.to_string());
        }
        self.refresh_save_sync();
    }
    pub(super) fn refresh_save_files(&mut self, input: DiscoveryInput) {
        let discovery = discover_installations(input);
        self.save_file_rules = None;
        self.save_sync_scope = None;
        self.save_sync_profile = None;
        self.save_sync_mapping_report = None;
        self.save_sync_enabled = false;
        let mut problem = self.save_sync_settings_problem.clone();
        let mut games = Vec::new();
        let mut unmatched = Vec::new();
        let mut scan_completed_at = None;
        let result = (|| -> Result<()> {
            let prefs = self
                .preferences
                .as_ref()
                .ok_or_else(|| Error::Auth("Connect to configure save sync".into()))?;
            let (installation_root, effective_saves_root, version) = if let Some(root) =
                &prefs.save_root
            {
                (root.clone(), root.clone(), "ordinary files".into())
            } else if let Some(root) = self
                .save_sync_selected_root
                .as_ref()
                .or(prefs.retrobat_root.as_ref())
            {
                let (_, saves) = rommfs_core::retrobat::launcher_paths(root)?;
                let version = std::fs::read_to_string(root.join("system/version.info"))
                    .unwrap_or_else(|_| "compatible installation".into());
                (root.clone(), saves, version.trim().to_owned())
            } else {
                return Err(Error::Unsupported("Choose a save-data folder, or select a RetroBat installation. Shared saves/cards are not synchronized.".into()));
            };
            if !effective_saves_root.is_absolute() {
                return Err(Error::Unsupported("Save folder must be absolute".into()));
            }
            ensure_no_reparse_components(&effective_saves_root)?;
            if let Some(roms) = &prefs.mount_root {
                if rommfs_core::save_sync::paths_overlap(&effective_saves_root, roms) {
                    return Err(Error::Unsupported(
                        "Save folder must not overlap the managed ROM folder".into(),
                    ));
                }
            }
            if let Some(directory) = &self.save_sync_storage_dir {
                if rommfs_core::save_sync::paths_overlap(&effective_saves_root, directory) {
                    return Err(Error::Unsupported(
                        "Save folder must not contain application settings or snapshots".into(),
                    ));
                }
            }
            let retrobat_root = if prefs.save_root.is_none() {
                Some(&installation_root)
            } else {
                prefs.retrobat_root.as_ref()
            };
            if let Some(root) = retrobat_root {
                let (bios, cheats) = rommfs_core::retrobat::auxiliary_paths(root)?;
                for other in [
                    bios,
                    cheats,
                    root.join("roms"),
                    root.join("emulationstation"),
                ] {
                    if rommfs_core::save_sync::paths_overlap(&effective_saves_root, &other) {
                        return Err(Error::Unsupported("Select save data only, not RetroBat ROMs, BIOS, cheats, frontend settings or the whole installation".into()));
                    }
                }
                for emulator in ["eden", "citron", "retroarch"] {
                    let portable = root.join("emulators").join(emulator);
                    if rommfs_core::save_sync::paths_overlap(&effective_saves_root, &portable)
                        && !effective_saves_root.starts_with(&portable)
                        || portable == effective_saves_root
                        || portable.join("user") == effective_saves_root
                    {
                        return Err(Error::Unsupported("Select the emulator's save subfolder, not its whole portable/user directory".into()));
                    }
                    for name in [
                        "user/keys",
                        "user/config",
                        "user/cache",
                        "user/nand/system/Contents/registered",
                    ] {
                        if rommfs_core::save_sync::paths_overlap(
                            &effective_saves_root,
                            &portable.join(name),
                        ) {
                            return Err(Error::Unsupported("Save folder overlaps firmware, keys, emulator configuration or cache data".into()));
                        }
                    }
                }
            }
            let identity = self.save_sync_identity.as_ref().ok_or_else(|| {
                Error::Auth("A verified account is required for save sync".into())
            })?;
            let scope = SaveSyncScope {
                server_id: self
                    .server_id
                    .clone()
                    .ok_or_else(|| Error::Auth("Connect before configuring saves".into()))?,
                account_id: identity.account_id.to_string(),
                installation_root,
                effective_saves_root,
            };
            let settings = self
                .save_sync_settings
                .as_ref()
                .ok_or_else(|| Error::Unsupported("Save settings are unavailable".into()))?;
            let catalogue = self.catalogue.as_ref().ok_or_else(|| {
                Error::Unsupported("Load the catalogue before configuring saves".into())
            })?;
            let source = self
                .source_snapshot
                .as_ref()
                .ok_or_else(|| Error::Unsupported("Source catalogue is unavailable".into()))?;
            let rules = SaveFileRules::from_catalogue(
                catalogue,
                &source.roms,
                settings.file_associations(&scope)?,
            )?;
            games = rules.game_names();
            let mut plan = rules.scan(&scope.effective_saves_root)?;
            scan_completed_at = Some(rommfs_core::cache::now_unix_secs());
            if let Some(directory) = &self.save_sync_storage_dir {
                let journal = SaveSyncJournal::open(
                    directory.join("save-sync-journal.db"),
                    directory.join("save-sync-snapshots"),
                    scope.clone(),
                )?;
                let backups = journal.recorded_backup_paths()?;
                for file in &mut plan.unmatched {
                    if backups
                        .iter()
                        .any(|backup| *backup == scope.effective_saves_root.join(&file.path))
                    {
                        file.reason = "Local backup — not synced".into();
                    }
                }
                for slot in journal.slots()? {
                    if let Ok(Some(mapping)) = rules.tracked_mapping(
                        slot.rom_key.rom_id,
                        &slot.relative_path,
                        &scope.effective_saves_root,
                    ) {
                        if !plan
                            .report
                            .mappings
                            .iter()
                            .any(|m| m.relative_path == mapping.relative_path)
                        {
                            plan.report.mappings.push(mapping);
                        }
                    }
                }
                rommfs_core::save_sync::ensure_distinct(&plan.report.mappings)?;
            }
            unmatched = plan.unmatched;
            let mut consent = settings.load_files(&scope)?;
            if self.pending_save_sync_disable == Some(self.current_connection_epoch) {
                consent.enabled = false;
                settings.save_files(&scope, consent)?;
                self.pending_save_sync_disable = None;
            }
            self.save_sync_enabled = consent.enabled;
            self.save_sync_debounce_secs = consent.debounce_secs;
            self.save_sync_profile = Some(RetroBatGbProfile {
                version,
                effective_saves_root: scope.effective_saves_root.clone(),
            });
            self.save_sync_mapping_report = Some(plan.report);
            self.save_file_rules = Some(rules.clone());
            self.save_sync_scope = Some(scope.clone());
            if !identity.can_read_saves() || !identity.can_write_saves() {
                return Err(Error::Unsupported(
                    "Account needs assets.read and assets.write to synchronize saves".into(),
                ));
            }
            let (_, card_config, _) = self.memory_card_plan(&scope);
            let start_agent = self.save_sync_enabled || card_config.is_some();
            if start_agent {
                let directory = self.save_sync_storage_dir.as_ref().ok_or_else(|| {
                    Error::Unsupported("Save journal directory is unavailable".into())
                })?;
                let client = self
                    .client
                    .as_ref()
                    .ok_or_else(|| Error::Auth("Connect before synchronizing saves".into()))?;
                let epoch = self.current_save_sync_epoch;
                let enablement = if self.save_sync_enabled {
                    (self.save_sync_gate.open(epoch) && self.save_sync_gate.enabled_for(epoch))
                        .then(|| self.save_sync_gate.enablement(epoch))
                } else {
                    Some(SaveSyncEnablement::Test(Arc::new(
                        std::sync::atomic::AtomicBool::new(false),
                    )))
                };
                if let Some(enablement) = enablement {
                    self.save_sync_agent = Some(SaveSyncAgent::start_with_rules(
                        scope,
                        self.save_sync_mapping_report
                            .as_ref()
                            .map(|report| report.mappings.clone())
                            .unwrap_or_default(),
                        identity.clone(),
                        client.clone(),
                        directory.join("save-sync-journal.db"),
                        directory.join("save-sync-snapshots"),
                        self.save_sync_debounce_secs,
                        self.save_sync_session_id,
                        enablement,
                        self.sink.clone(),
                        Some(rules),
                        card_config,
                    )?);
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            problem = Some(e.to_string());
            self.save_sync_gate.close();
        }
        if self.save_sync_agent.is_none() {
            if let (Some(scope), Some(report), Some(identity)) = (
                self.save_sync_scope.as_ref(),
                self.save_sync_mapping_report.as_ref(),
                self.save_sync_identity.as_ref(),
            ) {
                if let Err(error) =
                    self.emit_persisted_save_sync_review(scope, &report.mappings, identity)
                {
                    problem = Some(format!("Saved review state could not be read: {error}"));
                }
            }
        }
        if let Some(e) = self.save_sync_runtime_problem.take() {
            problem = Some(e);
        }
        let available = self.save_file_rules.is_some()
            && self
                .save_sync_identity
                .as_ref()
                .is_some_and(|i| i.can_read_saves() && i.can_write_saves())
            && problem.is_none();
        if !self.save_sync_enabled {
            self.save_sync_gate.close();
        }
        let profile = self.save_sync_profile.as_ref();
        let report = self.save_sync_mapping_report.as_ref();
        self.sink.emit(AppEvent::SaveSyncUpdated {
            session_id: self.save_sync_session_id,
            server_id: self.server_id.clone(),
            candidates: discovery.candidates,
            skipped: discovery.skipped,
            selected_root: self
                .save_sync_scope
                .as_ref()
                .map(|s| s.installation_root.display().to_string()),
            documented_saves_root: profile.map(|p| p.effective_saves_root.display().to_string()),
            effective_saves_root: profile.map(|p| p.effective_saves_root.display().to_string()),
            profile_version: profile.map(|p| p.version.clone()),
            account_id: self.save_sync_identity.as_ref().map(|i| i.account_id),
            mapped_targets: report.map_or(0, |r| r.mappings.len()),
            catalogue_unmapped: unmatched.iter().filter(|file| file.mappable).count(),
            existing_saves: None,
            preview_saves: report
                .map(|r| {
                    r.mappings
                        .iter()
                        .take(5)
                        .map(|m| m.target_path.display().to_string())
                        .collect()
                })
                .unwrap_or_default(),
            available,
            selected_problem: problem,
            enabled: self.save_sync_enabled
                && self
                    .save_sync_gate
                    .enabled_for(self.current_save_sync_epoch),
            debounce_secs: self.save_sync_debounce_secs,
        });
        if let Some(completed_at) = scan_completed_at {
            self.sink.emit(AppEvent::SaveSyncScanFinished {
                session_id: self.save_sync_session_id,
                completed_at,
            });
        }
        self.sink.emit(AppEvent::SaveFileMappings {
            session_id: self.save_sync_session_id,
            games,
            unmatched,
        });
        self.refresh_memory_cards();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_actions_map_only_their_visible_immediate_parent_group() {
        let directory = tempfile::tempdir().unwrap();
        let (mut worker, _fixture, _events) = generic_worker(directory.path());
        populate_games(&mut worker);
        let root = worker
            .preferences
            .as_ref()
            .unwrap()
            .save_root
            .clone()
            .unwrap();
        std::fs::create_dir_all(root.join("gb/unrelated")).unwrap();
        for path in ["mystery.srm", "gb/mystery.srm", "gb/unrelated/other.srm"] {
            std::fs::write(root.join(path), b"save").unwrap();
        }
        worker.refresh_save_sync();
        worker.associate_save_folder(Path::new("gb"), 1);
        let settings = worker.save_sync_settings.as_ref().unwrap();
        let scope = worker.save_sync_scope.as_ref().unwrap();
        assert_eq!(
            settings.file_associations(scope).unwrap(),
            std::collections::BTreeMap::from([("gb/mystery.srm".into(), 1)])
        );
        worker.associate_save_folder(Path::new(""), 1);
        let settings = worker.save_sync_settings.as_ref().unwrap();
        let scope = worker.save_sync_scope.as_ref().unwrap();
        assert_eq!(
            settings.file_associations(scope).unwrap(),
            std::collections::BTreeMap::from([
                ("gb/mystery.srm".into(), 1),
                ("mystery.srm".into(), 1)
            ])
        );
    }
    fn generic_worker(
        directory: &Path,
    ) -> (
        Worker,
        rommfs_fixture::FixtureServer,
        mpsc::Receiver<AppEvent>,
    ) {
        let fixture = rommfs_fixture::FixtureServer::start();
        fixture.use_romm_save_store(42, 200);
        let (sink, events) = rommfs_core::events::channel();
        let mut worker = Worker::new(sink);
        let root = directory.join("saves");
        std::fs::create_dir(&root).unwrap();
        worker.preferences = Some(ConnectionPreferences {
            server: fixture.url().into(),
            account_id: 42,
            device_identifier: "test".into(),
            mount_root: None,
            integration: IntegrationMode::Generic,
            retrobat_root: None,
            start_at_login: true,
            save_root: Some(root),
            hide_owned_copies: true,
        });
        worker.server_id = Some(server_id_of(fixture.url()));
        worker.client = Some(Arc::new(RommClient::new(fixture.url()).unwrap()));
        worker.save_sync_identity = Some(rommfs_core::romm::SaveSyncIdentity {
            account_id: 42,
            scopes: vec!["assets.read".into(), "assets.write".into()],
        });
        worker.catalogue = Some(
            rommfs_core::catalogue::build_catalogue(&server_id_of(fixture.url()), &[], &[], |_| {})
                .unwrap(),
        );
        worker.source_snapshot = Some(CatalogueSnapshot {
            platforms: vec![],
            roms: vec![],
            refreshed_at: 0,
            full_refreshed_at: 0,
        });
        worker.save_sync_settings =
            Some(SaveSyncSettingsStore::open(directory.join("settings.db")).unwrap());
        worker.save_sync_storage_dir = Some(directory.join("private"));
        (worker, fixture, events)
    }
    #[test]
    fn ordinary_debounce_persists_under_the_new_consent_version_without_enabling_sync() {
        let directory = tempfile::tempdir().unwrap();
        let (mut worker, fixture, _events) = generic_worker(directory.path());
        worker.refresh_save_sync();
        assert!(worker.save_file_rules.is_some());
        worker.set_save_sync_debounce(13);
        let scope = worker.save_sync_scope.as_ref().unwrap();
        assert_eq!(
            worker
                .save_sync_settings
                .as_ref()
                .unwrap()
                .load_files(scope)
                .unwrap(),
            ConsentSettings {
                enabled: false,
                debounce_secs: 13
            }
        );
        assert_eq!(worker.save_sync_debounce_secs, 13);
        assert!(worker.save_sync_agent.is_none());
        assert_eq!(fixture.request_count("GET", "/api/saves"), 0);
    }
    #[test]
    fn failed_startup_mount_blocks_later_refresh_from_resuming_consented_sync() {
        let directory = tempfile::tempdir().unwrap();
        let (mut worker, fixture, _events) = generic_worker(directory.path());
        worker.refresh_save_sync();
        let scope = worker.save_sync_scope.clone().unwrap();
        worker
            .save_sync_settings
            .as_ref()
            .unwrap()
            .save_files(
                &scope,
                ConsentSettings {
                    enabled: true,
                    debounce_secs: 1,
                },
            )
            .unwrap();
        worker.startup_restoring = true;
        worker.startup_requires_mount = true;
        worker.refresh_save_sync();
        assert!(worker.save_sync_agent.is_none());
        worker.startup_restoring = false;
        // A later background catalogue completion also uses this refresh path.
        worker.refresh_save_sync();
        worker.refresh_save_sync();
        assert!(worker.save_sync_agent.is_none());
        assert!(!worker
            .save_sync_gate
            .enabled_for(worker.current_save_sync_epoch));
        assert_eq!(fixture.request_count("GET", "/api/saves"), 0);
    }

    #[test]
    fn completed_startup_mount_resumes_previously_consented_save_sync() {
        let directory = tempfile::tempdir().unwrap();
        let (mut worker, _fixture, events) = generic_worker(directory.path());
        worker.refresh_save_sync();
        let scope = worker.save_sync_scope.clone().unwrap();
        worker
            .save_sync_settings
            .as_ref()
            .unwrap()
            .save_files(
                &scope,
                ConsentSettings {
                    enabled: true,
                    debounce_secs: 1,
                },
            )
            .unwrap();
        worker.startup_requires_mount = true;
        worker.refresh_save_sync();
        assert!(worker.save_sync_agent.is_none());
        let root = directory.path().join("roms");
        worker.rom_cache_directory = Some(directory.path().join("cache"));
        let fs = worker
            .build_fs(
                &worker.client.clone().unwrap(),
                &root,
                Arc::new(NoopHydratedRemover),
            )
            .unwrap();
        let (tx, rx) = mpsc::channel();
        worker.mount_preparation = Some(MountPreparation {
            root: root.clone(),
            server_id: scope.server_id.clone(),
            path: root.display().to_string(),
            started: Instant::now(),
            active: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            result: rx,
            handle: std::thread::spawn(|| {}),
        });
        worker.poll_mount_preparation();
        assert!(worker.save_sync_agent.is_none(), "mount still pending");
        assert!(!events
            .try_iter()
            .any(|event| matches!(event, AppEvent::StartupPreference { error: Some(_), .. })));
        tx.send(Ok((
            fs,
            ActiveMount {
                root,
                stop_fn: Box::new(|| {}),
            },
        )))
        .unwrap();
        worker.poll_mount_preparation();
        assert!(worker.mount.is_some());
        assert!(!worker.startup_requires_mount);
        assert!(worker.save_sync_agent.is_some());
        assert!(worker
            .save_sync_gate
            .enabled_for(worker.current_save_sync_epoch));
        worker.stop_save_sync_agent();
        worker.stop_mount();
    }

    fn populate_games(worker: &mut Worker) {
        let platform = serde_json::from_value(serde_json::json!({
            "id": 1, "slug": "gb", "fs_slug": "gb", "name": "Game Boy", "rom_count": 2
        }))
        .unwrap();
        let roms = [(1, "Game.gb"), (2, "Other.gb")].into_iter().map(|(id, name)| {
            serde_json::from_value(serde_json::json!({
                "id": id, "platform_fs_slug": "gb", "platform_slug": "gb", "fs_name": name,
                "fs_size_bytes": 3, "has_simple_single_file": true,
                "has_nested_single_file": false, "has_multiple_files": false,
                "missing_from_fs": false, "is_physical": false, "updated_at": "now",
                "files": [{"id": id * 10, "file_name": name, "file_size_bytes": 3, "is_top_level": true}]
            })).unwrap()
        }).collect::<Vec<_>>();
        worker.catalogue = Some(
            rommfs_core::catalogue::build_catalogue(
                worker.server_id.as_deref().unwrap(),
                std::slice::from_ref(&platform),
                &roms,
                |_| {},
            )
            .unwrap(),
        );
        worker.source_snapshot = Some(CatalogueSnapshot {
            platforms: vec![platform],
            roms,
            refreshed_at: 0,
            full_refreshed_at: 0,
        });
    }

    fn stage_review(
        worker: &Worker,
    ) -> (SaveSyncJournal, rommfs_core::save_sync::SaveMapping, String) {
        let mapping = worker.save_sync_mapping_report.as_ref().unwrap().mappings[0].clone();
        let directory = worker.save_sync_storage_dir.as_ref().unwrap();
        let mut journal = SaveSyncJournal::open(
            directory.join("save-sync-journal.db"),
            directory.join("save-sync-snapshots"),
            worker.save_sync_scope.clone().unwrap(),
        )
        .unwrap();
        journal
            .reconcile_mappings(worker.save_sync_mapping_report.as_ref().unwrap())
            .unwrap();
        journal
            .confirm_mapping(&mapping.rom_key, &mapping.relative_path)
            .unwrap();
        let incoming = journal
            .stage_incoming(&mapping, "31", b"remote save", "conflict", None, None, None)
            .unwrap();
        (journal, mapping, incoming.id)
    }

    #[test]
    fn correcting_a_journaled_owner_survives_refresh_and_preserves_old_review_artifacts() {
        let directory = tempfile::tempdir().unwrap();
        let (mut worker, fixture, _events) = generic_worker(directory.path());
        populate_games(&mut worker);
        let path = Path::new("gb/Game.srm");
        let root = worker
            .preferences
            .as_ref()
            .unwrap()
            .save_root
            .clone()
            .unwrap();
        std::fs::create_dir(root.join("gb")).unwrap();
        std::fs::write(root.join(path), b"local save").unwrap();
        worker.refresh_save_sync();
        let (mut journal, old, incoming_id) = stage_review(&worker);
        worker.associate_save_file(path, 2);
        for _ in 0..2 {
            worker.refresh_save_sync();
            let report = worker
                .save_sync_mapping_report
                .as_ref()
                .expect("correction disabled the scope");
            assert_eq!(report.mappings.len(), 1);
            assert_eq!(report.mappings[0].rom_key.rom_id, 2);
            journal.reconcile_mappings(report).unwrap();
            assert!(
                !journal
                    .slot(&old.rom_key)
                    .unwrap()
                    .unwrap()
                    .candidate_available
            );
            assert!(journal
                .incoming_saves()
                .unwrap()
                .iter()
                .any(|record| record.id == incoming_id));
        }
        assert_eq!(std::fs::read(root.join(path)).unwrap(), b"local save");
        assert_eq!(fixture.request_count("GET", "/api/saves"), 0);
    }

    #[test]
    fn associate_save_folder_maps_a_directory_group_and_leaves_other_unmatched() {
        let directory = tempfile::tempdir().unwrap();
        let (mut worker, _fixture, _events) = generic_worker(directory.path());
        populate_games(&mut worker);
        let root = worker
            .preferences
            .as_ref()
            .unwrap()
            .save_root
            .clone()
            .unwrap();
        // A subdirectory whose name matches no game stem: all files inside
        // scan unmatched until the user maps the group.
        std::fs::create_dir_all(root.join("gb/sub")).unwrap();
        std::fs::write(root.join("gb/sub/a.srm"), b"a").unwrap();
        std::fs::write(root.join("gb/sub/b.srm"), b"b").unwrap();
        std::fs::write(root.join("gb/Other.srm"), b"o").unwrap();
        worker.refresh_save_sync();
        worker.associate_save_folder(Path::new("gb/sub"), 1);
        let scope = worker.save_sync_scope.clone().unwrap();
        let associations = worker
            .save_sync_settings
            .as_ref()
            .unwrap()
            .file_associations(&scope)
            .unwrap();
        assert_eq!(associations.len(), 2);
        assert!(associations.keys().any(|p| p.ends_with("a.srm")));
        assert!(associations.keys().any(|p| p.ends_with("b.srm")));
        worker.refresh_save_sync();
        assert!(worker
            .save_sync_mapping_report
            .as_ref()
            .unwrap()
            .mappings
            .iter()
            .any(|m| m.rom_key.rom_id == 1 && m.relative_path.ends_with("a.srm")));
        // An empty/absent group keeps existing associations; nothing is
        // deleted and the failure is reported, not silent.
        worker.associate_save_folder(Path::new("missing"), 1);
        let associations = worker
            .save_sync_settings
            .as_ref()
            .unwrap()
            .file_associations(&scope)
            .unwrap();
        assert_eq!(associations.len(), 2);
    }

    #[test]
    fn paused_refresh_and_reopening_restore_the_persisted_review_queue_without_network_sync() {
        let directory = tempfile::tempdir().unwrap();
        let (mut worker, fixture, events) = generic_worker(directory.path());
        populate_games(&mut worker);
        let root = worker
            .preferences
            .as_ref()
            .unwrap()
            .save_root
            .clone()
            .unwrap();
        std::fs::create_dir(root.join("gb")).unwrap();
        std::fs::write(root.join("gb/Game.srm"), b"local save").unwrap();
        worker.refresh_save_sync();
        let (journal, _, incoming_id) = stage_review(&worker);
        drop(journal);
        let mut state = UiState::new(100);
        for _ in 0..2 {
            events.try_iter().for_each(drop);
            worker.set_save_sync_enabled(false);
            assert!(worker.save_sync_agent.is_none());
            for event in events.try_iter() {
                state.apply_event(&event);
            }
            let queue = state
                .save_sync_queue
                .as_ref()
                .expect("paused refresh lost its saved review queue");
            assert_eq!(queue.session_id, worker.save_sync_session_id);
            assert!(queue
                .incoming
                .iter()
                .any(|record| record.incoming_id == incoming_id));
        }
        let exported = directory.path().join("exported.bin");
        worker.export_save_sync_incoming(worker.save_sync_session_id, &incoming_id, &exported);
        assert_eq!(std::fs::read(exported).unwrap(), b"remote save");
        assert_eq!(fixture.request_count("GET", "/api/saves"), 0);
        // Reopen the same settings and journal with consent still disabled.
        let (sink, reopened_events) = rommfs_core::events::channel();
        let mut reopened = Worker::new(sink);
        reopened.preferences = worker.preferences.clone();
        reopened.server_id = worker.server_id.clone();
        reopened.save_sync_identity = worker.save_sync_identity.clone();
        reopened.save_sync_settings =
            Some(SaveSyncSettingsStore::open(directory.path().join("settings.db")).unwrap());
        reopened.save_sync_storage_dir = worker.save_sync_storage_dir.clone();
        populate_games(&mut reopened);
        reopened.refresh_save_sync();
        assert!(reopened.save_sync_agent.is_none());
        assert!(reopened_events.try_iter().any(|event| matches!(event,
            AppEvent::SaveSyncQueueUpdated(queue) if queue.incoming.iter().any(|record| record.incoming_id == incoming_id))));
    }
}
