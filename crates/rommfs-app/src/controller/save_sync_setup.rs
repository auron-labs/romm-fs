//! Save-sync discovery, consent, preview, and export setup.
use super::*;

impl Worker {
    pub(super) fn refresh_save_sync(&mut self) {
        if self.startup_restoring || (self.startup_requires_mount && self.mount.is_none()) {
            return;
        }
        self.refresh_save_sync_from(save_sync_discovery_input());
    }

    pub(super) fn refresh_save_sync_from(&mut self, input: DiscoveryInput) {
        self.advance_save_sync_session();
        self.refresh_save_sync_in_session(input);
    }

    /// A check keeps the selected scope and its last known review visible.
    /// Its new worker session still rejects completions from the drained actor.
    pub(super) fn check_save_sync(&mut self) {
        if self.startup_restoring || (self.startup_requires_mount && self.mount.is_none()) {
            return;
        }
        self.save_sync_session_id = self.save_sync_session_id.saturating_add(1);
        self.sink.emit(AppEvent::SaveSyncCheckStarted {
            session_id: self.save_sync_session_id,
        });
        self.refresh_save_sync_in_session(save_sync_discovery_input());
    }

    fn refresh_save_sync_in_session(&mut self, input: DiscoveryInput) {
        self.stop_save_sync_agent();
        if self.preferences.is_some() {
            self.refresh_save_files(input);
            return;
        }
        let discovery = discover_installations(input);
        let mut selected_problem = self.save_sync_settings_problem.clone();
        let mut selected_root = None;
        let mut documented_saves_root = None;
        let mut effective_saves_root = None;
        let mut profile_version = None;
        let mut account_id = None;
        let mut mapped_targets = 0;
        let mut catalogue_unmapped = 0;
        let mut existing_saves = None;
        let mut preview_saves = Vec::new();
        let mut profile = None;
        let mut mapping_report = None;

        if let Some(root) = self.save_sync_selected_root.as_deref() {
            selected_root = Some(root.display().to_string());
            match validate_installation(root) {
                Err(problem) => selected_problem = Some(selected_installation_problem(problem)),
                Ok(info) => {
                    selected_root = Some(info.install_root.display().to_string());
                    documented_saves_root = Some(info.documented_saves_root.display().to_string());
                    let visible_gb_names = self
                        .save_sync_catalogue
                        .as_ref()
                        .map(|catalogue| {
                            catalogue
                                .entries
                                .iter()
                                .filter(|entry| entry.platform_dir.eq_ignore_ascii_case("gb"))
                                .map(|entry| entry.file_name.clone())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    match resolve_retrobat_gb_profile(root, &visible_gb_names) {
                        Err(error) => selected_problem = Some(error.to_string()),
                        Ok(resolved) => {
                            effective_saves_root =
                                Some(resolved.effective_saves_root.display().to_string());
                            profile_version = Some(resolved.version.clone());
                            if let Some(catalogue) = self.save_sync_catalogue.as_ref() {
                                match map_catalogue(catalogue, &resolved.effective_saves_root) {
                                    Ok(report) => {
                                        mapped_targets = report.supported_count();
                                        catalogue_unmapped = report.unmapped_count();
                                        existing_saves =
                                            Some(rommfs_core::save_sync::preview_existing_saves(
                                                &resolved.effective_saves_root,
                                                &report,
                                            ));
                                        preview_saves = report
                                            .mappings
                                            .iter()
                                            .take(5)
                                            .map(|mapping| {
                                                mapping.target_path.display().to_string()
                                            })
                                            .collect();
                                        mapping_report = Some(report);
                                    }
                                    Err(error) => selected_problem = Some(error.to_string()),
                                }
                            }
                            profile = Some(resolved);
                        }
                    }
                }
            }
        }

        if let Some(identity) = &self.save_sync_identity {
            account_id = Some(identity.account_id);
        } else if self.client.is_some() {
            selected_problem.get_or_insert_with(|| {
                "Save sync needs a verified authenticated account with me.read, assets.read, and assets.write.".into()
            });
        }

        let scope = match (
            self.server_id.as_ref(),
            self.save_sync_identity.as_ref(),
            self.save_sync_selected_root.as_ref(),
            profile.as_ref(),
        ) {
            (Some(server_id), Some(identity), Some(installation_root), Some(profile)) => {
                Some(SaveSyncScope {
                    server_id: server_id.clone(),
                    account_id: identity.account_id.to_string(),
                    installation_root: installation_root.clone(),
                    effective_saves_root: profile.effective_saves_root.clone(),
                })
            }
            _ => None,
        };
        let consent = match (self.save_sync_settings.as_ref(), scope.as_ref()) {
            (Some(settings), Some(scope)) => match settings.load(scope).and_then(|mut consent| {
                if self.pending_save_sync_disable == Some(self.current_connection_epoch) {
                    consent.enabled = false;
                    settings.save(scope, consent)?;
                    self.pending_save_sync_disable = None;
                }
                Ok(consent)
            }) {
                Ok(consent) => Some(consent),
                Err(error) => {
                    selected_problem = Some(format!("Consent settings could not be read: {error}"));
                    None
                }
            },
            _ => None,
        };
        self.save_sync_scope = scope.clone();
        self.save_sync_profile = profile;
        self.save_sync_mapping_report = mapping_report;
        if let Some(consent) = consent {
            self.save_sync_enabled = consent.enabled;
            self.save_sync_debounce_secs = consent.debounce_secs;
        } else {
            self.save_sync_enabled = false;
        }

        let save_permissions = self
            .save_sync_identity
            .as_ref()
            .is_some_and(|identity| identity.can_read_saves() && identity.can_write_saves());
        if self.save_sync_identity.is_some() && !save_permissions {
            selected_problem = Some(
                "The authenticated account lacks assets.read or assets.write; ROM access is unchanged.".into(),
            );
        }
        let available = scope.is_some()
            && save_permissions
            && self.save_sync_settings.is_some()
            && self
                .save_sync_mapping_report
                .as_ref()
                .is_some_and(|report| report.supported_count() > 0);

        #[cfg(windows)]
        let available = available
            && scope.as_ref().is_some_and(|scope| {
                match crate::save_sync_agent::check_save_root_writable(&scope.effective_saves_root)
                {
                    Ok(()) => true,
                    Err(error) => {
                        selected_problem = Some(error.to_string());
                        false
                    }
                }
            });

        let (_, card_config, _) = match scope.as_ref() {
            Some(scope) => self.memory_card_plan(scope),
            None => (Vec::new(), None, None),
        };
        let save_start = self.save_sync_enabled
            && available
            && self
                .save_sync_mapping_report
                .as_ref()
                .is_some_and(|report| !report.mappings.is_empty());
        if (save_start || card_config.is_some()) && scope.is_some() {
            if save_start && self.save_sync_storage_dir.is_none() {
                self.save_sync_gate.close();
                selected_problem = Some("No persistent save-sync directory is available.".into());
            } else if let (Some(scope), Some(client), Some(identity), Some(settings_dir)) = (
                scope.clone(),
                self.client.clone(),
                self.save_sync_identity.clone(),
                self.save_sync_storage_dir.clone(),
            ) {
                let mappings = self
                    .save_sync_mapping_report
                    .as_ref()
                    .map(|report| report.mappings.clone())
                    .unwrap_or_default();
                let epoch = self.current_save_sync_epoch;
                let enablement = if save_start {
                    (self.save_sync_gate.open(epoch) && self.save_sync_gate.enabled_for(epoch))
                        .then(|| self.save_sync_gate.enablement(epoch))
                } else {
                    Some(SaveSyncEnablement::Test(Arc::new(
                        std::sync::atomic::AtomicBool::new(false),
                    )))
                };
                if let Some(enablement) = enablement {
                    match SaveSyncAgent::start(
                        scope,
                        mappings,
                        identity,
                        Arc::clone(&client),
                        settings_dir.join("save-sync-journal.db"),
                        settings_dir.join("save-sync-snapshots"),
                        self.save_sync_debounce_secs,
                        self.save_sync_session_id,
                        enablement,
                        self.sink.clone(),
                        card_config,
                    ) {
                        Ok(agent) => self.save_sync_agent = Some(agent),
                        Err(error) => {
                            self.save_sync_gate.close();
                            selected_problem =
                                Some(format!("Save watcher could not start: {error}"));
                        }
                    }
                }
            }
        } else {
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
                    selected_problem =
                        Some(format!("Saved review state could not be read: {error}"));
                }
            }
        }

        if let Some(problem) = self.save_sync_runtime_problem.take() {
            selected_problem = Some(problem);
        }
        if let Some(settings_problem) = &self.save_sync_settings_problem {
            selected_problem = Some(match selected_problem {
                Some(problem) => format!("{problem} {settings_problem}"),
                None => settings_problem.clone(),
            });
        }
        self.sink.emit(AppEvent::SaveSyncUpdated {
            session_id: self.save_sync_session_id,
            server_id: self.server_id.clone(),
            candidates: discovery.candidates,
            skipped: discovery.skipped,
            selected_root,
            documented_saves_root,
            effective_saves_root,
            profile_version,
            account_id,
            mapped_targets,
            catalogue_unmapped,
            existing_saves,
            preview_saves,
            available,
            selected_problem,
            enabled: self.save_sync_enabled
                && self
                    .save_sync_gate
                    .enabled_for(self.current_save_sync_epoch),
            debounce_secs: self.save_sync_debounce_secs,
        });
        self.refresh_memory_cards();
    }

