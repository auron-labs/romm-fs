//! Memory-card discovery, per-folder consent, and the plan the save agent owns.
use super::*;

impl Worker {
    /// Discover card folders, apply per-folder consent, and build the config the
    /// save agent owns. A card problem never fails the save refresh.
    pub(super) fn memory_card_plan(
        &self,
        scope: &SaveSyncScope,
    ) -> (
        Vec<MemoryCardFolderStatus>,
        Option<MemoryCardConfig>,
        Option<String>,
    ) {
        let mut folders = Vec::new();
        let mut enabled = Vec::new();
        let mut problem = None;
        match discover_card_folders(&scope.effective_saves_root) {
            Ok(discovered) => {
                for folder in discovered {
                    let on = self.save_sync_settings.as_ref().is_some_and(|settings| {
                        settings
                            .load_card_folder(scope, folder.emulator.name(), &folder.relative_dir)
                            .unwrap_or(false)
                    });
                    folders.push(MemoryCardFolderStatus {
                        emulator: folder.emulator.name().to_owned(),
                        label: folder.label.to_owned(),
                        relative_dir: folder.relative_dir.to_string_lossy().replace('\\', "/"),
                        cards: folder.cards.len(),
                        enabled: on,
                    });
                    if on {
                        enabled.push(folder);
                    }
                }
            }
            Err(error) => problem = Some(error.to_string()),
        }
        let available = self
            .save_sync_identity
            .as_ref()
            .is_some_and(|identity| identity.can_read_saves() && identity.can_write_saves());
        let config = match (
            available,
            self.save_sync_identity.clone(),
            self.client.clone(),
            self.save_sync_storage_dir.clone(),
        ) {
            (true, Some(identity), Some(client), Some(directory)) if !enabled.is_empty() => {
                Some(MemoryCardConfig {
                    scope: scope.clone(),
                    folders: enabled,
                    identity,
                    client,
                    journal_path: directory.join("save-sync-journal.db"),
                    debounce_secs: self.save_sync_debounce_secs,
                    session_id: self.save_sync_session_id,
                })
            }
            _ => None,
        };
        (folders, config, problem)
    }

    /// Emit the card folder status for the current scope. The card agent itself
    /// is started with the save agent, so nothing is started here.
    pub(super) fn refresh_memory_cards(&mut self) {
        let (folders, _, problem) = match self.save_sync_scope.clone() {
            Some(scope) => self.memory_card_plan(&scope),
            None => (Vec::new(), None, None),
        };
        let available = self
            .save_sync_identity
            .as_ref()
            .is_some_and(|identity| identity.can_read_saves() && identity.can_write_saves());
        self.sink.emit(AppEvent::MemoryCardsUpdated {
            session_id: self.save_sync_session_id,
            available,
            folders,
            conflicts: Vec::new(),
            problem,
        });
    }

    /// Turn one discovered card folder on or off. Consent is explicit and off
    /// by default; it is scoped to this server, account, installation, and root.
    /// The refresh restarts the save agent, which owns the card thread.
    pub(super) fn set_memory_card_folder_enabled(
        &mut self,
        emulator: &str,
        relative_dir: &Path,
        enabled: bool,
    ) {
        let result = (|| -> Result<()> {
            let scope = self
                .save_sync_scope
                .as_ref()
                .ok_or_else(|| Error::Unsupported("Choose a save folder first".into()))?;
            self.save_sync_settings
                .as_ref()
                .ok_or_else(|| Error::Unsupported("Save settings are unavailable".into()))?
                .save_card_folder(scope, emulator, relative_dir, enabled)
        })();
        if let Err(error) = result {
            self.save_sync_runtime_problem = Some(format!("Memory-card consent failed: {error}"));
        }
        self.refresh_save_sync();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save_sync_agent::{reconcile_once, MemoryCardConfig, MemoryCardReport};
    use rommfs_core::romm::SaveSyncIdentity;
    use rommfs_core::save_sync::{discover_card_folders, write_card_archive, MemoryCardJournal};
    use rommfs_fixture::FixtureServer;
    use std::io::Cursor;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn save_identity() -> SaveSyncIdentity {
        SaveSyncIdentity {
            account_id: 42,
            scopes: vec!["assets.read".into(), "assets.write".into()],
        }
    }

    fn drain_memory_cards(
        events: &mpsc::Receiver<AppEvent>,
    ) -> Option<Vec<MemoryCardFolderStatus>> {
        events.try_iter().find_map(|event| match event {
            AppEvent::MemoryCardsUpdated { folders, .. } => Some(folders),
            _ => None,
        })
    }

    /// A zip holding one card file named after the card.
    fn card_archive(name: &str, bytes: &[u8]) -> Vec<u8> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        let mut cursor = Cursor::new(Vec::new());
        write_card_archive(&path, &mut cursor).unwrap();
        cursor.into_inner()
    }

