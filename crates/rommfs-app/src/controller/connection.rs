//! Authentication, pairing, restoration, and logout operations.
use super::*;

impl Worker {
    pub(super) fn connect(&mut self, url: &str, username: &str, password: &str) {
        // Credentials are used to authenticate and then dropped from this
        // stack frame; they are never stored in logs/events (R1).
        self.advance_save_sync_session();
        self.sink.emit(AppEvent::Connecting);
        self.stop_save_sync_agent();
        self.cancel_pairing();
        if self.mount.is_some() {
            self.stop_mount();
        }

        // Starting a new connection replaces the previous session, even when
        // URL validation or authentication fails. Keep the mounted-session
        // rejection above intact because that mount still owns its client.
        // Dropping the old result receiver prevents stale refreshes from publishing.
        self.cancel_catalogue_refresh();
        self.source_snapshot = None;
        self.catalogue_account = None;
        self.client = None;
        self.server_id = None;
        self.catalogue = None;
        self.save_sync_catalogue = None;
        self.save_sync_identity = None;
        self.save_sync_scope = None;
        self.save_sync_profile = None;
        self.save_sync_mapping_report = None;
        self.save_sync_enabled = false;
        self.names.clear();
        self.fs = None;

        let client = match RommClient::new(url) {
            Ok(c) => Arc::new(c),
            Err(e) => {
                self.fail_connect(&e);
                self.refresh_save_sync();
                return;
            }
        };
        let save_sync_identity =
            match client.authenticate_for_save_sync(Credentials { username, password }) {
                Ok(identity) => identity,
                Err(error) => {
                    self.fail_connect(&error);
                    self.refresh_save_sync();
                    return;
                }
            };

        self.accept_connection(client, url, save_sync_identity);
    }

    pub(super) fn forget_other_account(&mut self, url: &str, account_id: Option<i64>) {
        if self.preferences.as_ref().is_some_and(|p| {
            server_id_of(&p.server) != server_id_of(url) || account_id != Some(p.account_id)
        }) {
            if let Some(p) = self.preferences.take() {
                if let Err(error) =
                    rommfs_core::credential_store::delete(&server_id_of(&p.server), p.account_id)
                {
                    self.log(
                        Level::Error,
                        "credentials",
                        format!("Previous remembered account could not be removed: {error}"),
                    );
                }
            }
            if let Some(settings) = &self.connection_settings {
                if let Err(error) = settings.clear() {
                    self.log(Level::Error, "settings", error.to_string());
                }
            }
        }
    }

    pub(super) fn accept_connection(
        &mut self,
        client: Arc<RommClient>,
        url: &str,
        identity: Option<rommfs_core::romm::SaveSyncIdentity>,
    ) {
        self.forget_other_account(url, identity.as_ref().map(|i| i.account_id));
        if self.preferences.is_none() {
            if let Some(identity) = &identity {
                self.preferences = Some(ConnectionPreferences {
                    server: url.into(),
                    account_id: identity.account_id,
                    device_identifier: uuid::Uuid::new_v4().to_string(),
                    mount_root: None,
                    integration: IntegrationMode::Generic,
                    retrobat_root: None,
                    start_at_login: false,
                    save_root: None,
                });
            }
        }
        self.emit_integration(None);
        self.client = Some(Arc::clone(&client));
        self.server_id = Some(server_id_of(url));
        self.catalogue_account = identity
            .as_ref()
            .map(|identity| identity.account_id.to_string());
        self.save_sync_identity = identity;
        self.sink.emit(AppEvent::Connected);
        // Connecting cleared the remembered host/folder facts when this
        // sign-in began; now that the session is authenticated, republish
        // its stored config or a configured user lands on folder setup.
        self.sink.emit(AppEvent::ConnectionRestored {
            server: url.to_string(),
            mount_root: self
                .preferences
                .as_ref()
                .and_then(|p| p.mount_root.as_ref().map(|r| r.display().to_string())),
        });
        self.log(
            Level::Info,
            "connect",
            "Authenticated connection established",
        );
        if self.restore_catalogue() {
            self.refresh_save_sync();
            self.refresh_catalogue();
            return;
        }
        match self.load_catalogue(&client) {
            Ok(cat) => {
                self.store_catalogue(cat);
                self.refresh_save_sync();
            }
            Err(error) => {
                if error.needs_sign_in() {
                    self.sink.emit(AppEvent::SignInRequired {
                        reason: Some(error.to_string()),
                    });
                } else {
                    self.sink.emit(AppEvent::CatalogueFailed {
                        reason: error.to_string(),
                    });
                }
                self.log(Level::Error, "catalogue", error.to_string());
                self.refresh_save_sync();
            }
        }
    }