    pub(super) fn select_save_sync_installation(&mut self, path: &Path) {
        if let Some(prefs) = self.preferences.as_mut() {
            prefs.save_root = None;
            // Selecting a save installation must not change the mounted
            // library's integration capabilities or metadata destination.
            if let Some(settings) = &self.connection_settings {
                if let Err(e) = settings.save(prefs) {
                    self.save_sync_runtime_problem = Some(e.to_string());
                }
            }
        }
        self.stop_save_sync_agent();
        self.save_sync_enabled = false;
        self.save_sync_scope = None;
        // Explicit selection is remembered even while missing/invalid, so
        // refresh never changes to a different install on the user's behalf.
        if let Some(settings) = &self.save_sync_settings {
            if let Err(error) = settings.save_selected_installation(path) {
                self.save_sync_settings_problem = Some(format!("Selection was not saved: {error}"));
                self.log(
                    Level::Error,
                    "save-sync",
                    format!("Selection was not saved: {error}"),
                );
                self.refresh_save_sync();
                return;
            }
            self.save_sync_settings_problem = None;
        } else {
            self.save_sync_settings_problem = Some(
                "Selection cannot be persisted without the application settings database.".into(),
            );
        }
        self.save_sync_selected_root = Some(path.to_path_buf());
        self.refresh_save_sync();
    }

