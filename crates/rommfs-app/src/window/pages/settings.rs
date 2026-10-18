use super::super::*;
use crate::controller::{parse_progress_counts, RetroBatStep, StepStatus};
use rommfs_core::retrobat::SkippedSystem;

/// The Settings tabs (phase 2c). The selected index lives in view state.
const SETTINGS_TABS: [&str; 5] = [
    "RomM connection",
    "Games folder",
    "RetroBat",
    "Saves",
    "Application",
];

/// Settings page: a tab bar with one scroll region per tab (phase 2c).
pub fn render(
    view: &mut RommfsWindow,
    window: &mut Window,
    cx: &mut Context<RommfsWindow>,
) -> impl IntoElement {
    view.render_settings(window, cx)
}

impl RommfsWindow {
    /// Settings owns its own tab bar and per-tab scrolling, so the window
    /// renders it outside the shared page scroll region (phase 2c).
    fn render_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = self.settings_tab.min(SETTINGS_TABS.len() - 1);
        let handle = self.settings_tab_scrolls[tab].clone();
        let body: AnyElement = match tab {
            0 => self.render_account_tab(window, cx).into_any_element(),
            1 => self.render_games_folder_tab(window, cx).into_any_element(),
            2 => self.render_retrobat_tab(window, cx).into_any_element(),
            3 => self.render_saves_tab(window, cx).into_any_element(),
            _ => self.render_app_tab(cx).into_any_element(),
        };
        v_flex()
            .size_full()
            .min_h_0()
            .child(
                div().w_full().px_6().pt_6().pb_3().child(
                    v_flex()
                        .w_full()
                        .max_w(px(960.))
                        .mx_auto()
                        .gap_4()
                        .child(page_heading("Settings"))
                        .child(
                            TabBar::new("settings-tabs")
                                .selected_index(tab)
                                .children(
                                    SETTINGS_TABS
                                        .into_iter()
                                        .map(|label| Tab::new().label(label)),
                                )
                                .on_click(cx.listener(|view, index, _, cx| {
                                    view.settings_tab = *index;
                                    cx.notify();
                                })),
                        ),
                ),
            )
            .child(
                div()
                    .id("settings-tab-body")
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .track_scroll(&handle)
                    .overflow_y_scroll()
                    .vertical_scrollbar(&handle)
                    .child(
                        div()
                            .w_full()
                            .max_w(px(960.))
                            .mx_auto()
                            .px_6()
                            .pb_6()
                            .child(body),
                    ),
            )
    }

    /// True while a control that changes the folder layout or RetroBat setup
    /// would fail because the folder or a setup is running.
    fn settings_locked(&self) -> bool {
        self.state.retrobat_setup_running
            || self.state.auxiliary_running
            || matches!(self.state.mount, MountState::Mounted | MountState::Mounting)
    }

    /// The one chip a locked tab shows at the top (phase 2c: one chip, not a
    /// sentence under every control).
    fn lock_chip(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let running = matches!(self.state.mount, MountState::Mounted | MountState::Mounting);
        let busy = self.state.retrobat_setup_running || self.state.auxiliary_running;
        if !running && !busy {
            return None;
        }
        let mut row = h_flex().gap_2().items_center().child(status_marker(
            StatusKind::Warning,
            if running {
                "Games folder running"
            } else {
                "Setup in progress"
            },
            cx,
        ));
        if running {
            row = row.child(
                Button::new("settings-lock-stop")
                    .label("Stop")
                    .outline()
                    .small()
                    .on_click(cx.listener(|view, _, _, cx| view.on_stop_mount(cx))),
            );
        }
        Some(row.into_any_element())
    }

    /// Account: server, signed-in user, sign-in method, and the sign-out
    /// action. The tab title already says "Account" (phase 2a rule 1).
    fn render_account_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let missing_permission =
            !self.state.play_data_available && self.state.conn == ConnectionState::Connected;
        v_flex()
            .gap_3()
            .child(self.render_connection(window, cx))
            .when(missing_permission, |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(status_marker(
                            StatusKind::Warning,
                            "Favourites & play history · Needs permission",
                            cx,
                        ))
                        .child(
                            Button::new("reconnect-play-data")
                                .label("Reconnect")
                                .outline()
                                .small()
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.on_connect(cx);
                                    cx.notify();
                                })),
                        ),
                )
            })
    }

    /// Games folder: location, library layout, and start at login.
    fn render_games_folder_tab(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let locked = self.settings_locked();
        let connected = self.state.conn == ConnectionState::Connected;
        let typed = self.mount_input.read(cx).value().trim().to_string();
        let empty = typed.is_empty();
        // Apply is only meaningful when the field differs from the folder in
        // use (WS6.7).
        let changed = self
            .state
            .mount_path
            .clone()
            .or_else(|| self.state.configured_mount_root.clone())
            .is_none_or(|current| !current.eq_ignore_ascii_case(&typed));
        let can_apply = !locked && connected && !empty && changed;
        let apply_reason = if locked {
            "Stop the games folder first."
        } else if !connected {
            "Connect to RomM first."
        } else if empty {
            "Enter or browse to a folder."
        } else {
            "This is already the games folder."
        };
        let browse = Button::new("settings-browse-folder")
            .label("Browse…")
            .outline()
            .disabled(locked);
        let browse = if locked {
            browse.tooltip("Stop the games folder first.")
        } else {
            browse
        };
        let apply = Button::new("apply-games-folder")
            .label("Apply")
            .outline()
            .disabled(!can_apply);
        let apply = if can_apply {
            apply
        } else {
            apply.tooltip(apply_reason)
        };
        v_flex()
            .gap_6()
            .when_some(self.lock_chip(cx), |this, chip| this.child(chip))
            .child(
                v_flex()
                    .gap_3()
                    .child(section_heading("Location"))
                    .child(field_column(
                        "Games folder",
                        Input::new(&self.mount_input).w_full(),
                        cx,
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(browse.on_click(
                                cx.listener(|view, _, _, cx| view.browse_games_folder(cx)),
                            ))
                            .child(apply.on_click(cx.listener(|view, _, _, cx| {
                                let path = view.mount_input.read(cx).value().trim().to_string();
                                view.controller.send(Command::SetMountRoot { path });
                                cx.notify();
                            }))),
                    ),
            )
            .child(self.layout_radio(locked, cx))
            .child(self.render_start_at_login(cx))
    }

    /// Start-at-login plus its disabled reason as a tooltip (phase 2a rule 3).
    fn render_start_at_login(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let login_supported = cfg!(windows);
        let needs_connect =
            !self.state.start_at_login && self.state.conn != ConnectionState::Connected;
        let login_disabled = !login_supported || needs_connect;
        let login_reason = if !login_supported {
            "Start at login is available on Windows only."
        } else {
            "Connect to RomM with a remembered sign-in first."
        };
        let switch = Switch::new("start-at-login")
            .checked(self.state.start_at_login)
            .label("Start at login")
            .disabled(login_disabled);
        let switch = if login_disabled {
            switch.tooltip(login_reason)
        } else {
            switch
        };
        v_flex()
            .gap_3()
            .child(section_heading("Startup"))
            .child(switch.on_change(cx.listener(move |view, checked, _, cx| {
                if login_disabled {
                    return;
                }
                view.controller
                    .send(Command::SetStartAtLogin { enabled: *checked });
                cx.notify();
            })))
            .when_some(self.state.startup_error.clone(), |this, error| {
                this.child(error_line(error, cx))
            })
    }

    /// RetroBat: the three steps as a checklist. A finished step collapses to
    /// one row with its status and a Redo action (phase 2c, WS6.11).
    fn render_retrobat_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let locked = self.settings_locked();
        let steps = self.state.retrobat_steps();
        let primary = steps.primary();
        // A finished support step with issues shows "Done · N need attention"
        // in its header instead of a green "Done" beside the count (WS6.9).
        let support_note = (steps.support == StepStatus::Done)
            .then(|| {
                self.state.auxiliary_summary.as_ref().and_then(|summary| {
                    auxiliary_headline(summary.published, summary.unchanged, &summary.warnings)
                        .1
                        .map(|note| SharedString::from(format!("Done · {note}")))
                })
            })
            .flatten();
        let install_body = self
            .retrobat_install_step(primary == Some(RetroBatStep::Install), window, cx)
            .into_any_element();
        let link_body = self
            .retrobat_link_step(steps.link, primary == Some(RetroBatStep::Link), window, cx)
            .into_any_element();
        let support_body = self
            .support_files_step(
                steps.support,
                primary == Some(RetroBatStep::Support),
                locked,
                window,
                cx,
            )
            .into_any_element();
        let mut tab = v_flex().gap_3();
        if let Some(chip) = self.lock_chip(cx) {
            tab = tab.child(chip);
        }
        tab.child(self.retrobat_step(
            0,
            "Install RetroBat",
            (steps.install, None),
            install_body,
            window,
            cx,
        ))
        .child(self.retrobat_step(
            1,
            "Connect RetroBat to your games folder",
            (steps.link, None),
            link_body,
            window,
            cx,
        ))
        .child(self.retrobat_step(
            2,
            "BIOS & firmware",
            (steps.support, support_note),
            support_body,
            window,
            cx,
        ))
        .child(self.render_hide_copies(cx))
    }

    /// Hide RomM's copy of a game the user already has, with the current count.
    fn render_hide_copies(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = self.state.hide_copies_enabled;
        let hidden = self.state.hide_copies_count;
        v_flex()
            .gap_2()
            .child(section_heading("Your games"))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Switch::new("hide-owned-copies")
                            .checked(enabled)
                            .label("Hide RomM games you already have")
                            .on_change(cx.listener(|view, checked, _, cx| {
                                view.controller.send(Command::SetHideOwnedCopies {
                                    enabled: *checked,
                                });
                                cx.notify();
                            })),
                    )
                    .child(info_button(
                        "hide-owned-copies-help",
                        "When a game in RomM is also in your own roms folder, RomMFS hides the RomM copy in RetroBat. Your own files are never touched.",
                    ))
                    .when(enabled && hidden > 0, |this| {
                        this.child(muted_xs(format!("{hidden} hidden"), cx))
                    }),
            )
    }

    /// One step card. Done steps collapse to a row with a Redo action until
    /// the user expands them again.
    fn retrobat_step(
        &mut self,
        index: usize,
        title: &'static str,
        status: (StepStatus, Option<SharedString>),
        body: AnyElement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (status, note) = status;
        let number = index + 1;
        let done = status == StepStatus::Done;
        let open = window.use_keyed_state(
            SharedString::from(format!("retrobat-step-{index}-open")),
            cx,
            |_, _| std::cell::Cell::new(false),
        );
        let is_open = open.read(cx).get();
        if done && !is_open {
            let toggle = open.clone();
            let (kind, text) = match note {
                Some(note) => (StatusKind::Warning, note),
                None => (StatusKind::Success, "Done".into()),
            };
            return h_flex()
                .justify_between()
                .gap_3()
                .p_3()
                .border_1()
                .border_color(cx.theme().border)
                .rounded(cx.theme().radius)
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("{number}. {title}")),
                )
                .child(
                    h_flex()
                        .gap_3()
                        .items_center()
                        .child(status_marker(kind, text, cx))
                        .child(
                            Button::new(format!("retrobat-redo-{index}"))
                                .label("Redo")
                                .ghost()
                                .small()
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    toggle.update(cx, |open, _| open.set(true));
                                    cx.notify();
                                })),
                        ),
                )
                .into_any_element();
        }
        v_flex()
            .gap_2()
            .p_3()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius)
            .child(step_header(number, title, status, note, cx))
            .child(body)
            .into_any_element()
    }

    /// Save folder, save sync, and the save-change delay.
    fn render_saves_tab(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let enabled = self.state.save_sync_enabled;
        let available = self.state.save_sync_available;
        let switch = Switch::new("save-sync-enabled")
            .checked(enabled)
            .label("Automatic save sync")
            .disabled(!available && !enabled);
        let switch = if !available && !enabled {
            switch.tooltip("Choose a save folder first.")
        } else {
            switch
        };
        let use_save_folder = Button::new("choose-save-folder")
            .label("Use save folder")
            .outline()
            .disabled(self.state.conn != ConnectionState::Connected);
        let use_save_folder = if self.state.conn == ConnectionState::Connected {
            use_save_folder
        } else {
            use_save_folder.tooltip("Connect to RomM first.")
        };
        v_flex()
            .gap_6()
            .child(
                v_flex()
                    .gap_3()
                    .child(section_heading("Save folder"))
                    .when_some(
                        self.state
                            .save_sync_effective_saves_root
                            .clone()
                            .or_else(|| self.state.save_sync_selected_root.clone()),
                        |this, root| this.child(muted_xs(format!("Using {root}"), cx)),
                    )
                    .child(field_column(
                        "Save-data folder",
                        Input::new(&self.save_folder_input).w_full(),
                        cx,
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                use_save_folder.on_click(cx.listener(|view, _, _, cx| {
                                    view.controller.send(Command::SelectSaveFolder {
                                        path: PathBuf::from(
                                            view.save_folder_input.read(cx).value().trim(),
                                        ),
                                    });
                                    cx.notify();
                                })),
                            )
                            .child(info_button(
                                "save-folder-help",
                                "Select saves only, not an emulator's entire portable folder. All file formats and companions are supported. Shared saves stay out of ordinary save mapping; memory cards sync separately below.",
                            )),
                    ),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(section_heading("Syncing"))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(switch.on_change(cx.listener(|view, checked, _, cx| {
                                view.state.request_save_sync_enabled(*checked);
                                view.controller.send(Command::SetSaveSyncEnabled {
                                    enabled: *checked,
                                });
                                cx.notify();
                            })))
                            .child(info_button(
                                "save-sync-help",
                                "Changed files upload automatically. Remote-only changes replace an unchanged synchronized baseline; simultaneous changes and active files are kept for review. Local deletion never deletes remote saves.",
                            )),
                    )
                    .child(self.render_save_delay(cx)),
            )
            .child(self.render_memory_cards(cx))
            .child(self.render_play_history(cx))
    }

    /// One switch per discovered PS1/PS2 memory-card folder. Off by default and
    /// scoped like a save root; a shared card only syncs once its emulator exits.
    fn render_memory_cards(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let folders = self.state.memory_card_folders.clone();
        let available = self.state.memory_cards_available;
        let problem = self.state.memory_cards_problem.clone();
        v_flex()
            .gap_2()
            .child(section_heading("Memory cards"))
            .when(folders.is_empty(), |this| {
                this.child(muted_xs("No PS1/PS2 memory-card folders found.", cx))
            })
            .children(folders.into_iter().map(|folder| {
                let switch = Switch::new(format!("memory-card-{}", folder.relative_dir))
                    .checked(folder.enabled)
                    .label(format!(
                        "{} · {}",
                        folder.label,
                        plural(folder.cards, "card")
                    ))
                    .disabled(!available && !folder.enabled);
                let switch = if available || folder.enabled {
                    switch
                } else {
                    switch.tooltip("Reconnect to grant the save permissions.")
                };
                let emulator = folder.emulator.clone();
                let relative_dir = PathBuf::from(folder.relative_dir.clone());
                switch.on_change(cx.listener(move |view, checked, _, cx| {
                    view.controller.send(Command::SetMemoryCardFolderEnabled {
                        emulator: emulator.clone(),
                        relative_dir: relative_dir.clone(),
                        enabled: *checked,
                    });
                    cx.notify();
                }))
            }))
            .when_some(problem, |this, problem| this.child(error_line(problem, cx)))
    }

    /// Favourites and play history: off until the user turns it on, and only
    /// available once the session holds the play-data scopes (phase 3).
    fn render_play_history(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let available = self.state.play_data_available;
        let enabled = self.state.play_data_enabled;
        let switch = Switch::new("play-data-sync")
            .checked(enabled)
            .label("Favourites & play history")
            .disabled(!available);
        let switch = if available {
            switch
        } else {
            switch.tooltip("Reconnect to grant the favourites and play-history permissions.")
        };
        v_flex()
            .gap_2()
            .child(section_heading("Play history"))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(switch.on_change(cx.listener(|view, checked, _, cx| {
                        view.controller.send(Command::SetPlayDataSync {
                            enabled: *checked,
                        });
                        cx.notify();
                    })))
                    .child(info_button(
                        "play-data-help",
                        "Favourites, last played, play count and play time sync both ways with RomM. The later value wins for last played; a favourite changed on both sides keeps the RetroBat value.",
                    )),
            )
            .when_some(self.state.play_data_problem.clone(), |this, problem| {
                this.child(error_line(problem, cx))
            })
    }

    /// The quiet period before a changed save uploads.
    fn render_save_delay(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let can_apply = self.state.save_sync_available || self.state.save_sync_enabled;
        let apply = Button::new("save-sync-apply-debounce")
            .label("Apply")
            .outline()
            .small()
            .disabled(!can_apply);
        let apply = if can_apply {
            apply
        } else {
            apply.tooltip("Enable save sync first.")
        };
        let mut section = v_flex().gap_2().child(
            h_flex()
                .gap_3()
                .items_end()
                .child(div().w_64().child(field_column(
                    "Wait after a save changes (seconds)",
                    Input::new(&self.debounce_input).w_full().small(),
                    cx,
                )))
                .child(
                    apply.on_click(cx.listener(|view, _, _, cx| view.apply_save_sync_debounce(cx))),
                ),
        );
        if let Some(error) = self.debounce_error.clone() {
            section = section.child(error_line(error, cx));
        }
        section
    }

    /// App: what closing the window does, plus the logs folder.
    fn render_app_tab(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_6()
            .child(
                v_flex()
                    .gap_3()
                    .child(section_heading("Closing the window"))
                    .child(muted(close_behavior_copy(self.tray_hides), cx)),
            )
            .child(
                v_flex().gap_3().child(section_heading("Logs")).child(
                    Button::new("settings-open-log-folder")
                        .label("Open logs folder")
                        .outline()
                        .on_click(cx.listener(|view, _, _, cx| {
                            if let Some(path) = crate::controller::diagnostic_log_directory() {
                                cx.reveal_path(&path);
                            } else {
                                view.controller.sink().emit(AppEvent::log(
                                    Level::Error,
                                    "diagnostics",
                                    "No application log directory is available",
                                ));
                            }
                        })),
                ),
            )
    }
}

