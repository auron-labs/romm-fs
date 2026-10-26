use super::super::*;

pub fn render(
    view: &mut RommfsWindow,
    window: &mut Window,
    cx: &mut Context<RommfsWindow>,
) -> impl IntoElement {
    view.render_overview(window, cx)
}

impl RommfsWindow {
    fn render_overview(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let needs_sign_in = self.state.conn == ConnectionState::SignInRequired;
        let show_connection = needs_sign_in || self.state.conn_error.is_some();
        v_flex()
            .gap_4()
            .child(page_header(
                "Overview",
                "Your library and anything that needs attention.",
                None,
                cx,
            ))
            .when(show_connection, |this| {
                let problem = self.state.conn_error.as_deref().map_or_else(
                    || {
                        UserProblem::error(
                            "Sign in to RomM",
                            Some("Sign in again to reconnect your library.".into()),
                            None,
                        )
                    },
                    |error| UserProblem::from_text("Couldn't connect to RomM", error),
                );
                this.child(problem_block(
                    "overview-connection-problem",
                    &problem,
                    Some(self.settings_nav_button(
                        "overview-connection",
                        "Connection settings",
                        0,
                        needs_sign_in,
                        cx,
                    )),
                    window,
                    cx,
                ))
            })
            .child(self.render_mount(window, cx))
            .child(self.render_overview_retrobat(window, cx))
            .child(self.render_overview_saves(cx))
            .when(
                self.state.integration == IntegrationMode::RetroBat,
                |this| this.child(self.render_overview_metadata(window, cx)),
            )
    }

    fn render_overview_saves(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let (text, kind) = self.save_sync_pill();
        let review_count = self.state.save_sync_review_count();
        let description = if review_count > 0 {
            format!(
                "{} {} your review. Open Saves to see what happened and choose the next step.",
                plural(review_count, "save"),
                if review_count == 1 { "needs" } else { "need" }
            )
        } else if !self.state.save_sync_enabled {
            "Automatic sync is off. You can enable it in Saves to copy supported saves to RomM."
                .into()
        } else {
            self.state.save_sync_summary_text()
        };
        let action = Button::new("overview-saves")
            .label(if review_count > 0 {
                "Review saves"
            } else {
                "View saves"
            })
            .outline()
            .small()
            .on_click(cx.listener(|view, _, window, cx| {
                view.active_view = 1;
                view.save_review_filter = usize::from(view.state.save_sync_review_count() > 0);
                if view.save_review_filter == 1 {
                    view.save_review_search
                        .update(cx, |input, cx| input.set_value("", window, cx));
                    view.save_review_page = 0;
                }
                cx.notify();
            }));
        card(cx).p_4().child(
            v_flex()
                .gap_2()
                .child(status_row("Saves", kind, text, Some(action), cx))
                .child(muted(description, cx))
                .when_some(self.state.save_sync_attention(), |this, error| {
                    this.child(muted(failure_explanation(&error), cx))
                }),
        )
    }