    pub(super) fn set_save_sync_enabled(&mut self, enabled: bool) {
        if !enabled {
            self.pending_save_sync_disable = Some(self.current_connection_epoch);
        }
        if enabled {
            let ready = self.save_sync_scope.is_some()
                && self.save_sync_profile.is_some()
                && self.save_sync_identity.as_ref().is_some_and(|identity| {
                    identity.can_read_saves() && identity.can_write_saves()
                })
                && self
                    .save_sync_mapping_report
                    .as_ref()
                    .is_some_and(|report| {
                        report.supported_count() > 0 || self.save_file_rules.is_some()
                    });
            if !ready {
                self.save_sync_runtime_problem = Some(
                    "Enable refused: verify the account, save folder, and game mappings first."
                        .into(),
                );
                self.refresh_save_sync();
                return;
            }
            #[cfg(windows)]
            if let Some(scope) = &self.save_sync_scope {
                if let Err(error) =
                    crate::save_sync_agent::check_save_root_writable(&scope.effective_saves_root)
                {
                    self.save_sync_runtime_problem = Some(format!("Enable refused: {error}"));
                    self.refresh_save_sync();
                    return;
                }
            }
        }
        if let (Some(settings), Some(scope)) = (
            self.save_sync_settings.as_ref(),
            self.save_sync_scope.as_ref(),
        ) {
            let settings_value = ConsentSettings {
                enabled,
                debounce_secs: self.save_sync_debounce_secs,
            };
            let saved = if self.save_file_rules.is_some() {
                settings.save_files(scope, settings_value)
            } else {
                settings.save(scope, settings_value)
            };
            if let Err(error) = saved {
                self.save_sync_runtime_problem =
                    Some(format!("Consent could not be saved: {error}"));
                self.refresh_save_sync();
                return;
            }
            self.pending_save_sync_disable = None;
        } else if enabled {
            self.save_sync_runtime_problem = Some("Consent scope is unavailable.".into());
            self.refresh_save_sync();
            return;
        }
        self.save_sync_enabled = enabled;
        self.check_save_sync();
    }

