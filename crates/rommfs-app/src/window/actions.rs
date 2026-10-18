//! Event and picker handlers for the retained window entity.

use super::*;

impl RommfsWindow {
    pub(super) fn on_connect(&mut self, cx: &mut Context<Self>) {
        if self.state.conn == ConnectionState::Connecting {
            return;
        }
        if !self.password_sign_in {
            self.controller.send(Command::BeginPairing {
                url: self.url_input.read(cx).value().to_string(),
            });
            return;
        }
        self.controller.send(Command::Connect {
            url: self.url_input.read(cx).value().to_string(),
            username: self.user_input.read(cx).value().to_string(),
            password: self.password_input.read(cx).value().to_string(),
        });
    }
    pub(super) fn on_start_mount(&mut self, cx: &mut Context<Self>) {
        let path = self.mount_input.read(cx).value().to_string();
        self.start_mount(path, cx);
    }
    /// Start a mount at an explicit path — shared by the folder field's
    /// Start action and the per-state actions on Overview (UI-PLAN §3).
    pub(super) fn start_mount(&mut self, path: String, cx: &mut Context<Self>) {
        if matches!(self.state.mount, MountState::Mounting | MountState::Mounted) {
            return;
        }
        self.state.request_mount(path.clone());
        cx.notify();
        self.controller.send(Command::StartMount { path });
    }
    pub(super) fn on_stop_mount(&mut self, cx: &mut Context<Self>) {
        if self.state.mount != MountState::Mounted || self.state.mount_stopping {
            return;
        }
        self.state.mount_stopping = true;
        cx.notify();
        self.controller.send(Command::StopMount);
    }
    /// The RetroBat installation the Settings tab currently points at: the
    /// picker's selection, falling back to the already-applied root.
    pub(super) fn settings_install_root(&self, cx: &App) -> Option<PathBuf> {
        self.settings_install_select
            .read(cx)
            .selected_value()
            .map(|value| PathBuf::from(value.to_string()))
            .or_else(|| self.state.retrobat_root.as_deref().map(PathBuf::from))
    }
    /// Folder picker for a RetroBat installation discovery missed. Applies the
    /// RetroBat layout with that root immediately — the same thing a pick in
    /// the select does.
    pub(super) fn browse_settings_install(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select a RetroBat installation folder".into()),
        });
        cx.spawn(
            async move |this: WeakEntity<RommfsWindow>, cx| match paths.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        let _ = this.update(cx, |view, cx| {
                            view.controller.send(Command::SetIntegration {
                                mode: IntegrationMode::RetroBat,
                                retrobat_root: Some(path),
                            });
                            cx.notify();
                        });
                    }
                }
                Ok(Ok(None)) => {}
                Ok(Err(error)) => {
                    let _ = this.update(cx, |view, _| {
                        view.show_picker_error(format!("RetroBat folder picker failed: {error}"))
                    });
                }
                Err(_) => {
                    let _ = this.update(cx, |view, _| {
                        view.show_picker_error("folder picker response was interrupted".into())
                    });
                }
            },
        )
        .detach();
    }
    /// §4 step 2 offers a folder picker beside the free-text path.
    pub(super) fn browse_games_folder(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select the games folder".into()),
        });
        cx.spawn(
            async move |this: WeakEntity<RommfsWindow>, cx| match paths.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        let _ = this.update_in(cx, |view, window, cx| {
                            let text = path.display().to_string();
                            view.mount_input.update(cx, |input, cx| {
                                input.set_value(text, window, cx);
                            });
                            cx.notify();
                        });
                    }
                }
                Ok(Ok(None)) => {}
                Ok(Err(error)) => {
                    let _ = this.update(cx, |view, _| {
                        view.show_picker_error(format!("Games folder picker failed: {error}"))
                    });
                }
                Err(_) => {
                    let _ = this.update(cx, |view, _| {
                        view.show_picker_error("folder picker response was interrupted".into())
                    });
                }
            },
        )
        .detach();
    }
    pub(super) fn apply_save_sync_debounce(&mut self, cx: &mut Context<Self>) {
        let value = self.debounce_input.read(cx).value().trim().to_owned();
        let Ok(seconds) = value.parse::<u32>() else {
            self.debounce_error = Some("Enter a whole number from 1 to 3600 seconds.".into());
            cx.notify();
            return;
        };
        if !(1..=3600).contains(&seconds) {
            self.debounce_error = Some("Enter a whole number from 1 to 3600 seconds.".into());
            cx.notify();
            return;
        }
        self.debounce_error = None;
        self.debounce_input_applied = seconds.to_string();
        self.controller
            .send(Command::SetSaveSyncDebounce { seconds });
        cx.notify();
    }
    pub(super) fn prompt_export(
        &mut self,
        incoming: SaveSyncIncomingStatus,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.state.save_sync_effective_saves_root.as_deref() else {
            return;
        };
        let session_id = self.state.save_sync_session_id;
        if !self.pending_incoming_exists(&incoming.incoming_id) {
            return;
        }
        let root = PathBuf::from(root);
        let directory = root.parent().unwrap_or(&root).to_path_buf();
        let suggested_name = format!("rommfs-incoming-{}.rommfs-incoming", incoming.incoming_id);
        let prompt = cx.prompt_for_new_path(&directory, Some(suggested_name.as_str()));
        let incoming_id = incoming.incoming_id;
        cx.spawn(
            async move |this: WeakEntity<RommfsWindow>, cx| match prompt.await {
                Ok(Ok(Some(destination))) => {
                    let _ = this.update(cx, |view, cx| {
                        if view.state.save_sync_session_id != session_id
                            || !view.pending_incoming_exists(&incoming_id)
                        {
                            return;
                        }
                        view.controller.send(Command::ExportSaveSyncIncoming {
                            session_id,
                            incoming_id,
                            destination,
                        });
                        cx.notify();
                    });
                }
                Ok(Ok(None)) => {}
                Ok(Err(error)) => {
                    let _ = this.update(cx, |view, _| {
                        view.emit_export_error(session_id, &incoming_id, error.to_string());
                    });
                }
                Err(_) => {
                    let _ = this.update(cx, |view, _| {
                        view.emit_export_error(
                            session_id,
                            &incoming_id,
                            "file picker response was interrupted".into(),
                        );
                    });
                }
            },
        )
        .detach();
    }
    pub(super) fn pending_incoming_exists(&self, incoming_id: &str) -> bool {
        self.state.save_sync_queue.as_ref().is_some_and(|queue| {
            queue
                .incoming
                .iter()
                .any(|incoming| incoming.incoming_id == incoming_id)
        })
    }
    pub(super) fn show_picker_error(&self, error: String) {
        self.controller
            .sink()
            .emit(AppEvent::log(Level::Error, "save-sync", error));
    }
    pub(super) fn emit_export_error(&self, session_id: u64, incoming_id: &str, error: String) {
        self.controller
            .sink()
            .emit(AppEvent::SaveSyncExportFinished {
                session_id,
                incoming_id: incoming_id.to_owned(),
                destination: None,
                error: Some(error),
            });
    }
    pub(super) fn on_copy_log(&mut self, cx: &mut Context<Self>) {
        let body = self
            .state
            .log
            .lines()
            .map(super::pages::activity::format_log_line)
            .collect::<Vec<_>>()
            .join("\n");
        cx.write_to_clipboard(ClipboardItem::new_string(body));
    }
    /// A "go fix this" button that opens Settings on a specific tab.
    pub(super) fn settings_nav_button(
        &mut self,
        id: &'static str,
        label: &'static str,
        tab: usize,
        primary: bool,
        cx: &mut Context<Self>,
    ) -> Button {
        let button = Button::new(id).label(label);
        let button = if primary {
            button.primary()
        } else {
            button.outline()
        };
        button.on_click(cx.listener(move |this, _, _, cx| {
            this.active_view = 3;
            this.settings_tab = tab;
            cx.notify();
        }))
    }
    /// Jump to the Settings RetroBat tab (first-run optional step).
    pub(super) fn open_retrobat_settings(&mut self, cx: &mut Context<Self>) {
        self.active_view = 3;
        self.settings_tab = 2;
        cx.notify();
    }
    /// Run RetroBat step 2 (link the games folder) for the selected
    /// installation and configured games folder — shared by Settings and the
    /// Overview update action.
    pub(super) fn run_retrobat_setup(&mut self, cx: &mut Context<Self>) {
        let Some(installation) = self.settings_install_root(cx) else {
            return;
        };
        let managed_root = PathBuf::from(self.mount_input.read(cx).value().trim());
        self.controller.send(Command::SetupRetroBat {
            installation,
            managed_root,
        });
        cx.notify();
    }
}