    #[test]
    fn memory_card_consent_is_off_by_default_and_toggles_per_folder() {
        let directory = tempfile::tempdir().unwrap();
        let fixture = FixtureServer::start();
        fixture.use_romm_memory_card_store(42);
        let (sink, events) = rommfs_core::events::channel();
        let mut worker = Worker::new(sink);
        let saves = directory.path().join("saves");
        std::fs::create_dir_all(saves.join("psx/duckstation/memcards")).unwrap();
        std::fs::write(saves.join("psx/duckstation/memcards/Mcd001.mcd"), b"card").unwrap();
        worker.preferences = Some(ConnectionPreferences {
            server: fixture.url().into(),
            account_id: 42,
            device_identifier: "test".into(),
            mount_root: None,
            integration: IntegrationMode::Generic,
            retrobat_root: None,
            start_at_login: true,
            save_root: Some(saves.clone()),
            hide_owned_copies: true,
        });
        worker.server_id = Some(server_id_of(fixture.url()));
        worker.client = Some(Arc::new(RommClient::new(fixture.url()).unwrap()));
        worker.save_sync_identity = Some(save_identity());
        worker.catalogue =
            Some(build_catalogue(&server_id_of(fixture.url()), &[], &[], |_| {}).unwrap());
        worker.source_snapshot = Some(CatalogueSnapshot {
            platforms: vec![],
            roms: vec![],
            refreshed_at: 0,
            full_refreshed_at: 0,
        });
        worker.save_sync_settings =
            Some(SaveSyncSettingsStore::open(directory.path().join("settings.db")).unwrap());
        worker.save_sync_storage_dir = Some(directory.path().join("private"));

        worker.refresh_save_sync();
        let folders = drain_memory_cards(&events).expect("memory-card folders were emitted");
        assert!(folders.iter().any(|folder| {
            folder.relative_dir == "psx/duckstation/memcards"
                && folder.emulator == "duckstation"
                && !folder.enabled
                && folder.cards == 1
        }));
        assert!(worker.save_sync_agent.is_none());

        let folder = Path::new("psx/duckstation/memcards");
        worker.set_memory_card_folder_enabled("duckstation", folder, true);
        let scope = worker.save_sync_scope.clone().unwrap();
        assert!(worker
            .save_sync_settings
            .as_ref()
            .unwrap()
            .load_card_folder(&scope, "duckstation", folder)
            .unwrap());
        assert!(worker.save_sync_agent.is_some());

        worker.set_memory_card_folder_enabled("duckstation", folder, false);
        assert!(!worker
            .save_sync_settings
            .as_ref()
            .unwrap()
            .load_card_folder(&scope, "duckstation", folder)
            .unwrap());
        assert!(worker.save_sync_agent.is_none());
    }

    #[test]
    fn remote_only_restore_and_conflict_version_advance_preserve_copies() {
        let fixture = FixtureServer::start();
        fixture.use_romm_memory_card_store(42);
        let client = Arc::new(RommClient::new(fixture.url()).unwrap());
        let directory = tempfile::tempdir().unwrap();
        let saves = directory.path().join("saves");
        // The folder exists but is empty: the card must be restored from RomM.
        let memcards = saves.join("psx/duckstation/memcards");
        std::fs::create_dir_all(&memcards).unwrap();

        let scope = SaveSyncScope {
            server_id: "https://romm.example".into(),
            account_id: "42".into(),
            installation_root: saves.clone(),
            effective_saves_root: saves.clone(),
        };
        let journal_path = directory.path().join("save-sync-journal.db");
        let journal = MemoryCardJournal::open(&journal_path, scope.clone()).unwrap();
        let config = MemoryCardConfig {
            scope,
            folders: discover_card_folders(&saves).unwrap(),
            identity: save_identity(),
            client: client.clone(),
            journal_path,
            debounce_secs: 0,
            session_id: 1,
        };
        let active = Arc::new(AtomicBool::new(true));
        let (sink, _events) = rommfs_core::events::channel();

        // Seed a card that exists only on RomM (created on another machine).
        let remote = client.create_memory_card("Mcd001", "duckstation").unwrap();
        fixture.seed_memory_card_version(remote.id, card_archive("Mcd001.mcd", b"remote v1"));

        let report: MemoryCardReport = reconcile_once(&config, &journal, false, &active, &sink);
        assert!(report.conflicts.is_empty());
        let card_path = memcards.join("Mcd001.mcd");
        assert_eq!(std::fs::read(&card_path).unwrap(), b"remote v1");
        assert!(journal
            .baseline("duckstation", "Mcd001")
            .unwrap()
            .unwrap()
            .version_id
            .is_some());

        // Both sides change: the local card is kept, and each remote version
        // that conflicts is preserved as its own sibling.
        std::fs::write(&card_path, b"local v2").unwrap();
        let v2 =
            fixture.seed_memory_card_version(remote.id, card_archive("Mcd001.mcd", b"remote v2"));
        let report = reconcile_once(&config, &journal, false, &active, &sink);
        assert_eq!(report.conflicts.len(), 1);
        assert_eq!(report.conflicts[0].remote_version_id, v2);
        assert_eq!(std::fs::read(&card_path).unwrap(), b"local v2");

        let v3 =
            fixture.seed_memory_card_version(remote.id, card_archive("Mcd001.mcd", b"remote v3"));
        let report = reconcile_once(&config, &journal, false, &active, &sink);
        assert_eq!(report.conflicts[0].remote_version_id, v3);
        // The older conflict is not overwritten by the newer one.
        assert_eq!(
            std::fs::read(memcards.join(format!("Mcd001.rommfs-conflict-v{v2}.mcd"))).unwrap(),
            b"remote v2"
        );
        assert_eq!(
            std::fs::read(memcards.join(format!("Mcd001.rommfs-conflict-v{v3}.mcd"))).unwrap(),
            b"remote v3"
        );

        // Cancelled work leaves the card and the conflicts untouched.
        active.store(false, Ordering::SeqCst);
        let before = fixture.request_count("GET", "/api/memory-cards");
        let report = reconcile_once(&config, &journal, false, &active, &sink);
        assert!(report.conflicts.is_empty());
        assert_eq!(fixture.request_count("GET", "/api/memory-cards"), before);
        assert_eq!(std::fs::read(&card_path).unwrap(), b"local v2");
    }
}