    pub(super) fn set_save_sync_debounce(&mut self, seconds: u32) {
        if !(1..=3600).contains(&seconds) {
            self.save_sync_runtime_problem =
                Some("Debounce must be a whole number from 1 to 3600 seconds.".into());
            self.refresh_save_sync();
            return;
        }
        self.save_sync_debounce_secs = seconds;
        if let (Some(settings), Some(scope)) = (
            self.save_sync_settings.as_ref(),
            self.save_sync_scope.as_ref(),
        ) {
            let consent = ConsentSettings {
                enabled: self.save_sync_enabled,
                debounce_secs: seconds,
            };
            let saved = if self.save_file_rules.is_some() {
                settings.save_files(scope, consent)
            } else {
                settings.save(scope, consent)
            };
            if let Err(error) = saved {
                self.save_sync_runtime_problem =
                    Some(format!("Debounce could not be saved: {error}"));
            }
        }
        self.refresh_save_sync();
    }

    pub(super) fn export_save_sync_incoming(
        &mut self,
        session_id: u64,
        incoming_id: &str,
        destination: &Path,
    ) {
        if session_id != self.save_sync_session_id {
            return;
        }
        let result: Result<()> = (|| {
            if let Some(agent) = self.save_sync_agent.as_ref() {
                return agent.export_incoming(incoming_id, destination.to_path_buf());
            }
            let scope = self.save_sync_scope.as_ref().ok_or_else(|| {
                Error::Unsupported("save-sync scope is unavailable for export".into())
            })?;
            let mappings = &self
                .save_sync_mapping_report
                .as_ref()
                .ok_or_else(|| {
                    Error::Unsupported("save mappings are unavailable for export".into())
                })?
                .mappings;
            let identity = self.save_sync_identity.as_ref().ok_or_else(|| {
                Error::Unsupported("verified account identity is unavailable for export".into())
            })?;
            let settings_dir = self.save_sync_storage_dir.clone().ok_or_else(|| {
                Error::Unsupported("persistent save-sync directory is unavailable".into())
            })?;
            crate::save_sync_agent::export_pending_incoming(
                scope,
                mappings,
                identity,
                &settings_dir.join("save-sync-journal.db"),
                &settings_dir.join("save-sync-snapshots"),
                incoming_id,
                destination,
            )
        })();
        let (destination, error) = match result {
            Ok(()) => (Some(destination.display().to_string()), None),
            Err(error) => (None, Some(error.to_string())),
        };
        if let Some(error) = error.as_ref() {
            self.log(
                Level::Error,
                "save-sync",
                format!("incoming save export failed: {error}"),
            );
        }
        self.sink.emit(AppEvent::SaveSyncExportFinished {
            session_id,
            incoming_id: incoming_id.to_owned(),
            destination,
            error,
        });
    }

    pub(super) fn resolve_save_conflict(
        &mut self,
        session_id: u64,
        rom_id: i64,
        file_id: i64,
        local_hash: Option<&str>,
        incoming_id: Option<&str>,
    ) {
        if session_id != self.save_sync_session_id {
            return;
        }
        let result = (|| {
            let scope = self.save_sync_scope.as_ref().ok_or_else(|| {
                Error::Unsupported("save-sync scope is unavailable for conflict recovery".into())
            })?;
            let agent = self.save_sync_agent.as_ref().ok_or_else(|| {
                Error::Unsupported("save-sync agent is unavailable for conflict recovery".into())
            })?;
            agent.resolve_conflict(
                session_id,
                rommfs_core::catalogue::RomKey {
                    server_id: scope.server_id.clone(),
                    rom_id,
                    file_id,
                },
                local_hash.map(ToOwned::to_owned),
                incoming_id.map(ToOwned::to_owned),
            )
        })();
        match result {
            Ok(()) => self.save_sync_runtime_problem = None,
            Err(error) => {
                self.save_sync_runtime_problem = Some(error.to_string());
                self.log(
                    Level::Warn,
                    "save-sync",
                    format!("save conflict recovery failed: {error}"),
                );
            }
        }
        self.refresh_save_sync();
    }

