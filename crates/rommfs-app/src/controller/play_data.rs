//! Favourites and play-history sync: consent, availability, and the debounced
//! gamelist watch that re-runs metadata after RetroBat writes play data.
use super::*;
use rommfs_core::romm::PlayDataPermissions;

/// How often the linked gamelists are checked for changes while a mount is up.
pub(super) const PLAY_WATCH_INTERVAL: Duration = Duration::from_secs(5);

impl Worker {
    /// Publish consent and permission state; the watch follows the consent.
    pub(super) fn refresh_play_data(&mut self) {
        let available = self
            .save_sync_identity
            .as_ref()
            .is_some_and(|identity| PlayDataPermissions::from_scopes(&identity.scopes).can_sync());
        let account = self.preferences.as_ref().map(|p| p.account_id.to_string());
        let mut problem = None;
        let enabled = match (
            self.save_sync_settings.as_ref(),
            self.server_id.as_deref(),
            account.as_deref(),
            available,
        ) {
            (Some(settings), Some(server), Some(account), true) => {
                match settings.load_play_data(server, account) {
                    Ok(enabled) => enabled,
                    Err(error) => {
                        problem = Some(format!("Play-data consent could not be read: {error}"));
                        false
                    }
                }
            }
            _ => false,
        };
        self.play_data_available = available;
        self.play_data_enabled = enabled;
        self.play_data_permission_denied = false;
        if problem.is_some() {
            self.play_data_problem = problem.clone();
        }
        self.sync_play_watch();
        self.emit_play_data(problem);
    }

    pub(super) fn set_play_data_enabled(&mut self, enabled: bool) {
        if enabled && !self.play_data_available {
            self.play_data_permission_denied = false;
            self.play_data_problem =
                Some("Reconnect to grant the favourites and play-history permissions.".into());
            self.play_data_enabled = false;
            self.sync_play_watch();
            self.emit_play_data(self.play_data_problem.clone());
            return;
        }
        let saved = match (
            self.save_sync_settings.as_ref(),
            self.server_id.as_deref(),
            self.preferences.as_ref().map(|p| p.account_id.to_string()),
        ) {
            (Some(settings), Some(server), Some(account)) => {
                settings.save_play_data(server, &account, enabled)
            }
            _ => Err(Error::Unsupported(
                "Connect and verify an account first.".into(),
            )),
        };
        match saved {
            Ok(()) => {
                self.play_data_enabled = enabled;
                self.play_data_problem = None;
            }
            Err(error) => {
                self.play_data_problem = Some(format!("Play-data consent was not saved: {error}"));
                self.log(
                    Level::Error,
                    "play-data",
                    self.play_data_problem.clone().unwrap_or_default(),
                );
            }
        }
        self.sync_play_watch();
        self.emit_play_data(None);
    }

    fn emit_play_data(&self, problem: Option<String>) {
        self.sink.emit(AppEvent::PlayDataUpdated {
            available: self.play_data_available,
            enabled: self.play_data_enabled,
            permission_denied: self.play_data_permission_denied,
            problem,
        });
    }

    /// Start or stop the gamelist watch to match consent and the live session.
    fn sync_play_watch(&mut self) {
        let watching = self.play_data_enabled
            && self.mount.is_some()
            && self.preferences.as_ref().is_some_and(|p| {
                p.integration == IntegrationMode::RetroBat && p.retrobat_root.is_some()
            });
        if watching {
            if self.play_watch.is_none() {
                self.play_watch = Some(self.play_watch_snapshot());
                self.play_watch_checked = Instant::now();
            }
        } else {
            self.play_watch = None;
        }
    }

    fn play_watch_snapshot(&self) -> HashMap<String, Option<std::time::SystemTime>> {
        let mut snapshot = HashMap::new();
        let (Some(installation), Some(root), Some(server)) = (
            self.preferences
                .as_ref()
                .and_then(|p| p.retrobat_root.clone()),
            self.mount.as_ref().map(|mount| mount.root.clone()),
            self.server_id.clone(),
        ) else {
            return snapshot;
        };
        let Ok(systems) = rommfs_core::retrobat::linked_systems(&installation, &root, &server)
        else {
            return snapshot;
        };
        for system in systems {
            let path = installation.join("roms").join(&system).join("gamelist.xml");
            snapshot.insert(
                system,
                std::fs::metadata(&path).and_then(|m| m.modified()).ok(),
            );
        }
        snapshot
    }

    /// True when a publication should reconcile play data.
    pub(super) fn play_data_ready(&self) -> bool {
        self.play_data_enabled && self.play_data_available
    }

    /// Re-run metadata once a linked gamelist changes while RetroBat is closed.
    pub(super) fn poll_play_watch(&mut self) {
        // Mounts and integrations come and go; keep the watch in step.
        self.sync_play_watch();
        if self.play_watch.is_none() || self.play_watch_checked.elapsed() < PLAY_WATCH_INTERVAL {
            return;
        }
        self.play_watch_checked = Instant::now();
        if self.metadata_job.is_some()
            || self.metadata_ready.is_some()
            || self.metadata_publish.is_some()
        {
            return;
        }
        let current = self.play_watch_snapshot();
        if Some(&current) != self.play_watch.as_ref() {
            self.play_watch = Some(current);
            self.queue_metadata();
        }
    }
}
