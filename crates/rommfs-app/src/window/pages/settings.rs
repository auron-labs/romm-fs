use super::super::*;
use crate::controller::{parse_progress_counts, RetroBatStep, StepStatus};

/// Settings page: connection, folders, RetroBat, background operation, and
/// Advanced details (UI-PLAN §4).
pub fn render(
    view: &mut RommfsWindow,
    window: &mut Window,
    cx: &mut Context<RommfsWindow>,
) -> impl IntoElement {
    if std::mem::take(&mut view.scroll_to_retrobat) {
        // The anchor records its position while painting; this render
        // paints it, so the jump lands on the next frame.
        view.retrobat_anchor.scroll_to(window, cx);
    }
    view.render_settings(window, cx)
}

impl RommfsWindow {
    /// Settings in the plan's order (UI-PLAN §Settings organization):
    /// connection, folders, the RetroBat flow, background operation, then
    /// the Advanced disclosure.
    fn render_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locked = self.state.retrobat_setup_running
            || self.state.auxiliary_running
            || matches!(self.state.mount, MountState::Mounted | MountState::Mounting);
        v_flex()
            .gap_6()
            .child(page_heading("Settings"))
            .child(
                v_flex()
                    .gap_3()
                    .child(section_heading("Connection"))
                    .child(self.render_connection(window, cx)),
            )
            .child(Separator::horizontal())
            .child(self.render_folders_section(locked, window, cx))
            .child(Separator::horizontal())
            .child(self.render_retrobat_section(locked, window, cx))
            .child(Separator::horizontal())
            .child(
                v_flex()
                    .gap_3()
                    .child(section_heading("Background operation"))
                    .child(self.render_background_panel(cx)),
            )
            .child(Separator::horizontal())
            .child(
                Collapsible::new()
                    .open(self.advanced_open)
                    .child(self.disclosure_row(
                        "settings-advanced-disclosure",
                        "Advanced",
                        self.advanced_open,
                        |view| view.advanced_open = !view.advanced_open,
                        window,
                        cx,
                    ))
                    .content(self.render_advanced_detail(window, cx)),
            )
    }

    /// Folders: the games folder once, the layout choice, and the custom
    /// save-data folder behind its own disclosure (UI-PLAN §Settings).
    fn render_folders_section(
        &mut self,
        locked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .gap_3()
            .child(section_heading("Folders"))
            .child(field_column(
                "Games folder",
                Input::new(&self.mount_input).w_full(),
                cx,
            ))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new("settings-browse-folder")
                            .label("Browse…")
                            .outline()
                            .disabled(locked)
                            .on_click(cx.listener(|view, _, _, cx| view.browse_games_folder(cx))),
                    )
                    .child(
                        Button::new("apply-games-folder")
                            .label("Choose games folder")
                            .outline()
                            .disabled(
                                locked
                                    || self.state.conn != ConnectionState::Connected
                                    || self.mount_input.read(cx).value().trim().is_empty(),
                            )
                            .on_click(cx.listener(|view, _, _, cx| {
                                let path =
                                    view.mount_input.read(cx).value().trim().to_string();
                                view.controller.send(Command::SetMountRoot { path });
                                cx.notify();
                            })),
                    )
                    .when(
                        locked || self.state.conn != ConnectionState::Connected,
                        |this| {
                            // §6: the disabled action explains why.
                            this.child(muted_xs(
                                if locked {
                                    "Stop the games folder to change its location."
                                } else {
                                    "Connect to RomM first."
                                },
                                cx,
                            ))
                        },
                    ),
            )
            .child(self.layout_radio(locked, cx))
            .when_some(
                self.state
                    .save_sync_effective_saves_root
                    .clone()
                    .or_else(|| self.state.save_sync_selected_root.clone()),
                |this, root| {
                    this.child(muted_xs(format!("Save data is kept in {root}"), cx))
                },
            )
            .child(
                Collapsible::new()
                    .open(self.custom_saves_open)
                    .child(self.disclosure_row(
                        "custom-saves-disclosure",
                        "Use a different save folder",
                        self.custom_saves_open,
                        |view| view.custom_saves_open = !view.custom_saves_open,
                        window,
                        cx,
                    ))
                    .content(
                        v_flex()
                            .gap_2()
                            .pt_2()
                            .child(field_column(
                                "Save-data folder",
                                Input::new(&self.save_folder_input).w_full(),
                                cx,
                            ))
                            .child(
                                Button::new("choose-save-folder")
                                    .label("Use save folder")
                                    .outline()
                                    .disabled(self.state.conn != ConnectionState::Connected)
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.controller.send(Command::SelectSaveFolder {
                                            path: PathBuf::from(
                                                view.save_folder_input
                                                    .read(cx)
                                                    .value()
                                                    .trim(),
                                            ),
                                        });
                                        cx.notify();
                                    })),
                            )
                            .child(muted_xs(
                                "Select saves only, not an emulator's entire portable folder. All file formats and companions are supported. Shared saves and memory cards remain excluded.",
                                cx,
                            )),
                    ),
            )
    }
}