    pub(super) fn begin_pairing(&mut self, url: &str) {
        self.clear_active_connection();
        self.sink.emit(AppEvent::Connecting);
        let result: Result<()> = (|| {
            let client = Arc::new(RommClient::new(url)?);
            let identifier = self
                .preferences
                .as_ref()
                .map(|p| p.device_identifier.clone())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let request = client.begin_pairing(&identifier)?;
            let browser_url = client.pairing_url(&request)?;
            let interval = Duration::from_secs(request.interval.max(1));
            self.sink.emit(AppEvent::PairingReady {
                user_code: request.user_code.clone(),
                url: browser_url,
            });
            self.pairing = Some(PendingPairing {
                client,
                url: url.into(),
                identifier,
                expires: Instant::now() + Duration::from_secs(request.expires_in.min(600)),
                next_poll: Instant::now() + interval,
                interval,
                request,
                epoch: self.current_connection_epoch,
            });
            Ok(())
        })();
        if let Err(error) = result {
            self.fail_connect(&error);
        }
    }

    pub(super) fn cancel_pairing(&mut self) {
        if self.pairing.take().is_some() {
            self.sink.emit(AppEvent::PairingEnded {
                reason: Some("Browser sign-in cancelled".into()),
            });
        }
    }

    pub(super) fn poll_pairing(&mut self) {
        let Some(mut pairing) = self.pairing.take() else {
            return;
        };
        if pairing.epoch != self.connection_gate.current_epoch() {
            self.pairing = Some(pairing);
            return;
        }
        if Instant::now() >= pairing.expires {
            self.sink.emit(AppEvent::PairingEnded {
                reason: Some("Pairing expired; sign in again".into()),
            });
            return;
        }
        if Instant::now() < pairing.next_poll {
            self.pairing = Some(pairing);
            return;
        }
        let result = pairing.client.poll_pairing(&pairing.request.device_code);
        if pairing.epoch != self.connection_gate.current_epoch() {
            self.pairing = Some(pairing);
            return;
        }
        match result {
            Ok(PairingPoll::Pending | PairingPoll::SlowDown) => {
                if matches!(result, Ok(PairingPoll::SlowDown)) {
                    pairing.interval += Duration::from_secs(5);
                }
                pairing.next_poll = Instant::now() + pairing.interval;
                self.pairing = Some(pairing);
            }
            Ok(PairingPoll::Denied | PairingPoll::Expired) => {
                self.sink.emit(AppEvent::PairingEnded {
                    reason: Some("Browser sign-in denied or expired; sign in again".into()),
                });
            }
            Ok(PairingPoll::Approved(secret)) => {
                let result = self.finish_pairing(&pairing, &secret);
                if pairing.epoch != self.connection_gate.current_epoch() {
                    self.pairing = Some(pairing);
                    return;
                }
                self.sink.emit(AppEvent::PairingEnded {
                    reason: result.as_ref().err().map(ToString::to_string),
                });
                if let Err(error) = result {
                    self.fail_connect(&error);
                }
            }
            Err(error) => {
                self.sink.emit(AppEvent::PairingEnded {
                    reason: Some(error.to_string()),
                });
                self.fail_connect(&error);
            }
        }
    }

    pub(super) fn finish_pairing(
        &mut self,
        pairing: &PendingPairing,
        secret: &PairedSession,
    ) -> Result<()> {
        pairing.client.restore_paired_session(secret)?;
        let identity = pairing.client.save_sync_identity()?.ok_or_else(|| {
            Error::Auth("pairing did not return a verified account; approve me.read".into())
        })?;
        if pairing.epoch != self.connection_gate.current_epoch() {
            return Err(Error::Cancelled);
        }
        self.forget_other_account(&pairing.url, Some(identity.account_id));
        let server = server_id_of(&pairing.url);
        let mut preferences = self
            .preferences
            .clone()
            .unwrap_or_else(|| ConnectionPreferences {
                server: pairing.url.clone(),
                account_id: identity.account_id,
                device_identifier: pairing.identifier.clone(),
                mount_root: None,
                integration: IntegrationMode::Generic,
                retrobat_root: None,
                start_at_login: false,
                save_root: None,
            });
        preferences.device_identifier = pairing.identifier.clone();
        // A credential-store failure leaves the authenticated in-memory session
        // usable, but is explicitly not represented as remembered sign-in.
        match rommfs_core::credential_store::save(&server, identity.account_id, secret) {
            Ok(()) => {
                if let Some(settings) = &self.connection_settings {
                    settings.save(&preferences)?;
                }
            }
            Err(error) => self.log(
                Level::Warn,
                "credentials",
                format!("Sign-in will not be remembered: {error}"),
            ),
        }
        self.preferences = Some(preferences);
        self.accept_connection(pairing.client.clone(), &pairing.url, Some(identity));
        Ok(())
    }

