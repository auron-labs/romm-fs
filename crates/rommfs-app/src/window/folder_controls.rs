//! Connection and games-folder controls shared by window destinations.

use super::*;

impl RommfsWindow {
    /// Only the controls meaningful for the *current* auth state render —
    /// a signed-in session never sits beside its own sign-in buttons
    /// (UI-PLAN screenshot finding), and failures keep their real reason.
    /// The account block: real server/account when signed in, sign-in
    /// controls only when the state calls for them. The identity never
    /// falls back to a raw database id (UI-PLAN §Settings > Connection).
    pub(super) fn render_connection(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let conn = self.state.conn;
        let connecting = conn == ConnectionState::Connecting;
        let connected = conn == ConnectionState::Connected;
        // The server that actually answered — a remembered session may
        // never have typed into the url field this launch.
        let server = self
            .state
            .connected_server
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.url_input.read(cx).value().to_string());
        let user = self.user_input.read(cx).value();
        let summary = if server.is_empty() {
            "Signed in".to_string()
        } else if user.is_empty() {
            format!("Signed in to {server}")
        } else {
            format!("Signed in to {server} · {user}")
        };
        v_flex().gap_3()
            .when(!connected, |this| this
                .child(field_column("Server URL", Input::new(&self.url_input).w_full(), cx)))
            .when(connected, |this| this
                .child(muted(summary.clone(), cx))
                .child(h_flex().gap_2()
                    .child(Button::new("change-server").label("Change server").outline()
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.controller.send(Command::Logout); cx.notify();
                        })))
                    .child(Button::new("sign-out").label("Sign out")
                        .on_click(cx.listener(|view, _, _, cx| { view.controller.send(Command::Logout); cx.notify(); })))))
            .when(connecting, |this| this
                .child(Button::new("cancel-sign-in").label("Cancel sign-in")
                    .on_click(cx.listener(|view, _, _, cx| { view.controller.send(Command::CancelPairing); cx.notify(); }))))
            .when(!connecting && !connected, |this| this
                .child(Button::new("browser-sign-in").label("Sign in with browser…")
                    .primary()
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.password_sign_in = false; view.on_connect(cx); cx.notify();
                    })))
                .child(Collapsible::new().open(self.password_sign_in)
                    .child(Button::new("password-sign-in-options").ghost().label(if self.password_sign_in { "Hide password sign-in" } else { "Use password instead" })
                        .tooltip("Password sign-in is not remembered. Browser sign-in uses Windows Credential Manager.")
                        .on_click(cx.listener(|view,_,_,cx| { view.password_sign_in = !view.password_sign_in; cx.notify(); })))
                    .content(v_flex().gap_3()
                        .child(h_flex().gap_3().items_stretch()
                            .child(div().flex_1().min_w_0().child(field_column("Username", Input::new(&self.user_input).w_full(), cx)))
                            .child(div().id("password-field").frame_redacted(true).flex_1().min_w_0().child(field_column("Password", Input::new(&self.password_input).w_full().content_type(InputContentType::Password).mask_toggle(), cx))))
                        .child(Button::new("connect").label("Connect with password")
                            .on_click(cx.listener(|view,_,_,cx| view.on_connect(cx)))))))
            .when_some(self.state.pairing_code.clone(), |this, code| this.child(muted(format!("Approve RomMFS in your browser · code {code}"), cx)))
            .when_some(self.state.conn_error.clone(), |this, error| this.child(failure_block(
                "connection-failure", "Couldn't connect to RomM", failure_explanation(&error),
                None, error, window, cx,
            )))
    }
    /// Generic-folders vs RetroBat — the same choice in Settings > Folders
    /// and the first-run folder step.
    pub(super) fn layout_radio(
        &mut self,
        locked: bool,
        show_setup_link: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .gap_2()
            .child(section_heading("Library layout"))
            .child(
                RadioGroup::horizontal("library-layout")
                    .child(
                        Radio::new("generic")
                            .label("Generic folders")
                            .tooltip("Arranges games by platform."),
                    )
                    .child(Radio::new("retrobat").label("RetroBat").tooltip(
                        "Uses RetroBat's system names so its roms folder can point at your games.",
                    ))
                    .selected_index(Some(self.integration_choice))
                    .disabled(locked)
                    .on_change(cx.listener(|view, index, _, cx| {
                        view.integration_choice = *index;
                        // The layout applies on selection — no separate Apply step.
                        // Generic applies unconditionally; RetroBat applies once an
                        // installation is known, otherwise the picker below is the
                        // apply trigger.
                        if *index == 0 {
                            let retrobat_root =
                                view.state.retrobat_root.as_deref().map(PathBuf::from);
                            view.controller.send(Command::SetIntegration {
                                mode: IntegrationMode::Generic,
                                retrobat_root,
                            });
                        } else if let Some(root) = view.settings_install_root(cx) {
                            view.controller.send(Command::SetIntegration {
                                mode: IntegrationMode::RetroBat,
                                retrobat_root: Some(root),
                            });
                        }
                        cx.notify();
                    })),
            )
            .when(self.integration_choice == 1, |this| {
                let active = self.state.retrobat_setup_ok;
                this.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(status_marker(
                            if active {
                                StatusKind::Success
                            } else {
                                StatusKind::Muted
                            },
                            if active {
                                "Layout · RetroBat · Active"
                            } else {
                                "Layout · RetroBat · setup needed"
                            },
                            cx,
                        ))
                        .when(!active && show_setup_link, |this| {
                            this.child(
                                Button::new("layout-open-retrobat")
                                    .label("Set up RetroBat")
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.open_retrobat_settings(cx)
                                    })),
                            )
                        }),
                )
            })
    }
    /// Foreign entries reported under a refused mount/setup root: the classified
    /// list plus the safe remediation (delete only symlinks/junctions — never
    /// real user files) and an Escape hatch to open the folder for review.
    pub(super) fn render_foreign_remediation(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let root = self.state.foreign_root.clone()?;
        if self.state.foreign_entries.is_empty() && self.state.foreign_removal.is_none() {
            return None;
        }
        let links = self
            .state
            .foreign_entries
            .iter()
            .filter(|e| e.kind == ForeignKind::Link)
            .count();
        let mut list = v_flex()
            .gap_1()
            .pt_1()
            .child(section_heading("Files RomMFS does not manage"));
        for entry in self.state.foreign_entries.iter().take(12) {
            let kind = match entry.kind {
                ForeignKind::File => "file",
                ForeignKind::Directory => "folder",
                ForeignKind::Link => "link",
            };
            list = list.child(muted_xs(format!("{} ({kind})", entry.path), cx));
        }
        if self.state.foreign_entries.len() > 12 {
            list = list.child(muted_xs(
                format!("…and {} more", self.state.foreign_entries.len() - 12),
                cx,
            ));
        }
        let remove_root = root.clone();
        let open_root = root.clone();
        list = list.child(
            h_flex()
                .gap_2()
                .child(
                    Button::new("remove-foreign-links")
                        .label(format!("Remove {links} leftover links"))
                        .outline()
                        .small()
                        .disabled(links == 0)
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.controller.send(Command::RemoveForeignLinks {
                                root: PathBuf::from(remove_root.clone()),
                            });
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("open-foreign-folder")
                        .label("Open folder")
                        .ghost()
                        .small()
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.reveal_path(&PathBuf::from(open_root.clone()));
                        })),
                ),
        );
        if let Some(removal) = &self.state.foreign_removal {
            list = list.child(match removal {
                Ok(n) => status_marker(
                    StatusKind::Success,
                    format!("Removed {} links", fmt_count(*n)),
                    cx,
                )
                .into_any_element(),
                Err(e) => error_line(format!("Link cleanup failed: {e}"), cx).into_any_element(),
            });
        }
        Some(list)
    }
    /// The §3 games-folder section: human-readable state, the configured
    /// folder, the one action the current state calls for, and the library
    /// summary. Start is never shown beside a permanently disabled Stop;
    /// while ready, Stop and Refresh library are secondary actions.
    pub(super) fn render_mount(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let summary = self.state.folder_summary();
        let (mount_text, mount_kind) = self.mount_status(cx);
        let configured = self
            .state
            .mount_path
            .clone()
            .or_else(|| self.state.configured_mount_root.clone())
            .or_else(|| {
                let typed = self.mount_input.read(cx).value().trim().to_string();
                (summary != FolderSummary::SetupNeeded && !typed.is_empty()).then_some(typed)
            });
        let ready = summary == FolderSummary::Ready;
        let connected = self.state.conn == ConnectionState::Connected;
        let needs_retrobat = (self.state.retrobat_setup_migration
            || self.state.integration == IntegrationMode::RetroBat)
            && !self.state.retrobat_setup_ok;
        let primary = if matches!(summary, FolderSummary::Stopped | FolderSummary::Failed)
            && !connected
        {
            Some(self.settings_nav_button("overview-connect", "Connect to RomM", 0, true, cx))
        } else if matches!(summary, FolderSummary::Stopped | FolderSummary::Failed)
            && needs_retrobat
        {
            Some(self.settings_nav_button(
                "overview-finish-retrobat",
                "Finish RetroBat setup",
                2,
                true,
                cx,
            ))
        } else {
            match summary {
                FolderSummary::SetupNeeded => Some(self.settings_nav_button("overview-setup-folder", "Choose games folder", 1, true, cx)),
                FolderSummary::Stopped | FolderSummary::Failed => configured.clone().map(|path| {
                    Button::new("start-mount")
                        .icon(Icon::new(IconName::Play))
                        .label(if summary == FolderSummary::Failed { "Try again" } else { "Make games available" })
                        .primary()
                        .on_click(cx.listener(move |view, _, _, cx| view.start_mount(path.clone(), cx)))
                }),
                FolderSummary::Ready => configured.clone().map(|path| {
                    Button::new("open-mount")
                        .icon(Icon::new(IconName::FolderOpen))
                        .label("Open games folder")
                        .outline()
                        .disabled(self.state.mount_stopping)
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.state.folder_open_error = crate::desktop::open_games_folder(std::path::Path::new(&path))
                                .err().map(|error| format!("Couldn't open the games folder. Check the configured folder in Settings. {error}"));
                            cx.notify();
                        }))
                }),
                FolderSummary::Starting => Some(Button::new("start-mount").label("Preparing games…").primary().disabled(true)),
            }
        };
        let description = if ready {
            "Your games are available to emulators and frontends. Keep RomMFS running while you play."
        } else if needs_retrobat {
            "Finish connecting RetroBat to your library, then make your games available."
        } else {
            "Your RomM library appears as a folder on this PC. Game files download when opened."
        };
        card(cx).p_4().child(
            v_flex().gap_3()
                .child(status_row("Games", mount_kind, mount_text, None, cx))
                .when_some(self.state.catalogue, |this, (platforms, games, _, _)| {
                    this.child(div().text_lg().font_weight(FontWeight::SEMIBOLD)
                        .child(format!("{} games · {}", fmt_count(games), plural(platforms, "platform"))))
                })
                .child(muted(description, cx))
                .child(
                    h_flex().gap_2().flex_wrap().items_center()
                        .when_some(primary, |this, button| this.child(button))
                        .when(ready, |this| this.child(
                            Button::new("stop-mount")
                                .icon(Icon::new(IconName::Square))
                                .label(if self.state.mount_stopping { "Stopping…" } else { "Stop" })
                                .accessibility_label("Stop games folder")
                                .disabled(self.state.mount_stopping)
                                .outline()
                                .tooltip("Finish playing first. Stopping makes the games folder unavailable; downloaded game data stays cached.")
                                .on_click(cx.listener(|view, _, _, cx| view.on_stop_mount(cx))),
                        ))
                        .when(connected, |this| this.child(
                            Button::new("refresh-catalogue")
                                .icon(Icon::new(IconName::RefreshCw))
                                .label(if self.state.catalogue_progress.is_some() { "Checking library…" } else { "Check for library changes" })
                                .disabled(self.state.catalogue_progress.is_some() || summary == FolderSummary::Starting || self.state.mount_stopping)
                                .outline()
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.state.catalogue_progress = Some(("checking".into(), 0, None, 0));
                                    view.controller.send(Command::RefreshCatalogue);
                                    cx.notify();
                                })),
                        )),
                )
                .when_some(self.state.mount_error.clone(), |this, error| {
                    this.child(failure_block("mount-failure", "Couldn't make games available", failure_explanation(&error), None, error, window, cx))
                })
                .when_some(self.state.folder_open_error.clone(), |this, error| {
                    this.child(failure_block("folder-open-failure", "Couldn't open the games folder", failure_explanation(&error), None, error, window, cx))
                })
                .when_some(self.render_foreign_remediation(cx), |this, block| this.child(block))
                .when_some(self.state.catalogue_progress.clone(), |this, (_, completed, total, elapsed)| {
                    this.child(operation_progress("catalogue-progress", "Checking for library changes…", completed, total, elapsed, cx))
                })
                .when(summary == FolderSummary::Starting, |this| {
                    match self.state.mount_progress.clone() {
                        Some((stage, completed, total, elapsed)) => this.child(operation_progress("mount-progress", stage_label(&stage), completed, total, elapsed, cx)),
                        None => this.child(Progress::new("mount-progress").loading(true).small()),
                    }
                })
                .when(self.state.catalogue_pending_remount, |this| {
                    let restart = self.state.mount_path.clone().or_else(|| self.state.configured_mount_root.clone());
                    this.child(banner(
                        StatusKind::Warning,
                        IconName::RefreshCw,
                        "Library changes are ready. Finish playing before applying them; the games folder will restart.",
                        restart.map(|path| Button::new("restart-mount")
                            .label("Apply library changes")
                            .disabled(self.state.mount_stopping || summary == FolderSummary::Starting)
                            .outline().small()
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.state.request_mount(path.clone());
                                view.controller.send(Command::RestartMount { path: path.clone() });
                                cx.notify();
                            }))),
                        cx,
                    ))
                })
                .when_some(self.state.catalogue, |this, (_, _, files, skipped)| {
                    this.when(skipped > 0, |this| {
                        this.child(
                            h_flex().gap_2().flex_wrap()
                                .child(muted(format!("{} couldn't be included in this folder.", plural(skipped, "library item")), cx))
                                .child(Button::new("library-skipped-activity").label("View reasons").outline().small()
                                    .on_click(cx.listener(|view, _, _, cx| { view.active_view = 2; cx.notify(); }))),
                        )
                    })
                    .child(notes_disclosure(
                        "library-details", "Library details".into(),
                        vec![
                            format!("{} game files are listed. Listing a file doesn't download it.", fmt_count(files)),
                            format!("{} couldn't be included. This counts both whole games and individual files; it isn't a count of missing games.", plural(skipped, "library item")),
                            "Items may be unavailable because of missing information in RomM, unsupported systems or file types, or names Windows can't use. Activity has the individual reasons.".into(),
                        ], window, cx,
                    ))
                })
                .when_some(configured, |this, path| {
                    let text = window.use_keyed_state(SharedString::from(format!("games-folder-path-{path}")), cx, |window, cx| InputState::new(window, cx).default_value(path));
                    this.child(h_flex().gap_2().items_center()
                        .child(div().flex_1().min_w_0().child(Input::new(&text).readonly(true).w_full()))
                        .child(self.settings_nav_button("overview-folder-settings", "Folder settings", 1, false, cx)))
                })
                .when_some(self.state.catalogue_refreshed_at, |this, timestamp| {
                    this.child(muted_xs(format!("Library checked {}", relative_time(timestamp)), cx))
                }),
        )
    }
}