/// Header line of one step: number and title with its status on the right.
fn step_header(number: u8, title: &'static str, status: StepStatus, cx: &App) -> impl IntoElement {
    let (kind, text) = match status {
        StepStatus::Done => (StatusKind::Success, "Done"),
        StepStatus::Ready => (StatusKind::Warning, "Next"),
        StepStatus::Running => (StatusKind::Warning, "In progress"),
        StepStatus::Failed => (StatusKind::Danger, "Needs attention"),
        StepStatus::Blocked(_) => (StatusKind::Muted, "Waiting"),
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

/// Splits the setup result into a headline and the per-system notes that
/// the earlier setup recorded as non-fatal (systems left as they are).
fn setup_result_notes(detail: &str) -> (Option<&'static str>, Vec<String>) {
    let body = detail.split_once(": ").map_or(detail, |(_, rest)| rest);
    if !body.contains("left as-is") {
        return (None, Vec::new());
    }
    let lines = body
        .split("; ")
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect();
    (
        Some("Some systems already had files in RetroBat's roms folder and were left as they are."),
        lines,
    )
}

impl RommfsWindow {
    /// The RetroBat section: install, connect to the games folder, optional
    /// support files — a status per step and one highlighted action.
    pub(in crate::window) fn render_retrobat_section(
        &mut self,
        locked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let steps = self.state.retrobat_steps();
        let primary = steps.primary();
        let card = |cx: &App| {
            v_flex()
                .gap_2()
                .p_3()
                .border_1()
                .border_color(cx.theme().border)
                .rounded(cx.theme().radius)
        };
        let install = card(cx)
            .child(step_header(1, "Install RetroBat", steps.install, cx))
            .child(self.retrobat_install_step(primary == Some(RetroBatStep::Install), window, cx));
        let link = card(cx)
            .child(step_header(
                2,
                "Connect RetroBat to your games folder",
                steps.link,
                cx,
            ))
            .child(self.retrobat_link_step(
                steps.link,
                primary == Some(RetroBatStep::Link),
                window,
                cx,
            ));
        let support = card(cx)
            .child(step_header(
                3,
                "Support files (optional)",
                steps.support,
                cx,
            ))
            .child(self.support_files_step(
                steps.support,
                primary == Some(RetroBatStep::Support),
                locked,
                window,
                cx,
            ));
        v_flex()
            .id("settings-retrobat")
            .anchor_scroll(Some(self.retrobat_anchor.clone()))
            .gap_3()
            .child(section_heading("RetroBat (optional)"))
            .child(muted(
                "RetroBat is a separate game launcher. If you use it, these steps connect it to your games folder. Your games folder works without it.",
                cx,
            ))
            .child(install)
            .child(link)
            .child(support)
    }

    fn retrobat_install_step(
        &mut self,
        primary: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let installed = self.state.retrobat_root.clone();
        let running = self.state.installer_running;
        let ready = self.state.installer_ready;
        let version = self.state.save_sync_profile_version.clone();
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
                this.child(muted(
                    match version {
                        Some(v) => format!("Using {root} (version {v})"),
                        None => format!("Using {root}"),
                    },
                    cx,
                ))
            })
            .when(installed.is_none(), |this| {
                this.child(muted(
                    "Download the official installer and run it, or choose a RetroBat you already have.",
                    cx,
                ))
                .child(
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
                            button("launch-retrobat-installer", "Launch installer…", primary && ready)
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
                    this.child(problem_block("installer-problem", &problem, None, window, cx))
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
                            .on_click(cx.listener(|view, _, _, cx| view.browse_settings_install(cx))),
                    ),
            )
            .when_some(
                self.state
                    .integration_error
                    .as_deref()
                    .map(|raw| UserProblem::from_text("Couldn't use that RetroBat installation", raw)),
                |this, problem| {
                    this.child(problem_block("integration-problem", &problem, None, window, cx))
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
        let stop_needed = blocked == Some("Stop the games folder first.");
        let done_notes = (status == StepStatus::Done)
            .then(|| {
                self.state
                    .retrobat_setup_detail
                    .as_deref()
                    .map(setup_result_notes)
            })
            .flatten();
        v_flex()
            .gap_2()
            .child(muted(
                "Adds a link in RetroBat's roms folder for each system, pointing at your games folder, and configures Switch saves. Systems that already have files there are left as they are. Stop RetroBat and its emulators first; nothing existing is moved or removed.",
                cx,
            ))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child({
                        let b = Button::new("setup-retrobat").label(label).disabled(!enabled);
                        let b = if primary && status != StepStatus::Done {
                            b.primary()
                        } else {
                            b.outline()
                        };
                        b.on_click(cx.listener(|view, _, _, cx| {
                            if let Some(installation) = view.settings_install_root(cx) {
                                view.controller.send(Command::SetupRetroBat {
                                    installation,
                                    managed_root: PathBuf::from(
                                        view.mount_input.read(cx).value().trim(),
                                    ),
                                });
                            }
                            cx.notify();
                        }))
                    })
                    .when_some(blocked.filter(|_| !stop_needed), |this, reason| {
                        this.child(muted_xs(reason, cx))
                    })
                    .when(stop_needed, |this| this.child(self.locked_reason(cx))),
            )
            .when(running, |this| {
                this.child(step_progress("setup-progress", None)).child(muted(
                    self.state
                        .retrobat_setup_detail
                        .clone()
                        .unwrap_or_else(|| "Setting up RetroBat…".into()),
                    cx,
                ))
            })
            .when_some(done_notes, |this, (note, lines)| {
                this.child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(cx.theme().success)
                        .child("RetroBat is connected to your games folder."),
                )
                .when_some(note, |this, note| {
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
        v_flex()
            .gap_2()
            .child(muted(
                "BIOS files, Switch firmware and keys, and RetroArch cheats from your connected RomM library. Files are downloaded as-is, nothing is extracted, nothing you already have is overwritten, and these are never save data. Stop emulators before continuing.",
                cx,
            ))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child({
                        let b = Button::new("materialize-auxiliary")
                            .label("Download support files")
                            .disabled(!enabled);
                        let b = if primary { b.primary() } else { b.outline() };
                        b.on_click(cx.listener(|view, _, _, cx| {
                            view.controller.send(Command::MaterializeAuxiliary);
                            cx.notify();
                        }))
                    })
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
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(cx.theme().success)
                                    .child(headline),
                            )
                            .when_some(note, |this, note| {
                                this.child(muted(note, cx)).child(notes_disclosure(
                                    "auxiliary-notes",
                                    "Show what needs attention".into(),
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

impl RommfsWindow {
    /// A disclosure row is one interactive control: pointer, Enter, Space,
    /// and a visible focus ring all toggle the same open state.
    fn disclosure_row(
        &mut self,
        id: &'static str,
        title: &'static str,
        open: bool,
        toggle: fn(&mut Self),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let focus_handle = window
            .use_keyed_state(SharedString::from(format!("{id}-focus")), cx, |_, cx| {
                cx.focus_handle().tab_stop(true)
            })
            .read(cx)
            .clone();
        let focused = focus_handle.is_focused(window);
        h_flex()
            .id(id)
            .key_context("Disclosure")
            .track_focus(&focus_handle)
            .on_action(cx.listener(move |view, _: &ActivateControl, _window, cx| {
                toggle(view);
                cx.notify();
            }))
            .on_click(cx.listener(move |view, _, _, cx| {
                toggle(view);
                cx.notify();
            }))
            .w_full()
            .gap_2()
            .h_8()
            .px_3()
            .border_1()
            .border_color(if focused {
                cx.theme().primary
            } else {
                cx.theme().border
            })
            .rounded(cx.theme().radius)
            .child(
                Icon::new(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .small()
                .text_color(cx.theme().muted_foreground),
            )
            .child(title)
    }
    /// Start-at-login plus the honest close/quit description.
    fn render_background_panel(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let login_supported = cfg!(windows);
        let needs_connect =
            !self.state.start_at_login && self.state.conn != ConnectionState::Connected;
        let login_disabled = !login_supported || needs_connect;
        let login_reason = if !login_supported {
            "Start at login is available on Windows only."
        } else if needs_connect {
            "Connect to RomM with a remembered sign-in first."
        } else {
            ""
        };
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        Switch::new("start-at-login")
                            .checked(self.state.start_at_login)
                            .label("Start at login")
                            .disabled(login_disabled)
                            .on_change(cx.listener(move |view, checked, _, cx| {
                                if login_disabled {
                                    return;
                                }
                                view.controller.send(Command::SetStartAtLogin {
                                    enabled: *checked,
                                });
                                cx.notify();
                            })),
                    )
                    .when(login_disabled, |this| {
                        this.child(muted_xs(login_reason, cx))
                    }),
            )
            .child(muted(
                format!(
                    "{} Login startup reconnects and starts the games folder before resuming previously authorized save sync.",
                    close_behavior_copy(self.tray_hides)
                ),
                cx,
            ))
            .when_some(self.state.startup_error.clone(), |this, error| {
                this.child(error_line(error, cx))
            })
    }
    fn render_advanced_detail(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let can_apply_debounce = self.state.save_sync_available || self.state.save_sync_enabled;
        let mut detail = v_flex()
            .gap_2()
            .pt_2()
            .child(muted_xs(
                "Changed files upload automatically. Remote-only changes replace an unchanged synchronized baseline; simultaneous changes and active files are kept for review. Local deletion never deletes remote saves.",
                cx,
            ))
            .child(muted_xs(
                if self.tray_hides {
                    "Sync continues while RomMFS runs, including in the tray."
                } else {
                    "Sync continues while RomMFS runs."
                },
                cx,
            ))
            .child(
                h_flex()
                    .gap_3()
                    .items_end()
                    .child(div().w_40().child(field_column(
                        "Wait after a save changes (seconds)",
                        Input::new(&self.debounce_input).w_full().small(),
                        cx,
                    )))
                    .child(
                        Button::new("save-sync-apply-debounce")
                            .label("Apply")
                            .outline()
                            .small()
                            .disabled(!can_apply_debounce)
                            .on_click(cx.listener(|view, _, _, cx| view.apply_save_sync_debounce(cx))),
                    ),
            );
        if let Some(error) = self.debounce_error.clone() {
            detail = detail.child(error_line(error, cx));
        }
        detail
    }
}