/// Header line of one step: number and title with its status on the right.
/// `note` overrides the status phrase when a done step still has issues.
fn step_header(
    number: usize,
    title: &'static str,
    status: StepStatus,
    note: Option<SharedString>,
    cx: &App,
) -> impl IntoElement {
    let (kind, text) = match note {
        Some(note) => (StatusKind::Warning, note),
        None => match status {
            StepStatus::Done => (StatusKind::Success, "Done".into()),
            StepStatus::Ready => (StatusKind::Warning, "Next".into()),
            StepStatus::Running => (StatusKind::Warning, "In progress".into()),
            StepStatus::Failed => (StatusKind::Danger, "Needs attention".into()),
            StepStatus::Blocked(_) => (StatusKind::Muted, "Waiting".into()),
        },
    };
    h_flex()
        .justify_between()
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(format!("{number}. {title}")),
        )
        .child(status_marker(kind, text, cx))
}

/// A progress bar for a step: determinate when the units are known.
fn step_progress(id: &'static str, counts: Option<(u64, u64)>) -> impl IntoElement {
    match counts {
        Some((done, total)) => Progress::new(id)
            .value((100.0 * done as f32 / total as f32).min(100.0))
            .small(),
        None => Progress::new(id).loading(true).small(),
    }
}

/// The "folders restored" note from a finished setup result, when any were.
/// Skipped systems are reported separately as chips (WS6.10).
fn restored_note(detail: &str) -> Option<(String, Vec<String>)> {
    let body = detail.split_once(": ").map_or(detail, |(_, rest)| rest);
    if !body.contains("restored") {
        return None;
    }
    let lines = body
        .split("; ")
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect();
    Some((
        "Folders an earlier version moved aside are back in RetroBat's roms folder.".into(),
        lines,
    ))
}