    fn render_overview_retrobat(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let migration = self.state.retrobat_setup_migration
            && !self.state.retrobat_setup_ok
            && !self.state.retrobat_setup_running;
        let (text, kind, description) = if self.state.integration != IntegrationMode::RetroBat {
            (
                "Optional",
                StatusKind::Muted,
                "Use RetroBat to browse and play your RomM games.",
            )
        } else if self.state.retrobat_setup_running {
            (
                "Setting up…",
                StatusKind::Muted,
                "Connecting your RomM games to RetroBat.",
            )
        } else if self.state.retrobat_setup_error.is_some() {
            (
                "Needs attention",
                StatusKind::Danger,
                "RetroBat setup couldn't finish. Open its settings to try again.",
            )
        } else if self.state.retrobat_setup_ok {
            (
                "Setup complete",
                StatusKind::Success,
                "Open RetroBat to browse your library. Keep RomMFS running while you play.",
            )
        } else if migration {
            (
                "Update needed",
                StatusKind::Warning,
                "Update the RetroBat connection before making games available.",
            )
        } else {
            (
                "Setup needed",
                StatusKind::Muted,
                "Choose your RetroBat installation and connect it to your library.",
            )
        };
        let action = self
            .settings_nav_button(
                "overview-retrobat",
                if self.state.retrobat_setup_ok {
                    "RetroBat settings"
                } else {
                    "Set up RetroBat"
                },
                2,
                false,
                cx,
            )
            .small();
        card(cx).p_4().child(
            v_flex().gap_2()
                .child(status_row("RetroBat", kind, text, Some(action), cx))
                .child(muted(description, cx))
                .when(migration, |this| {
                    this.child(h_flex().child(
                        Button::new("overview-retrobat-update")
                            .label("Update RetroBat connection")
                            .outline()
                            .on_click(cx.listener(|view, _, _, cx| view.run_retrobat_setup(cx))),
                    ))
                })
                .when_some(self.state.retrobat_setup_error.clone(), |this, error| {
                    this.child(muted(failure_explanation(&error), cx))
                        .child(details_disclosure("overview-retrobat-error", error, window, cx))
                })
                .when_some(self.state.play_data_problem.clone(), |this, problem| {
                    this.child(muted(
                        if problem.contains("403") {
                            "RomM denied access to favourites and play history. Check your account permissions in RomM."
                        } else {
                            "Favourites and play history couldn't update. Check your RomM connection."
                        },
                        cx,
                    ))
                    .child(details_disclosure("play-data-problem", problem, window, cx))
                    .child(h_flex().child(self.settings_nav_button("play-data-connection", "Connection settings", 0, false, cx)))
                }),
        )
    }

    fn render_overview_metadata(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let summary = self.state.frontend_metadata_summary();
        let (kind, text) = match summary {
            FrontendMetadataSummary::NotStarted => (StatusKind::Muted, "Not updated yet"),
            FrontendMetadataSummary::CheckingArtwork | FrontendMetadataSummary::Publishing => {
                (StatusKind::Muted, "Updating…")
            }
            FrontendMetadataSummary::Waiting => (StatusKind::Warning, "Waiting for access"),
            FrontendMetadataSummary::Failed => (StatusKind::Danger, "Update failed"),
            FrontendMetadataSummary::UpToDate => (StatusKind::Success, "Up to date"),
        };
        let retry = (summary == FrontendMetadataSummary::Failed).then(|| {
            Button::new("overview-metadata-retry")
                .label("Retry update")
                .accessibility_label("Retry artwork and game information update")
                .outline()
                .small()
                .on_click(cx.listener(|view, _, _, cx| {
                    view.controller.send(Command::RefreshMetadata);
                    cx.notify();
                }))
        });
        card(cx).p_4().child(
            v_flex().gap_2()
                .child(status_row("Artwork and game information", kind, text, retry, cx))
                .when(self.state.frontend_metadata_pending, |this| {
                    this.child(metadata_progress("overview-metadata-progress", &self.state, cx))
                })
                .when(summary == FrontendMetadataSummary::NotStarted, |this| {
                    this.child(muted("Updates automatically when your games are available in RetroBat.", cx))
                })
                .when(summary == FrontendMetadataSummary::UpToDate, |this| {
                    this.child(muted(
                        self.state.frontend_metadata_updated_at.map_or_else(
                            || "Artwork and game information are ready in RetroBat.".into(),
                            |at| format!("Updated {}. Artwork and game information are ready in RetroBat.", relative_time(at)),
                        ),
                        cx,
                    ))
                })
                .when_some(self.state.frontend_metadata_error.clone(), |this, error| {
                    this.child(muted(failure_explanation(&error), cx))
                        .when(self.state.mount == MountState::Mounted && !self.state.mount_stopping, |this| {
                            this.child(muted("Your games remain available. Artwork and game information may be out of date.", cx))
                        })
                        .child(details_disclosure("overview-metadata-error", error, window, cx))
                }),
        )
    }
}