    pub(super) fn restore_connection(&mut self) {
        let result: Result<()> = (|| {
            let Some(settings) = &self.connection_settings else {
                return Ok(());
            };
            let Some(preferences) = settings.load()? else {
                return Ok(());
            };
            // Connecting clears the previous session's host/folder facts;
            // emit it BEFORE ConnectionRestored or the just-restored
            // configured_mount_root is wiped and a configured user renders
            // as "Set up your games folder" (UI-PLAN §3/§4).
            self.sink.emit(AppEvent::Connecting);
            self.sink.emit(AppEvent::ConnectionRestored {
                server: preferences.server.clone(),
                mount_root: preferences
                    .mount_root
                    .as_ref()
                    .map(|p| p.display().to_string()),
            });
            self.preferences = Some(preferences.clone());
            let Some(secret) = rommfs_core::credential_store::load(
                &server_id_of(&preferences.server),
                preferences.account_id,
            )?
            else {
                return Err(Error::Auth(
                    "remembered account needs browser sign-in".into(),
                ));
            };
            let client = Arc::new(RommClient::new(&preferences.server)?);
            client.restore_paired_session(&secret)?;
            // Authentication rejection is never hidden by a warm cache.
            let identity = client
                .save_sync_identity()?
                .ok_or_else(|| Error::Auth("stored session account unavailable".into()))?;
            if identity.account_id != preferences.account_id {
                return Err(Error::Auth(
                    "stored session belongs to another account".into(),
                ));
            }
            self.startup_restoring =
                std::env::args().any(|arg| arg == "--startup") && preferences.start_at_login;
            self.startup_requires_mount = self.startup_restoring;
            self.sink.emit(AppEvent::StartupPreference {
                enabled: preferences.start_at_login,
                error: None,
            });
            self.accept_connection(client, &preferences.server, Some(identity));
            if self.startup_restoring {
                if let Some(root) = &preferences.mount_root {
                    self.start_mount(&root.display().to_string());
                }
                self.startup_restoring = false;
                if self.mount.is_some() {
                    self.refresh_save_sync();
                } else {
                    self.log(Level::Warn,"startup","Automatic mount did not complete; save sync remains stopped. Open RomMFS to inspect the error.");
                    self.sink.emit(AppEvent::StartupPreference { enabled:preferences.start_at_login,error:Some("The games folder did not start automatically. Start it before resuming save sync.".into()) });
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.startup_restoring = false;
            self.fail_connect(&error);
        }
        if self.client.is_none() {
            self.refresh_save_sync();
        }
    }

    pub(super) fn clear_active_connection(&mut self) {
        self.startup_requires_mount = false;
        self.startup_restoring = false;
        self.cancel_auxiliary();
        self.save_file_rules = None;
        self.cancel_pairing();
        self.stop_save_sync_agent();
        self.stop_mount();
        self.advance_save_sync_session();
        self.cancel_catalogue_refresh();
        self.client = None;
        self.server_id = None;
        self.catalogue = None;
        self.source_snapshot = None;
        self.catalogue_account = None;
        self.names.clear();
        self.save_sync_catalogue = None;
        self.save_sync_identity = None;
        self.save_sync_scope = None;
        self.save_sync_enabled = false;
    }

    pub(super) fn logout(&mut self) {
        self.clear_active_connection();
        let result: Result<()> = (|| {
            if let Some(preferences) = &self.preferences {
                rommfs_core::credential_store::delete(
                    &server_id_of(&preferences.server),
                    preferences.account_id,
                )?;
            }
            if let Some(settings) = &self.connection_settings {
                settings.clear()?;
            }
            Ok(())
        })();
        self.preferences = None;
        if cfg!(windows) {
            if let Err(e) = crate::desktop::set_start_at_login(false) {
                self.log(
                    Level::Warn,
                    "startup",
                    format!("Could not remove login startup entry: {e}"),
                );
            }
        }
        self.sink.emit(AppEvent::StartupPreference {
            enabled: false,
            error: None,
        });
        if let Err(error) = result {
            self.log(
                Level::Error,
                "logout",
                format!("Could not remove remembered sign-in: {error}"),
            );
        }
        self.sink.emit(AppEvent::SignedOut);
        self.refresh_save_sync();
    }
}