/// One chip per skipped system, its reason on hover (WS6.10). The list wraps
/// instead of running on as one line.
fn skipped_systems_chips(skipped: Vec<SkippedSystem>) -> impl IntoElement {
    h_flex()
        .flex_wrap()
        .gap_1()
        .children(skipped.into_iter().enumerate().map(|(index, system)| {
            Button::new(SharedString::from(format!("skipped-system-{index}")))
                .ghost()
                .small()
                .label(system.system)
                .tooltip(system.reason)
        }))
}

impl RommfsWindow {
    fn retrobat_install_step(
        &mut self,
        primary: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let installed = self.state.retrobat_root.clone();
        let running = self.state.installer_running;
        let ready = self.state.installer_ready;
        let button = |id: &'static str, label: &'static str, main: bool| {
            let b = Button::new(id).label(label);
            if main {
                b.primary()
            } else {
                b.outline()
            }
        };
        v_flex()
            .gap_2()
            .when_some(installed.clone(), |this, root| {
                this.child(muted(format!("Using {root}"), cx))
            })
            .when(installed.is_none(), |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .child(
                            button(
                                "download-retrobat",
                                if ready {
                                    "Download again"
                                } else {
                                    "Download installer"
                                },
                                primary && !ready,
                            )
                            .disabled(running)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.controller.send(Command::DownloadRetroBatInstaller);
                                cx.notify();
                            })),
                        )
                        .child(
                            button(
                                "launch-retrobat-installer",
                                "Launch installer…",
                                primary && ready,
                            )
                            .disabled(!ready || running)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.controller.send(Command::LaunchRetroBatInstaller);
                                cx.notify();
                            })),
                        )
                        .child(
                            button("show-retrobat-installer", "Show installer file", false)
                                .disabled(!ready)
                                .on_click(cx.listener(|view, _, _, _| {
                                    view.controller.send(Command::ShowRetroBatInstaller);
                                })),
                        )
                        .when(running, |this| {
                            this.child(
                                Button::new("cancel-retrobat-installer")
                                    .label("Cancel download")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.controller.send(Command::CancelRetroBatInstaller);
                                        cx.notify();
                                    })),
                            )
                        }),
                )
                .when(running, |this| {
                    let (received, total) = self.state.installer_progress;
                    this.child(step_progress(
                        "installer-progress",
                        total.filter(|t| *t > 0).map(|t| (received, t)),
                    ))
                })
                .when_some(self.state.installer_detail.clone(), |this, detail| {
                    this.child(muted(detail, cx))
                })
            })
            .when_some(
                self.state
                    .installer_error
                    .as_deref()
                    .filter(|_| installed.is_none())
                    .map(|raw| UserProblem::from_text("Couldn't get the RetroBat installer", raw)),
                |this, problem| {
                    this.child(problem_block(
                        "installer-problem",
                        &problem,
                        None,
                        window,
                        cx,
                    ))
                },
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(div().w_40().flex_shrink_0().child(if installed.is_some() {
                        "RetroBat installation"
                    } else {
                        "Already installed?"
                    }))
                    .child(
                        div().flex_1().min_w_0().child(
                            Select::new(&self.settings_install_select)
                                .w_full()
                                .accessibility_label("RetroBat installation")
                                .placeholder("Select a RetroBat installation…"),
                        ),
                    )
                    .child(
                        Button::new("settings-install-browse")
                            .label("Browse…")
                            .outline()
                            .on_click(
                                cx.listener(|view, _, _, cx| view.browse_settings_install(cx)),
                            ),
                    ),
            )
            .when_some(
                self.state.integration_error.as_deref().map(|raw| {
                    UserProblem::from_text("Couldn't use that RetroBat installation", raw)
                }),
                |this, problem| {
                    this.child(problem_block(
                        "integration-problem",
                        &problem,
                        None,
                        window,
                        cx,
                    ))
                },
            )
    }

    fn retrobat_link_step(
        &mut self,
        status: StepStatus,
        primary: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let blocked = match status {
            StepStatus::Blocked(reason) => Some(reason),
            _ => None,
        };
        let enabled = matches!(
            status,
            StepStatus::Ready | StepStatus::Failed | StepStatus::Done
        );
        let running = status == StepStatus::Running;
        let label = match status {
            StepStatus::Failed => "Retry setup",
            StepStatus::Done => "Set up again",
            _ => "Set up RetroBat",
        };
        let done = status == StepStatus::Done;
        let skipped = if done {
            self.state.retrobat_setup_skipped.clone()
        } else {
            Vec::new()
        };
        let restored = if done {
            self.state
                .retrobat_setup_detail
                .as_deref()
                .and_then(restored_note)
        } else {
            None
        };
        let mut action = Button::new("setup-retrobat")
            .label(label)
            .disabled(!enabled)
            .tooltip("Close RetroBat and its emulators first.");
        action = if primary && status != StepStatus::Done {
            action.primary()
        } else {
            action.outline()
        };
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(action.on_click(cx.listener(|view, _, _, cx| {
                        view.run_retrobat_setup(cx);
                    })))
                    .when_some(blocked, |this, reason| this.child(muted_xs(reason, cx))),
            )
            .when(running, |this| {
                this.child(step_progress("setup-progress", None))
                    .child(muted(
                        self.state
                            .retrobat_setup_detail
                            .clone()
                            .unwrap_or_else(|| "Setting up RetroBat…".into()),
                        cx,
                    ))
            })
            .when(done, |this| {
                this.child(status_marker(StatusKind::Success, "Connected", cx))
                    .when(!skipped.is_empty(), |this| {
                        this.child(muted(
                            format!("{} couldn't be linked", plural(skipped.len(), "system")),
                            cx,
                        ))
                        .child(skipped_systems_chips(skipped.clone()))
                    })
                    .when_some(restored, |this, (note, lines)| {
                        this.child(muted(note, cx)).child(notes_disclosure(
                            "setup-notes",
                            "Show details".into(),
                            lines,
                            window,
                            cx,
                        ))
                    })
            })
            .when_some(self.render_foreign_remediation(cx), |this, block| {
                this.child(block)
            })
            .when_some(
                self.state
                    .retrobat_setup_error
                    .as_deref()
                    .map(|raw| UserProblem::from_text("Couldn't finish RetroBat setup", raw)),
                |this, problem| {
                    this.child(problem_block("setup-problem", &problem, None, window, cx))
                },
            )
    }

    fn support_files_step(
        &mut self,
        status: StepStatus,
        primary: bool,
        locked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let blocked = match status {
            StepStatus::Blocked(reason) => Some(reason),
            _ => None,
        };
        let enabled = !locked && !matches!(status, StepStatus::Blocked(_) | StepStatus::Running);
        let running = self.state.auxiliary_running;
        let action = Button::new("materialize-auxiliary")
            .label("Download support files")
            .disabled(!enabled);
        let action = if primary {
            action.primary()
        } else {
            action.outline()
        };
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(action.on_click(cx.listener(|view, _, _, cx| {
                        view.controller.send(Command::MaterializeAuxiliary);
                        cx.notify();
                    })))
                    .child(info_button(
                        "support-files-help",
                        "BIOS files, Switch firmware and keys, and RetroArch cheats from your connected RomM library. Files are downloaded as-is; nothing you already have is overwritten.",
                    ))
                    .when(running, |this| {
                        this.child(
                            Button::new("cancel-auxiliary")
                                .label("Cancel")
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.controller.send(Command::CancelAuxiliary);
                                    cx.notify();
                                })),
                        )
                    })
                    .when_some(blocked, |this, reason| this.child(muted_xs(reason, cx))),
            )
            .when(running, |this| {
                let detail = self.state.auxiliary_detail.clone();
                this.child(step_progress(
                    "auxiliary-progress",
                    detail.as_deref().and_then(parse_progress_counts),
                ))
                .when_some(detail, |this, detail| this.child(muted(detail, cx)))
            })
            .when_some(
                self.state
                    .auxiliary_error
                    .as_deref()
                    .map(|raw| UserProblem::from_text("Couldn't download support files", raw)),
                |this, problem| {
                    this.child(problem_block("auxiliary-problem", &problem, None, window, cx))
                },
            )
            .when_some(
                self.state
                    .auxiliary_summary
                    .clone()
                    .filter(|_| !running),
                |this, summary| {
                    let (headline, note) = auxiliary_headline(
                        summary.published,
                        summary.unchanged,
                        &summary.warnings,
                    );
                    let lines: Vec<String> =
                        summary.warnings.iter().map(describe_auxiliary).collect();
                    this.child(
                        v_flex()
                            .gap_1()
                            .child(muted(headline, cx))
                            .when_some(note, |this, _| {
                                this.child(notes_disclosure(
                                    "auxiliary-notes",
                                    "What needs attention".into(),
                                    lines,
                                    window,
                                    cx,
                                ))
                            }),
                    )
                },
            )
            .when_some(
                self.state.frontend_metadata_detail.clone(),
                |this, detail| this.child(muted(detail, cx)),
            )
            .when_some(
                self.state.frontend_metadata_error.clone(),
                |this, error| this.child(error_line(error, cx)),
            )
    }
}
