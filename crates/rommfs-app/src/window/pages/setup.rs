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
    /// Setup follows the user's route from RomM through folder choice and
    /// optional RetroBat linking to the games folder.
    fn render_setup(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mounted = self.state.mount == MountState::Mounted;
        let mounting = self.state.mount == MountState::Mounting;
        let connected = self.state.conn == ConnectionState::Connected;
        let (mount_text, mount_kind) = self.mount_status(cx);
        let has_retrobat = self.settings_install_root(cx).is_some();
        let mount_path_ready = !self.mount_input.read(cx).value().trim().is_empty();
        let needs_retrobat_setup = self.integration_choice == 1
            && (!self.state.retrobat_setup_ok || !self.state.retrobat_setup_issues.is_empty());
        v_flex()
            .gap_6()
            .child(page_header(
                "Welcome to RomMFS",
                "Connect your RomM library, choose where games appear, then start the games folder. Game files download only when opened.",
                None,
                cx,
            ))
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
                    .child(section_heading("2. Choose folders and RetroBat"))
                    .child(muted("Choose an empty games folder. RomMFS will put your library here without moving your existing games.", cx))
                    .when(!connected, |this| this.child(muted("Connect to RomM above to continue.", cx)))
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
                                    .disabled(mounted || mounting)
                                    .on_click(cx.listener(|view, _, _, cx| view.browse_games_folder(cx))),
                            ),
                    )
                    .child(self.layout_radio(mounted || mounting, false, cx))
                    .when(self.integration_choice == 1, |this| {
                        this.child(Select::new(&self.settings_install_select).w_full()
                            .accessibility_label("RetroBat installation")
                            .placeholder("Choose a RetroBat installation…")
                            .disabled(mounted || mounting))
                            .child(h_flex().gap_2()
                                .child(Button::new("setup-browse-retrobat")
                                    .label("Browse…")
                                    .outline()
                                    .disabled(mounted || mounting)
                                    .on_click(cx.listener(|view, _, _, cx| view.browse_settings_install(cx))))
                                .child(Button::new("setup-connect-retrobat")
                                    .label(if self.state.retrobat_setup_running { "Connecting…" } else if needs_retrobat_setup { "Connect RetroBat" } else { "Connected" })
                                    .outline()
                                    .disabled(!connected || !has_retrobat || !mount_path_ready || !needs_retrobat_setup || mounted || mounting || self.state.retrobat_setup_running)
                                    .on_click(cx.listener(|view, _, _, cx| view.run_retrobat_setup(cx))))
                            )
                            .when_some(self.state.retrobat_setup_error.clone(), |this, error| {
                                this.child(muted(failure_explanation(&error), cx))
                            })
                    }),
            )
            .child(Separator::horizontal())
            .child(
                v_flex()
                    .gap_3()
                    .child(section_heading("3. Start the games folder"))
                    .child(muted(
                        if !connected {
                            "Connect to RomM above before starting the games folder."
                        } else if !mount_path_ready {
                            "Choose a games folder above before starting."
                        } else if needs_retrobat_setup {
                            "Connect the selected RetroBat installation before starting the games folder."
                        } else {
                            "Your folders are ready. Start the games folder when you want to play."
                        },
                        cx,
                    ))
                    .child(
                        h_flex()
                            .gap_3()
                            .items_center()
                            .child(
                                Button::new("setup-start-folder")
                                    .label(if mounted {
                                        "Games are available"
                                    } else if mounting {
                                        "Preparing games…"
                                    } else {
                                        "Start games folder"
                                    })
                                    .primary()
                                    .disabled(!connected || !mount_path_ready || needs_retrobat_setup || mounted || mounting)
                                    .on_click(cx.listener(|view, _, _, cx| view.on_start_mount(cx))),
                            )
                            .child(status_marker(mount_kind, mount_text, cx)),
                    )
                    .when(mounting, |this| match self.state.mount_progress.clone() {
                        Some((stage, completed, total, elapsed)) => this.child(operation_progress(
                            "setup-mount-progress",
                            stage_label(&stage),
                            completed,
                            total,
                            elapsed,
                            cx,
                        )),
                        None => this.child(Progress::new("setup-mount-progress").loading(true).small()),
                    })
                    .when_some(self.state.mount_error.clone(), |this, error| {
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
                    .when_some(self.render_foreign_remediation(cx), |this, block| this.child(block))
                    .child(self.settings_nav_button(
                        "setup-advanced",
                        "More options in Settings",
                        1,
                        false,
                        cx,
                    )),
            )
    }
}
