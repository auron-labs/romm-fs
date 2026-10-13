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
        let needs_sign_in = self.state.conn == ConnState::SignInRequired;
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
                        .child(
                            h_flex()
                                .justify_between()
                                .child(section_heading("RomM connection"))
                                .child(status_marker(conn_kind, conn_text, cx)),
                        )
                        .when_some(self.state.conn_error.clone(), |this, error| {
                            this.child(error_line(error, cx))
                        })
                        .child(self.nav_button(
                            "overview-connection",
                            if needs_sign_in {
                                "Sign in"
                            } else {
                                "Connection settings"
                            },
                            3,
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
                    .child(
                        h_flex()
                            .justify_between()
                            .child(section_heading("Save sync"))
                            .child(status_marker(sync_kind, sync_text, cx)),
                    )
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
                                    "{} file{} found that aren't linked to a game yet",
                                    fmt_count(unlinked),
                                    if unlinked == 1 { "" } else { "s" }
                                ),
                                cx,
                            ))
                        },
                    )
                    .child(self.nav_button("overview-saves", "Review saves", 1, false, cx)),
            )
            .child(Separator::horizontal())
            .child(self.render_overview_retrobat(cx))
    }

    /// §3 RetroBat summary: installed directory, running/error state, and
    /// the manage action (the install buttons live in Settings).
    fn render_overview_retrobat(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let (text, kind) = if self.state.retrobat_setup_running {
            ("Setting up\u{2026}", StatusKind::Warning)
        } else if self.state.retrobat_setup_error.is_some() {
            ("Setup failed \u{2014} see Settings", StatusKind::Danger)
        } else if self.state.retrobat_setup_ok {
            ("Ready", StatusKind::Success)
        } else if self.state.retrobat_root.is_some() {
            ("Finish setup in Settings", StatusKind::Warning)
        } else {
            ("Not set up", StatusKind::Muted)
        };
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .justify_between()
                    .child(section_heading("RetroBat"))
                    .child(status_marker(kind, text, cx)),
            )
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
            .child(self.nav_button("overview-retrobat", "RetroBat settings", 3, false, cx))
    }
}
