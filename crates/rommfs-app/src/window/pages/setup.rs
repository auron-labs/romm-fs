use super::super::*;

/// First-run setup for an unconfigured install. Optional integrations stay
/// in Settings and are never prerequisites for starting the games folder.
pub fn render(
    view: &mut RommfsWindow,
    window: &mut Window,
    cx: &mut Context<RommfsWindow>,
) -> impl IntoElement {
    view.render_setup(window, cx)
}

impl RommfsWindow {
    /// First-run setup for an unconfigured install (UI-PLAN §3): connect to
    /// RomM, then choose the games folder. Optional integrations stay in
    /// Settings — they are never prerequisites here.
    fn render_setup(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mounted = self.state.mount == MountState::Mounted;
        let mounting = self.state.mount == MountState::Mounting;
        let (mount_text, mount_kind) = self.mount_status(cx);
        v_flex()
            .gap_6()
            .child(page_heading("Set up RomMFS"))
            .child(
                v_flex()
                    .gap_3()
                    .child(section_heading("1. Connect to RomM"))
                    .child(self.render_connection(window, cx)),
            )
            .child(Separator::horizontal())
            .child(
                v_flex()
                    .gap_3()
                    .child(section_heading("2. Choose the games folder"))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_end()
                            .child(div().flex_1().min_w_0().child(field_column(
                                "Games folder",
                                Input::new(&self.mount_input).w_full(),
                                cx,
                            )))
                            .child(
                                Button::new("setup-browse-folder")
                                    .label("Browse…")
                                    .outline()
                                    .on_click(
                                        cx.listener(|view, _, _, cx| view.browse_games_folder(cx)),
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .items_center()
                            .child(
                                Button::new("setup-start-folder")
                                    .label(if mounted {
                                        "Folder is running"
                                    } else {
                                        "Start games folder"
                                    })
                                    .primary()
                                    .disabled(mounted || mounting)
                                    .on_click(
                                        cx.listener(|view, _, _, cx| view.on_start_mount(cx)),
                                    ),
                            )
                            .child(status_marker(mount_kind, mount_text, cx)),
                    )
                    .when(mounting, |this| match self.state.mount_progress.clone() {
                        Some((stage, completed, total, elapsed)) => this.child(operation_progress(
                            "setup-mount-progress",
                            &stage,
                            completed,
                            total,
                            elapsed,
                            cx,
                        )),
                        None => this.child(Progress::new("setup-mount-progress").value(0.)),
                    })
                    .when_some(self.state.mount_error.clone(), |this, error| {
                        // §6 failure presentation; the Start action above
                        // is already this section's retry.
                        this.child(failure_block(
                            "setup-mount-failure",
                            "Couldn't start the games folder",
                            failure_explanation(&error),
                            None,
                            error,
                            window,
                            cx,
                        ))
                    })
                    .when_some(self.render_foreign_remediation(cx), |this, block| {
                        this.child(block)
                    })
                    .child(self.settings_nav_button(
                        "setup-advanced",
                        "More options in Settings",
                        1,
                        false,
                        cx,
                    )),
            )
            .child(Separator::horizontal())
            .child(
                v_flex()
                    .gap_3()
                    .child(section_heading("3. RetroBat (optional)"))
                    .child(div().child("Use RetroBat?"))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("setup-use-retrobat")
                                    .label("Set up RetroBat")
                                    .outline()
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.open_retrobat_settings(cx)
                                    })),
                            )
                            .child(
                                Button::new("setup-skip-retrobat")
                                    .label("Skip")
                                    .ghost()
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.active_view = 0;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
    }
}
