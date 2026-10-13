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
        _window: &mut Window,
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
                        .on_click(cx.listener(|view,_,_,cx| { view.password_sign_in = !view.password_sign_in; cx.notify(); })))
                    .content(v_flex().gap_3()
                        .child(h_flex().gap_3().items_stretch()
                            .child(div().flex_1().min_w_0().child(field_column("Username", Input::new(&self.user_input).w_full(), cx)))
                            .child(div().flex_1().min_w_0().child(field_column("Password", Input::new(&self.password_input).w_full().content_type(InputContentType::Password).mask_toggle(), cx))))
                        .child(Button::new("connect").label("Connect with password")
                            .on_click(cx.listener(|view,_,_,cx| view.on_connect(cx))))
                        .child(muted("Password sign-in is not remembered. Browser sign-in uses Windows Credential Manager.", cx)))))
            .when_some(self.state.pairing_code.clone(), |this, code| this.child(muted(format!("Approve RomMFS in your browser · code {code}"), cx)))
            .when_some(self.state.conn_error.clone(), |this,error| this.child(error_line(error,cx)))
    }
    /// Generic-folders vs RetroBat — the same choice in Settings > Folders
    /// and the first-run folder step.
    pub(super) fn layout_radio(
        &mut self,
        locked: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .gap_2()
            .child(section_heading("Library layout"))
            .child(muted_xs(
                "Generic folders arranges games by platform. RetroBat uses RetroBat's system names so its roms folder can point at your games.",
                cx,
            ))
            .child(
                RadioGroup::horizontal("library-layout")
                    .children(["Generic folders", "RetroBat"])
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
                this.child(muted_xs(
                    if self.state.retrobat_setup_ok {
                        "The RetroBat layout is active and RetroBat is connected to your games folder."
                    } else {
                        "RetroBat's layout is used once RetroBat is connected to your games folder (see RetroBat below)."
                    },
                    cx,
                ))
            })
    }
    /// Why a Settings action is unavailable, with the stop action beside it
    /// when the games folder is the cause.
    pub(super) fn locked_reason(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let running = matches!(self.state.mount, MountState::Mounted | MountState::Mounting);
        let busy = self.state.retrobat_setup_running || self.state.auxiliary_running;
        h_flex()
            .gap_2()
            .items_center()
            .child(muted_xs(
                if running {
                    "Stop the games folder first (it is running)."
                } else if busy {
                    "Wait for the current setup or install to finish, or cancel it."
                } else {
                    ""
                },
                cx,
            ))
            .when(running, |this| {
                this.child(
                    Button::new("locked-stop-games-folder")
                        .label("Stop games folder")
                        .outline()
                        .small()
                        .on_click(cx.listener(|view, _, _, cx| view.on_stop_mount(cx))),
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
        list = list.child(muted_xs(
            "Only leftover links are removed — real files and folders are never deleted. Remove or move them yourself, then retry.",
            cx,
        ));
        if let Some(removal) = &self.state.foreign_removal {
            list = list.child(match removal {
                Ok(n) => muted_xs(format!("Removed {n} links. You can retry now."), cx)
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
                // A loaded library with no reported path: use the folder
                // Settings shows rather than asking for setup again.
                let typed = self.mount_input.read(cx).value().trim().to_string();
                (summary != FolderSummary::SetupNeeded && !typed.is_empty()).then_some(typed)
            });
        let ready = summary == FolderSummary::Ready;
        let connected = self.state.conn == ConnectionState::Connected;

        // The primary action this state offers (UI-PLAN §3 truth table).
        let primary: Option<Button> = match summary {
            FolderSummary::SetupNeeded => {
                Some(self.nav_button("overview-setup-folder", "Choose games folder", 4, true, cx))
            }
            FolderSummary::Stopped => configured.clone().map(|path| {
                Button::new("start-mount")
                    .label("Start games folder")
                    .primary()
                    .on_click(cx.listener(move |view, _, _, cx| view.start_mount(path.clone(), cx)))
            }),
            FolderSummary::Ready => configured.clone().map(|path| {
                Button::new("open-mount")
                    .label("Open games folder")
                    .primary()
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.reveal_path(&PathBuf::from(path.clone()));
                    }))
            }),
            // Failed renders no row action — the Retry lives inside the
            // failure block next to its reason (UI-PLAN §6).
            FolderSummary::Failed | FolderSummary::Starting => None,
        };

        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .justify_between()
                    .child(section_heading("Games folder"))
                    .child(status_marker(mount_kind, mount_text, cx)),
            )
            .when_some(configured, |this, path| {
                this.child(muted(path, cx))
            })
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .when_some(primary, |this, button| this.child(button))
                    .when(ready, |this| {
                        this.child(
                            Button::new("stop-mount")
                                .label("Stop games folder")
                                .outline()
                                .on_click(
                                    cx.listener(|view, _, _, cx| view.on_stop_mount(cx)),
                                ),
                        )
                    })
                    .when(connected, |this| {
                        this.child(
                            Button::new("refresh-catalogue")
                                .label("Refresh library")
                                .outline()
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.controller.send(Command::RefreshCatalogue);
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .when(ready, |this| {
                this.child(muted_xs(
                    "Stopping detaches the folder — games stay on the server and files already downloaded are kept, but the folder can't be opened until it starts again.",
                    cx,
                ))
            })
            .when_some(self.state.mount_error.clone(), |this, error| {
                let retry = self
                    .state
                    .mount_path
                    .clone()
                    .or_else(|| self.state.configured_mount_root.clone())
                    .map(|path| {
                        Button::new("retry-mount")
                            .label("Retry")
                            .primary()
                            .small()
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.start_mount(path.clone(), cx)
                            }))
                    });
                this.child(failure_block(
                    "mount-failure",
                    "Couldn't start the games folder",
                    failure_explanation(&error),
                    retry,
                    error,
                    window,
                    cx,
                ))
            })
            .when_some(self.render_foreign_remediation(cx), |this, block| {
                this.child(block)
            })
            .when_some(self.state.catalogue, |this, (platforms, roms, skipped)| {
                this.child(muted(
                    format!(
                        "{} platforms · {} ROMs · {} skipped",
                        fmt_count(platforms),
                        fmt_count(roms),
                        fmt_count(skipped),
                    ),
                    cx,
                ))
            })
            .when_some(self.state.catalogue_refreshed_at, |this, timestamp| {
                this.child(muted(
                    format!("Last complete catalogue: {}", relative_time(timestamp)),
                    cx,
                ))
            })
            .when(self.state.catalogue_pending_remount, |this| {
                this.child(muted(
                    "A complete refresh is ready; stop and start the games folder to publish it.",
                    cx,
                ))
            })
            .when_some(
                self.state.catalogue_progress.clone(),
                |this, (_stage, completed, total, elapsed)| {
                    // §3 copy: the refresh is "Updating game list…" — the
                    // internal stage name stays in the technical log.
                    this.child(operation_progress(
                        "catalogue-progress",
                        "Updating game list…",
                        completed,
                        total,
                        elapsed,
                        cx,
                    ))
                },
            )
            .when(
                summary == FolderSummary::Starting,
                |this| match self.state.mount_progress.clone() {
                    Some((stage, completed, total, elapsed)) => this.child(operation_progress(
                        "mount-progress",
                        &stage,
                        completed,
                        total,
                        elapsed,
                        cx,
                    )),
                    None => this.child(Progress::new("mount-progress").loading(true).small()),
                },
            )
            .child(muted(
                format!(
                    "Game files download when an application opens them. {}",
                    close_behavior_copy(self.tray_hides)
                ),
                cx,
            ))
    }
}