    pub(super) fn advance_save_sync_session(&mut self) {
        self.save_sync_session_id = self.save_sync_session_id.saturating_add(1);
        self.sink.emit(AppEvent::SaveSyncSessionChanged {
            session_id: self.save_sync_session_id,
        });
    }

    pub(super) fn emit_persisted_save_sync_review(
        &self,
        scope: &SaveSyncScope,
        mappings: &[rommfs_core::save_sync::SaveMapping],
        _identity: &rommfs_core::romm::SaveSyncIdentity,
    ) -> Result<()> {
        let Some(settings_dir) = &self.save_sync_storage_dir else {
            return Ok(());
        };
        let journal_path = settings_dir.join("save-sync-journal.db");
        if !journal_path.is_file() {
            return Ok(());
        }
        let journal = SaveSyncJournal::open(
            &journal_path,
            settings_dir.join("save-sync-snapshots"),
            scope.clone(),
        )?;
        let incoming_records = journal.incoming_saves()?;
        let incoming = incoming_records
            .iter()
            .map(|record| rommfs_core::events::SaveSyncIncomingStatus {
                incoming_id: record.id.clone(),
                rom_id: record.rom_key.rom_id,
                rom_name: mappings
                    .iter()
                    .find(|mapping| mapping.rom_key == record.rom_key)
                    .map(|mapping| mapping.visible_rom_name.clone())
                    .unwrap_or_else(|| format!("ROM {}", record.rom_key.rom_id)),
                remote_id: record.remote_id.clone(),
                content_hash: record.content_hash.clone(),
                reason: record.reason.clone(),
                state: record.state.clone(),
            })
            .collect::<Vec<_>>();
        let mut games = Vec::new();
        let mut attention_games = 0;
        let mut failure = None;
        for mapping in mappings {
            let Some(slot) = journal.slot(&mapping.rom_key)? else {
                continue;
            };
            if slot.needs_attention {
                attention_games += 1;
            }
            if failure.is_none() {
                failure = slot.last_failure.clone();
            }
            games.push(SaveSyncGameStatus {
                work: if slot.needs_attention {
                    rommfs_core::events::SaveSyncWork::NeedsReview
                } else {
                    rommfs_core::events::SaveSyncWork::NotChecked
                },
                rom_id: mapping.rom_key.rom_id,
                file_id: mapping.rom_key.file_id,
                local_path: mapping.target_path.display().to_string(),
                rom_name: mapping.visible_rom_name.clone(),
                local_hash: slot.current_local_hash,
                remote_id: slot.remote_slot_id,
                remote_hash: slot.remote_baseline_hash,
                incoming_ids: incoming_records
                    .iter()
                    .filter(|record| record.rom_key == mapping.rom_key)
                    .map(|record| record.id.clone())
                    .collect(),
                issue: slot.last_failure.or_else(|| {
                    slot.local_removed.then(|| {
                        "the tracked local save was removed; the remote copy was preserved".into()
                    })
                }),
                installed_incoming: false,
            });
        }
        let pending_outbound = journal
            .snapshots()?
            .iter()
            .filter(|snapshot| {
                matches!(
                    snapshot.state,
                    SnapshotState::Ready
                        | SnapshotState::RemoteInFlight
                        | SnapshotState::RemoteAmbiguous
                )
            })
            .count()
            + journal.dirty_mappings()?.len();
        self.sink
            .emit(AppEvent::SaveSyncQueueUpdated(SaveSyncQueueStatus {
                session_id: self.save_sync_session_id,
                mapped_games: mappings.len(),
                // Persisted baselines are useful review context, but they are
                // not a fresh inventory verification for this session.
                reconciled_games: 0,
                pending_outbound,
                pending_incoming: 0,
                attention_games,
                network_paused: false,
                authentication_required: false,
                actor_failed: false,
                failure,
                games,
                incoming,
            }));
        Ok(())
    }

    pub(super) fn stop_save_sync_agent(&mut self) {
        self.save_sync_gate.close();
        if let Some(mut agent) = self.save_sync_agent.take() {
            agent.cancel();
            self.draining.push(std::thread::spawn(move || agent.stop()));
        }
    }
}
