use super::super::*;

/// Overview: truthful summaries for the three subsystems, each with the
/// action its own state calls for (UI-PLAN §3).
pub fn render(
    view: &mut RommfsWindow,
    window: &mut Window,
    cx: &mut Context<RommfsWindow>,
) -> impl IntoElement {
    view.render_overview(window, cx)
}

impl RommfsWindow {
    /// Overview: truthful summaries for the three subsystems, each with the
    /// action its own state calls for (UI-PLAN §3).
    fn render_overview(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (conn_text, conn_kind) = self.conn_status(cx);
        let (sync_text, sync_kind) = self.save_sync_status();
        let needs_sign_in = self.state.conn == ConnectionState::SignInRequired;
        // §3: the connection block appears only for a live problem or
        // when sign-in is needed — a healthy connection adds no section.
        let show_connection = needs_sign_in || self.state.conn_error.is_some();
        let unlinked = self.state.save_sync_catalogue_unmapped;
        v_flex()
            .gap_6()
            .child(page_heading("Overview"))
            .when(show_connection, |this| {
                this.child(
                    v_flex()
                        .gap_2()
                        .child(status_row(
                            "RomM connection",
                            conn_kind,
                            conn_text,
                            None,
                            cx,
                        ))
                        .when_some(self.state.conn_error.clone(), |this, error| {
                            this.child(error_line(error, cx))
                        })
                        .child(self.settings_nav_button(
                            "overview-connection",
                            if needs_sign_in {
                                "Sign in"
                            } else {
                                "Connection settings"
                            },
                            0,
                            needs_sign_in,
                            cx,
                        )),
                )
                .child(Separator::horizontal())
            })
            .child(self.render_mount(window, cx))
            .child(Separator::horizontal())
            .child(
                v_flex()
                    .gap_2()
                    .child(status_row("Saves", sync_kind, sync_text, None, cx))
                    .when_some(
                        self.state
                            .save_sync_attention()
                            .or_else(|| self.state.save_sync_off_hint().map(str::to_owned)),
                        |this, problem| this.child(muted(problem, cx)),
                    )
                    .when(
                        self.state.save_sync_summary() == SaveSyncSummary::NoLinkedFiles
                            && unlinked > 0,
                        |this| {
                            this.child(muted(
                                format!(
                                    "{} found that aren't linked to a game yet",
                                    plural(unlinked, "file")
                                ),
                                cx,
                            ))
                        },
                    )
                    .child(
                        h_flex().child(
                            Button::new("overview-saves")
                                .label(if self.state.save_sync_review_count() > 0 {
                                    "Review saves"
                                } else {
                                    "View saves"
                                })
                                .outline()
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.active_view = 1;
                                    view.save_review_attention_only =
                                        view.state.save_sync_review_count() > 0;
                                    if view.save_review_attention_only {
                                        view.save_review_search.update(cx, |input, cx| {
                                            input.set_value("", window, cx)
                                        });
                                        view.save_review_page = 0;
                                    }
                                    cx.notify();
                                })),
                        ),
                    ),
            )
            .child(Separator::horizontal())
            .child(self.render_overview_retrobat(window, cx))
    }

    /// §3 RetroBat summary: installed directory, running/error state, and
    /// the manage action (the install buttons live in Settings). An earlier
    /// build's setup record gets an inline Update action that runs step 2.
    fn render_overview_retrobat(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let migration = self.state.retrobat_setup_migration
            && !self.state.retrobat_setup_ok
            && !self.state.retrobat_setup_running;
        let (text, kind) = if self.state.retrobat_setup_running {
            ("Setting up\u{2026}", StatusKind::Warning)
        } else if self.state.retrobat_setup_error.is_some() {
            ("Setup failed", StatusKind::Danger)
        } else if self.state.retrobat_setup_ok {
            ("Configured", StatusKind::Success)
        } else if migration {
            ("Needs a quick update", StatusKind::Warning)
        } else if self.state.retrobat_root.is_some() {
            ("Finish setup in Settings", StatusKind::Warning)
        } else {
            ("Not set up", StatusKind::Muted)
        };
        v_flex()
            .gap_2()
            .child(status_row("RetroBat", kind, text, None, cx))
            .when_some(self.state.retrobat_root.clone(), |this, root| {
                this.child(muted(root, cx))
            })
            .when_some(self.state.retrobat_setup_detail.clone(), |this, detail| {
                if self.state.retrobat_setup_running {
                    this.child(muted(detail, cx))
                } else {
                    this
                }
            })
            .child(
                h_flex()
                    .gap_2()
                    .when(migration, |this| {
                        this.child(
                            Button::new("overview-retrobat-update")
                                .label("Update")
                                .primary()
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.run_retrobat_setup(cx);
                                })),
                        )
                    })
                    .child(self.settings_nav_button(
                        "overview-retrobat",
                        "RetroBat settings",
                        2,
                        false,
                        cx,
                    )),
            )
            .when_some(self.state.play_data_problem.clone(), |this, problem| {
                this.child(status_row(
                    "Favourites and play history",
                    StatusKind::Warning,
                    if problem.contains("403") {
                        "RomM denied access to favourites and play history."
                    } else {
                        "Couldn't update favourites and play history"
                    },
                    None,
                    cx,
                ))
                .child(details_disclosure("play-data-problem", problem, window, cx))
                .child(self.settings_nav_button(
                    "play-data-connection",
                    "Connection settings",
                    0,
                    false,
                    cx,
                ))
            })
            .when(self.state.retrobat_setup_ok, |this| {
                this.child(Separator::horizontal())
                    .child(self.render_overview_metadata(window, cx))
            })
    }

    /// WS4.2: the frontend metadata stage with live counts while it runs, or
    /// "up to date · N ago", with a Retry after a failure.
    fn render_overview_metadata(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let error = self.state.frontend_metadata_error.clone();
        let pending = self.state.frontend_metadata_pending;
        let (kind, text) = if pending {
            (
                StatusKind::Warning,
                self.state
                    .frontend_metadata_detail
                    .clone()
                    .unwrap_or_else(|| "Updating…".into()),
            )
        } else if error.is_some() {
            (
                StatusKind::Danger,
                "Couldn't update artwork and game information".to_string(),
            )
        } else if let Some(at) = self.state.frontend_metadata_updated_at {
            (
                StatusKind::Success,
                format!("Up to date · {}", relative_time(at)),
            )
        } else {
            (StatusKind::Muted, "Not synced yet".to_string())
        };
        let retry = error.as_ref().filter(|_| !pending).map(|_| {
            Button::new("overview-metadata-retry")
                .label("Retry update")
                .accessibility_label("Retry artwork and game information update")
                .primary()
                .small()
                .on_click(cx.listener(|view, _, _, cx| {
                    view.controller.send(Command::RefreshMetadata);
                    cx.notify();
                }))
        });
        v_flex()
            .gap_2()
            .child(status_row("Artwork and game information", kind, text, retry, cx))
            .when(pending, |this| {
                this.child(match self.state.frontend_metadata_progress {
                    Some((completed, total)) if completed < total => v_flex()
                        .gap_1()
                        .child(muted(
                            format!(
                                "{} / {} artwork files checked",
                                fmt_count(completed),
                                fmt_count(total)
                            ),
                            cx,
                        ))
                        .child(
                            Progress::new("overview-metadata-progress")
                                .value(if total > 0 {
                                    (100.0 * completed as f32 / total as f32).min(100.0)
                                } else {
                                    0.0
                                })
                                .small(),
                        )
                        .into_any_element(),
                    progress => v_flex()
                        .gap_1()
                        .when(progress.is_some(), |this| {
                            this.child(muted("Artwork checks complete", cx))
                        })
                        .child(Progress::new("overview-metadata-progress").loading(true).small())
                        .into_any_element(),
                })
            })
            .when_some(error, |this, error| this.child(muted(if self.state.mount == MountState::Mounted && !self.state.mount_stopping { "Your games are available, but artwork and game information may be missing or out of date." } else { "Artwork and game information may be missing or out of date." }, cx)).child(details_disclosure("overview-metadata-error", error, window, cx)))
    }
}
