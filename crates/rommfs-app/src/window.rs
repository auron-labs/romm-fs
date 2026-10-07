//! The single GPUI window (PRD R5): connection form, mount controls,
//! save-sync setup, download progress, bounded scrollable log. Renders
//! `UiState` only — no timers, no fabricated progress, no business logic here.
//!
//! Built on `gpui-kit` (GPUI plus `gpui_kit::component`): text editing,
//! keyboard activation, and focus traversal come from the component
//! library; the view composes sections and forwards `Command`s.

use crate::controller::{Command, ConnState, Controller, MountState, UiState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    collapsible::Collapsible,
    h_flex,
    input::{Input, InputContentType, InputEvent, InputState},
    label::Label,
    progress::Progress,
    select::{Select, SelectEvent, SelectState},
    separator::Separator,
    try_parse_color, v_flex, ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _,
    StyledExt as _, Theme, ThemeMode,
};
use gpui_kit::{
    actions, div, point, prelude::*, px, size, App, Bounds, ClipboardItem,
    Context, Entity, Focusable as _, FontWeight, Hsla, KeyBinding, PathPromptOptions, ScrollHandle,
    SharedString, Subscription, WeakEntity, Window, WindowBounds, WindowOptions,
};
use rommfs_core::events::{AppEvent, Level, SaveSyncIncomingStatus};
use rommfs_core::save_sync::{
    ExistingSavePreview, ExistingSaveScanStatus, MAX_EXISTING_SAVE_SCAN_DEPTH,
    MAX_EXISTING_SAVE_SCAN_ENTRIES,
};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

actions!(
    rommfs,
    [
        /// Keyboard activation for the disclosure row (Enter/Space).
        ActivateControl,
    ]
);

const LOG_CAP: usize = 500;
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Semantic status roles mapped onto the active theme — no raw colors in
/// application code.
#[derive(Clone, Copy)]
enum StatusKind {
    Muted,
    Success,
    Warning,
    Danger,
}

impl StatusKind {
    fn color(self, cx: &App) -> Hsla {
        let theme = cx.theme();
        match self {
            StatusKind::Muted => theme.muted_foreground,
            StatusKind::Success => theme.success,
            StatusKind::Warning => theme.warning,
            StatusKind::Danger => theme.danger,
        }
    }
}

fn status_dot(color: Hsla) -> impl IntoElement {
    div().size_2().flex_shrink_0().rounded_full().bg(color)
}

/// Leading dot + state text used in the header and section status lanes.
fn status_marker(kind: StatusKind, text: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    h_flex()
        .gap_2()
        .child(status_dot(kind.color(cx)))
        .child(div().whitespace_nowrap().child(text.into()))
}

fn muted(text: impl Into<String>, cx: &App) -> impl IntoElement {
    div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

fn muted_xs(text: impl Into<String>, cx: &App) -> impl IntoElement {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

fn error_line(text: impl Into<String>, cx: &App) -> impl IntoElement {
    div()
        .text_sm()
        .text_color(cx.theme().danger)
        .child(text.into())
}

fn section_heading(text: &'static str) -> impl IntoElement {
    div()
        .text_base()
        .font_weight(FontWeight::SEMIBOLD)
        .child(text)
}

/// Label above a control, used by the connection row.
fn field_column(label: &'static str, control: impl IntoElement, cx: &App) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(
            Label::new(label)
                .text_xs()
                .text_color(cx.theme().muted_foreground),
        )
        .child(control)
}

fn fmt_count(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

// ---------------------------------------------------------------------------
// The window view: renders `UiState`, forwards clicks as `Command`s.
// ---------------------------------------------------------------------------

struct RommfsWindow {
    controller: Controller,
    event_rx: mpsc::Receiver<AppEvent>,
    state: UiState,
    url_input: Entity<InputState>,
    user_input: Entity<InputState>,
    password_input: Entity<InputState>,
    mount_input: Entity<InputState>,
    install_select: Entity<SelectState<Vec<SharedString>>>,
    debounce_input: Entity<InputState>,
    debounce_input_applied: String,
    debounce_error: Option<String>,
    mapping_open: bool,
    log_scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl RommfsWindow {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (controller, event_rx) = Controller::spawn();

        let url_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("http://romm.local:8080"));
        let user_input = cx.new(|cx| InputState::new(window, cx).placeholder("username"));
        let password_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("password")
                .masked(true)
        });
        // Drive roots reject directory creation for standard users on many
        // systems; the profile directory is always writable.
        let default_mount = std::env::var_os("USERPROFILE")
            .map(|profile| std::path::PathBuf::from(profile).join("RomM"))
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "C:\\RomM".to_string());
        let mount_input = cx.new(|cx| InputState::new(window, cx).default_value(default_mount));
        let install_select =
            cx.new(|cx| SelectState::new(Vec::<SharedString>::new(), None, window, cx));
        let debounce_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(rommfs_core::save_sync::DEFAULT_DEBOUNCE_SECS.to_string())
        });

        let mut subscriptions = Vec::new();

        // Enter in a connection field is the section's default commit.
        for input in [&url_input, &user_input, &password_input] {
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                |view, _state, event, _window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        view.on_connect(cx);
                    }
                },
            ));
        }

        subscriptions.push(cx.subscribe_in(
            &install_select,
            window,
            |view, _state, event, _window, cx| {
                let SelectEvent::Confirm(value) = event;
                let Some(value) = value else { return };
                let value = value.to_string();
                // Programmatic selection sync reports the same value; only
                // user picks that actually change the root become commands.
                if view
                    .state
                    .save_sync_selected_root
                    .as_deref()
                    .is_some_and(|root| root.eq_ignore_ascii_case(&value))
                {
                    return;
                }
                if let Some(candidate) = view.state.save_sync_candidates.iter().find(|candidate| {
                    candidate
                        .info
                        .install_root
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&value)
                }) {
                    let path = candidate.info.install_root.clone();
                    view.controller
                        .send(Command::SelectSaveSyncInstallation { path });
                    cx.notify();
                }
            },
        ));

        subscriptions.push(cx.subscribe_in(
            &debounce_input,
            window,
            |view, _state, event, _window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    view.apply_save_sync_debounce(cx);
                }
            },
        ));

        // Worker events arrive on the channel; poll it on the UI executor —
        // every applied event is a real fact, nothing fabricated (R5).
        cx.spawn_in(window, async move |this: WeakEntity<RommfsWindow>, cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;
                if this
                    .update_in(cx, |view, window, cx| view.drain_events(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        let view = Self {
            controller,
            event_rx,
            state: UiState::new(LOG_CAP),
            url_input,
            user_input,
            password_input,
            mount_input,
            install_select,
            debounce_input,
            debounce_input_applied: rommfs_core::save_sync::DEFAULT_DEBOUNCE_SECS.to_string(),
            debounce_error: None,
            mapping_open: false,
            log_scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        let focus = view.url_input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        view
    }

    /// Drain pending worker events into `UiState` and repaint if needed.
    fn drain_events(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut new_logs = false;
        let mut save_sync_updated = false;
        let mut changed = false;
        while let Ok(event) = self.event_rx.try_recv() {
            if matches!(event, AppEvent::Log(_)) {
                new_logs = true;
            }
            match &event {
                AppEvent::SaveSyncUpdated { .. } | AppEvent::SaveSyncSessionChanged { .. } => {
                    save_sync_updated = true;
                }
                _ => {}
            }
            match &event {
                AppEvent::SaveSyncSessionChanged { .. } => {
                    if self.debounce_input.read(cx).value() == self.debounce_input_applied {
                        let default = rommfs_core::save_sync::DEFAULT_DEBOUNCE_SECS.to_string();
                        self.debounce_input
                            .update(cx, |input, cx| input.set_value(default.clone(), window, cx));
                        self.debounce_input_applied = default;
                    }
                    self.debounce_error = None;
                }
                AppEvent::SaveSyncUpdated {
                    session_id,
                    debounce_secs,
                    ..
                } if *session_id == self.state.save_sync_session_id => {
                    let previous = self.debounce_input_applied.clone();
                    let next = debounce_secs.to_string();
                    if self.debounce_input.read(cx).value() == previous {
                        self.debounce_input
                            .update(cx, |input, cx| input.set_value(next.clone(), window, cx));
                    }
                    self.debounce_input_applied = next;
                }
                _ => {}
            }
            self.state.apply(&event);
            changed = true;
        }
        if save_sync_updated {
            self.sync_install_select(window, cx);
        }
        if new_logs {
            let max = self.log_scroll.max_offset();
            self.log_scroll.set_offset(point(px(0.), max.y));
        }
        if changed {
            cx.notify();
        }
    }

    /// Keep the installation dropdown in step with the latest discovery
    /// facts: same items, same selected root (matched case-insensitively,
    /// like the controller).
    fn sync_install_select(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let items: Vec<SharedString> = self
            .state
            .save_sync_candidates
            .iter()
            .map(|candidate| {
                SharedString::from(candidate.info.install_root.to_string_lossy().into_owned())
            })
            .collect();
        let selected = self
            .state
            .save_sync_selected_root
            .as_deref()
            .and_then(|root| {
                items
                    .iter()
                    .find(|item| item.eq_ignore_ascii_case(root))
                    .cloned()
            });
        self.install_select.update(cx, |state, cx| {
            state.set_items(items, window, cx);
            match selected {
                Some(value) => state.set_selected_value(&value, window, cx),
                None => state.set_selected_index(None, window, cx),
            }
        });
    }

    fn on_connect(&mut self, cx: &mut Context<Self>) {
        if self.state.conn == ConnState::Connecting {
            return;
        }
        self.controller.send(Command::Connect {
            url: self.url_input.read(cx).value().to_string(),
            username: self.user_input.read(cx).value().to_string(),
            password: self.password_input.read(cx).value().to_string(),
        });
    }

    fn on_start_mount(&mut self, cx: &mut Context<Self>) {
        if matches!(self.state.mount, MountState::Mounting | MountState::Mounted) {
            return;
        }
        let path = self.mount_input.read(cx).value().to_string();
        self.state.request_mount(path.clone());
        cx.notify();
        self.controller.send(Command::StartMount { path });
    }

    fn on_stop_mount(&mut self, _cx: &mut Context<Self>) {
        if self.state.mount != MountState::Mounted {
            return;
        }
        self.controller.send(Command::StopMount);
    }

    fn browse_save_sync(&mut self, cx: &mut Context<Self>) {
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
                            view.controller
                                .send(Command::SelectSaveSyncInstallation { path });
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

    fn apply_save_sync_debounce(&mut self, cx: &mut Context<Self>) {
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

    fn prompt_export(&mut self, incoming: SaveSyncIncomingStatus, cx: &mut Context<Self>) {
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

    fn pending_incoming_exists(&self, incoming_id: &str) -> bool {
        self.state.save_sync_queue.as_ref().is_some_and(|queue| {
            queue
                .incoming
                .iter()
                .any(|incoming| incoming.incoming_id == incoming_id)
        })
    }

    fn show_picker_error(&self, error: String) {
        self.controller
            .sink()
            .emit(AppEvent::log(Level::Error, "save-sync", error));
    }

    fn emit_export_error(&self, session_id: u64, incoming_id: &str, error: String) {
        self.controller
            .sink()
            .emit(AppEvent::SaveSyncExportFinished {
                session_id,
                incoming_id: incoming_id.to_owned(),
                destination: None,
                error: Some(error),
            });
    }

    fn on_copy_log(&mut self, cx: &mut Context<Self>) {
        let body = self
            .state
            .log
            .lines()
            .map(format_log_line)
            .collect::<Vec<_>>()
            .join("\n");
        cx.write_to_clipboard(ClipboardItem::new_string(body));
    }

    // --- render helpers ---

    fn conn_status(&self, cx: &App) -> (SharedString, StatusKind) {
        match self.state.conn {
            ConnState::Idle => ("Not connected".into(), StatusKind::Muted),
            ConnState::Connecting => ("Connecting…".into(), StatusKind::Warning),
            ConnState::Connected => {
                let user = self.user_input.read(cx).value();
                if user.is_empty() {
                    ("Connected".into(), StatusKind::Success)
                } else {
                    (format!("Connected · {user}").into(), StatusKind::Success)
                }
            }
            ConnState::Failed => ("Connection failed".into(), StatusKind::Danger),
            ConnState::SignInRequired => ("Sign-in required".into(), StatusKind::Danger),
        }
    }

    fn mount_status(&self, _cx: &App) -> (SharedString, StatusKind) {
        match self.state.mount {
            MountState::NotMounted => ("Not mounted".into(), StatusKind::Muted),
            MountState::Mounting => ("Mounting…".into(), StatusKind::Warning),
            MountState::Mounted => ("Mounted".into(), StatusKind::Success),
            MountState::Failed => ("Mount failed".into(), StatusKind::Danger),
        }
    }

    fn save_sync_status(&self) -> (&'static str, StatusKind) {
        if self.state.conn == ConnState::SignInRequired
            || self.state.save_sync_authentication_required
        {
            return ("Authentication required", StatusKind::Danger);
        }
        if self.state.conn == ConnState::Failed {
            return ("Offline", StatusKind::Danger);
        }
        if !self.state.save_sync_enabled {
            return ("Disabled", StatusKind::Muted);
        }
        if self
            .state
            .save_sync_queue
            .as_ref()
            .is_some_and(|queue| queue.network_paused)
        {
            return ("Paused", StatusKind::Danger);
        }
        if !self.state.save_sync_available {
            return ("Not ready", StatusKind::Danger);
        }
        let Some(queue) = self.state.save_sync_queue.as_ref() else {
            return ("Reconciling", StatusKind::Warning);
        };
        if queue.actor_failed {
            return ("Failed", StatusKind::Danger);
        }
        if queue.reconciled_games < queue.mapped_games {
            return ("Reconciling", StatusKind::Warning);
        }
        if queue.attention_games > 0 || queue.pending_incoming > 0 {
            return ("Review needed", StatusKind::Warning);
        }
        if queue.failure.is_some() {
            return ("Retrying", StatusKind::Warning);
        }
        if queue.pending_outbound > 0 {
            return ("Syncing", StatusKind::Warning);
        }
        ("Up to date", StatusKind::Success)
    }

    /// A disclosure row is one interactive control: pointer, Enter, Space,
    /// and a visible focus ring all toggle the same open state.
    fn disclosure_row(
        &mut self,
        id: &'static str,
        title: &'static str,
        open: bool,
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
            .on_action(cx.listener(|view, _: &ActivateControl, _window, cx| {
                view.mapping_open = !view.mapping_open;
                cx.notify();
            }))
            .on_click(cx.listener(|view, _, _, cx| {
                view.mapping_open = !view.mapping_open;
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

    fn render_header(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (conn_text, conn_kind) = self.conn_status(cx);
        h_flex()
            .justify_between()
            .child(
                div()
                    .text_2xl()
                    .font_weight(FontWeight::BOLD)
                    .child("RomMFS"),
            )
            .child(status_marker(conn_kind, conn_text, cx))
    }

    fn render_connection(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let connecting = self.state.conn == ConnState::Connecting;
        v_flex()
            .gap_2()
            .child(section_heading("Connection"))
            .child(
                h_flex()
                    .gap_3()
                    .items_end()
                    .child(div().flex_1().min_w_0().child(field_column(
                        "Server URL",
                        Input::new(&self.url_input).w_full(),
                        cx,
                    )))
                    .child(div().w_48().child(field_column(
                        "Username",
                        Input::new(&self.user_input).w_full(),
                        cx,
                    )))
                    .child(
                        div().w_48().child(field_column(
                            "Password",
                            Input::new(&self.password_input)
                                .w_full()
                                .content_type(InputContentType::Password)
                                .mask_toggle(),
                            cx,
                        )),
                    )
                    .child(
                        Button::new("connect")
                            .label("Connect")
                            .primary()
                            .loading(connecting)
                            .disabled(connecting)
                            .on_click(cx.listener(|view, _, _, cx| view.on_connect(cx))),
                    ),
            )
            .when_some(self.state.conn_error.clone(), |this, error| {
                this.child(error_line(error, cx))
            })
    }

    fn render_mount(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mounted = self.state.mount == MountState::Mounted;
        let mounting = self.state.mount == MountState::Mounting;
        let (mount_text, mount_kind) = self.mount_status(cx);
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .justify_between()
                    .child(section_heading("ROM mount"))
                    .child(status_marker(mount_kind, mount_text, cx)),
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(div().w_40().flex_shrink_0().child("Mount folder"))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&self.mount_input).w_full()),
                    )
                    .child(
                        Button::new("start-mount")
                            .label("Start")
                            .outline()
                            .disabled(mounted || mounting)
                            .on_click(cx.listener(|view, _, _, cx| view.on_start_mount(cx))),
                    )
                    .child(
                        Button::new("stop-mount")
                            .label("Stop")
                            .primary()
                            .disabled(!mounted)
                            .on_click(cx.listener(|view, _, _, cx| view.on_stop_mount(cx))),
                    ),
            )
            .when_some(self.state.mount_error.clone(), |this, error| {
                this.child(error_line(error, cx))
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
            .child(muted(
                "Read-only files download when opened. Closing RomMFS stops the mount.",
                cx,
            ))
    }

    fn render_save_sync(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let enabled = self.state.save_sync_enabled;
        let (status, status_kind) = self.save_sync_status();
        let has_candidates = !self.state.save_sync_candidates.is_empty();

        let saves_folder = self
            .state
            .save_sync_effective_saves_root
            .clone()
            .unwrap_or_else(|| "Not verified".into());
        let profile = self
            .state
            .save_sync_profile_version
            .as_deref()
            .map(|version| format!("RetroBat {version} · RetroArch / Gambatte · Game Boy"))
            .unwrap_or_else(|| "RetroBat profile not verified".into());
        let scope = match (
            &self.state.save_sync_server_id,
            self.state.save_sync_account_id,
        ) {
            (Some(server), Some(account)) => format!(
                "Server: {server} · Account: {} (ID {account})",
                self.user_input.read(cx).value()
            ),
            _ => "Server and account not verified".into(),
        };
        let (supported, skipped) = match self.state.save_sync_existing_saves.as_ref() {
            Some(preview)
                if matches!(
                    preview.status,
                    ExistingSaveScanStatus::Complete | ExistingSaveScanStatus::Partial
                ) =>
            {
                (
                    fmt_count(preview.supported_files),
                    fmt_count(preview.skipped_files),
                )
            }
            _ => ("—".into(), "—".into()),
        };

        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_3()
                            .child(section_heading("Save sync"))
                            .child(status_marker(status_kind, status, cx)),
                    )
                    .child(if enabled {
                        Button::new("save-sync-toggle")
                            .label("Disable save sync")
                            .outline()
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.state.save_sync_enabled = false;
                                view.controller
                                    .send(Command::SetSaveSyncEnabled { enabled: false });
                                cx.notify();
                            }))
                    } else {
                        Button::new("save-sync-toggle")
                            .label("Enable save sync")
                            .primary()
                            .disabled(!self.state.save_sync_available)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.controller
                                    .send(Command::SetSaveSyncEnabled { enabled: true });
                                cx.notify();
                            }))
                    }),
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(div().w_40().flex_shrink_0().child("RetroBat installation"))
                    .child(
                        div().flex_1().min_w_0().child(
                            Select::new(&self.install_select)
                                .w_full()
                                .accessibility_label("RetroBat installation")
                                .placeholder(if has_candidates {
                                    "Select a RetroBat installation…"
                                } else {
                                    "No RetroBat installations found"
                                })
                                .disabled(!has_candidates),
                        ),
                    )
                    .child(
                        Button::new("save-sync-browse")
                            .label("Browse…")
                            .outline()
                            .on_click(cx.listener(|view, _, _, cx| view.browse_save_sync(cx))),
                    )
                    .child(
                        Button::new("save-sync-refresh")
                            .label("Refresh")
                            .outline()
                            .on_click(cx.listener(|view, _, _, _cx| {
                                view.controller.send(Command::RefreshSaveSync)
                            })),
                    ),
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(div().w_40().flex_shrink_0().child("Saves folder"))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_9()
                            .px_3()
                            .flex()
                            .items_center()
                            .rounded(cx.theme().radius)
                            .border_1()
                            .border_color(cx.theme().input)
                            .bg(cx.theme().input_background())
                            .child(div().truncate().child(saves_folder)),
                    ),
            )
            .child(muted(profile, cx))
            .child(muted(scope, cx))
            .child(div().text_sm().child(format!(
                "{} mapped games · {} supported saves · {} skipped files",
                fmt_count(self.state.save_sync_mapped_targets),
                supported,
                skipped,
            )))
            .child(self.render_mapping_disclosure(window, cx))
            .child(muted(
                "Existing saves are never replaced. Incoming differences are kept for review.",
                cx,
            ))
            .child(muted_xs("Sync runs only while RomMFS is open.", cx))
    }

    fn render_mapping_disclosure(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let open = self.mapping_open;
        Collapsible::new()
            .open(open)
            .child(self.disclosure_row(
                "save-sync-mapping-disclosure",
                "Mapping preview and skipped files",
                open,
                window,
                cx,
            ))
            .content(self.render_mapping_detail(window, cx))
    }

    fn render_mapping_detail(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let preview = self.state.save_sync_preview.clone();
        let existing_saves = self.state.save_sync_existing_saves.clone();
        let queue = self.state.save_sync_queue.clone();
        let can_export = self.state.save_sync_effective_saves_root.is_some()
            && self.state.save_sync_account_id.is_some();
        let can_apply_debounce = self.state.save_sync_available || self.state.save_sync_enabled;

        let mut detail = v_flex()
            .gap_2()
            .pt_2()
            .child(muted_xs(
                format!(
                    "Read-only filename/metadata scan; save contents are not opened. Bounded to {MAX_EXISTING_SAVE_SCAN_ENTRIES} entries and {MAX_EXISTING_SAVE_SCAN_DEPTH} directory levels."
                ),
                cx,
            ))
            .child(muted_xs(
                "Preview only: gb/*.gb → gb/<visible ROM stem>.srm. RTC companions, .gbc, and other profiles are skipped. Existing remote differences require review.",
                cx,
            ))
            .child(muted_xs(existing_save_inventory_summary(existing_saves.as_ref()), cx));

        for path in preview {
            detail = detail.child(
                div()
                    .text_xs()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_color(cx.theme().muted_foreground)
                    .child(path),
            );
        }

        detail = detail.child(
            h_flex()
                .gap_3()
                .items_end()
                .child(div().w_32().child(field_column(
                    "Debounce (seconds)",
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

        // Live queue detail only exists once sync is enabled and has run.
        if let Some(queue) = queue {
            let last_action = self.state.save_sync_transfers.last().map(|transfer| {
                format!(
                    "ROM {} · {} · {}{}",
                    transfer.rom_id,
                    transfer.revision,
                    transfer.phase,
                    transfer
                        .detail
                        .as_ref()
                        .map(|detail| format!(" — {detail}"))
                        .unwrap_or_default()
                )
            });
            detail = detail
                .child(
                    div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Affected games"),
                )
                .child(
                    div()
                        .text_sm()
                        .child(format!(
                            "{} mapped · {} reconciled · {} pending uploads · {} incoming · {} games need attention",
                            queue.mapped_games,
                            queue.reconciled_games,
                            queue.pending_outbound,
                            queue.pending_incoming,
                            queue.attention_games,
                        )),
                );
            if let Some(failure) = queue
                .failure
                .clone()
                .or(self.state.save_sync_failure.clone())
            {
                detail = detail.child(error_line(format!("Last failure: {failure}"), cx));
            }
            if let Some(problem) = self.state.save_sync_problem.clone() {
                detail = detail.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().warning)
                        .child(format!("Paused: {problem}")),
                );
            }
            if let Some(action) = last_action {
                detail = detail.child(muted_xs(format!("Last action: {action}"), cx));
            }
            for game in &queue.games {
                let hashes = format!(
                    "local {} · remote ID {} · remote {}",
                    game.local_hash.as_deref().unwrap_or("unknown"),
                    game.remote_id.as_deref().unwrap_or("none"),
                    game.remote_hash.as_deref().unwrap_or("unknown"),
                );
                let mut row = v_flex().child(div().text_xs().child(format!(
                    "{} (ROM {}) · {hashes}",
                    game.rom_name, game.rom_id
                )));
                if let Some(issue) = &game.issue {
                    row = row.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().warning)
                            .child(format!("Attention: {issue}")),
                    );
                }
                detail = detail.child(row);
            }

            if !queue.incoming.is_empty() {
                detail = detail.child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Incoming saves · export only"),
                );
                for incoming in &queue.incoming {
                    detail = detail.child(self.render_incoming_row(incoming, can_export, cx));
                }
            }
        }

        detail
    }

    fn render_incoming_row(
        &mut self,
        incoming: &SaveSyncIncomingStatus,
        can_export: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let incoming = incoming.clone();
        let error = self
            .state
            .save_sync_export_errors
            .get(&incoming.incoming_id)
            .cloned();
        let feedback = self
            .state
            .save_sync_export_feedback
            .get(&incoming.incoming_id)
            .cloned();
        let mut row = v_flex()
            .gap_1()
            .child(div().text_xs().child(format!(
                "{} · RomM save {} · pending revision {} · {} · {} · {}",
                incoming.rom_name,
                incoming.remote_id,
                incoming.incoming_id,
                incoming.state,
                incoming.reason,
                incoming.content_hash,
            )))
            .child(
                Button::new(format!("export-{}", incoming.incoming_id))
                    .label("Export…")
                    .outline()
                    .small()
                    .disabled(!can_export)
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.prompt_export(incoming.clone(), cx);
                    })),
            );
        if let Some(error) = error {
            row = row.child(error_line(format!("Export failed: {error}"), cx));
        }
        if let Some(path) = feedback {
            row = row.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().success)
                    .child(format!("Exported to {path}; review remains pending.")),
            );
        }
        row
    }

    fn render_downloads(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let header = |text: &'static str| {
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(text)
        };
        v_flex()
            .gap_2()
            .child(section_heading("Downloads"))
            .child(
                h_flex()
                    .gap_3()
                    .child(div().flex_1().min_w_0().child(header("File")))
                    .child(div().w_64().child(header("Progress")))
                    .child(div().w_40().child(header("Status"))),
            )
            .child(
                div()
                    .id("downloads")
                    .v_flex()
                    .gap_2()
                    .max_h_40()
                    .children(if self.state.downloads.is_empty() {
                        vec![muted("No downloads yet", cx).into_any_element()]
                    } else {
                        self.state
                            .downloads
                            .iter()
                            .map(|download| {
                                self.render_download_row(download.clone(), cx)
                                    .into_any_element()
                            })
                            .collect::<Vec<_>>()
                    })
                    .overflow_y_scrollbar(),
            )
    }

    fn render_download_row(
        &self,
        download: crate::controller::DownloadView,
        cx: &App,
    ) -> impl IntoElement {
        let (status_text, status_kind) = match &download.finished {
            None => (
                match download.total {
                    Some(total) => {
                        format!("{} / {}", fmt_bytes(download.received), fmt_bytes(total))
                    }
                    None => format!("{} —", fmt_bytes(download.received)),
                },
                StatusKind::Muted,
            ),
            Some(Ok(())) => ("Complete".to_string(), StatusKind::Success),
            Some(Err(error)) => (format!("Failed: {error}"), StatusKind::Danger),
        };
        let percent = match download.total {
            Some(total) if total > 0 => {
                (100.0 * download.received as f64 / total as f64).min(100.0)
            }
            _ if matches!(download.finished, Some(Ok(()))) => 100.0,
            _ => 0.0,
        } as f32;
        h_flex()
            .gap_3()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(download.file_name.clone()),
            )
            .child(
                div().w_64().child(
                    h_flex()
                        .gap_2()
                        .child(
                            div().flex_1().child(
                                Progress::new(("download", download.rom_id))
                                    .value(percent)
                                    .xsmall(),
                            ),
                        )
                        .child(
                            div()
                                .w_10()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{percent:.0}%")),
                        ),
                ),
            )
            .child(
                div()
                    .w_40()
                    .text_sm()
                    .whitespace_nowrap()
                    .text_color(status_kind.color(cx))
                    .child(status_text),
            )
    }

    fn render_log(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .flex_1()
            .min_h_0()
            .gap_2()
            .child(
                h_flex()
                    .justify_between()
                    .child(section_heading("Activity log"))
                    .child(
                        Button::new("copy-log")
                            .label("Copy")
                            .outline()
                            .small()
                            .on_click(cx.listener(|view, _, _, cx| view.on_copy_log(cx))),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .bg(cx.theme().muted)
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded(cx.theme().radius)
                    .child(
                        div()
                            .id("activity-log")
                            .track_scroll(&self.log_scroll)
                            .size_full()
                            .overflow_y_scroll()
                            .p_2()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_xs()
                            .children(if self.state.log.lines().next().is_none() {
                                vec![div()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("No activity yet")
                                    .into_any_element()]
                            } else {
                                self.state
                                    .log
                                    .lines()
                                    .map(|line| render_log_line(line, cx).into_any_element())
                                    .collect()
                            }),
                    )
                    .vertical_scrollbar(&self.log_scroll),
            )
    }
}

fn level_str(level: Level) -> &'static str {
    match level {
        Level::Info => "INFO",
        Level::Warn => "WARN",
        Level::Error => "ERROR",
    }
}

fn level_kind(level: Level) -> StatusKind {
    match level {
        Level::Info => StatusKind::Muted,
        Level::Warn => StatusKind::Warning,
        Level::Error => StatusKind::Danger,
    }
}

fn fmt_time(unix_secs: u64) -> String {
    let secs = unix_secs % 86_400;
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

fn format_log_line(line: &rommfs_core::events::LogLine) -> String {
    if line.op.is_empty() {
        format!(
            "{} {:<5} {}",
            fmt_time(line.unix_secs),
            level_str(line.level),
            line.message
        )
    } else {
        format!(
            "{} {:<5} [{}] {}",
            fmt_time(line.unix_secs),
            level_str(line.level),
            line.op,
            line.message
        )
    }
}

fn render_log_line(line: &rommfs_core::events::LogLine, cx: &App) -> impl IntoElement {
    let time = div()
        .text_color(cx.theme().muted_foreground)
        .child(fmt_time(line.unix_secs));
    let level = div()
        .w_12()
        .text_color(level_kind(line.level).color(cx))
        .child(level_str(line.level));
    let body = if line.op.is_empty() {
        line.message.clone()
    } else {
        format!("[{}] {}", line.op, line.message)
    };
    h_flex()
        .gap_2()
        .child(time)
        .child(level)
        .child(div().text_color(cx.theme().foreground).child(body))
}

fn fmt_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB"];
    let mut v = n as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}

fn existing_save_inventory_summary(preview: Option<&ExistingSavePreview>) -> String {
    let Some(preview) = preview else {
        return "Existing-save inventory not available until the path, profile, and catalogue are verified.".into();
    };
    let diagnostics = if preview.diagnostics.is_empty() {
        String::new()
    } else {
        format!(" {}", preview.diagnostics.join(" "))
    };
    match preview.status {
        ExistingSaveScanStatus::Unavailable => {
            format!("Existing-file scan unavailable; counts are not reported.{diagnostics}")
        }
        ExistingSaveScanStatus::Complete | ExistingSaveScanStatus::Partial => {
            let completeness = if preview.status == ExistingSaveScanStatus::Partial {
                "partial; counts may be incomplete"
            } else {
                "complete"
            };
            let reasons = preview
                .skipped_reasons
                .iter()
                .map(|count| format!("{} {}", count.files, count.reason.label()))
                .collect::<Vec<_>>()
                .join(", ");
            let skipped = if reasons.is_empty() {
                String::new()
            } else {
                format!(" ({reasons})")
            };
            format!(
                "Existing files ({completeness}): {} supported mapped SRAM files; {} skipped{skipped}.{diagnostics}",
                preview.supported_files, preview.skipped_files
            )
        }
    }
}

impl Render for RommfsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_sm()
            .child(self.render_header(window, cx))
            .child(self.render_connection(window, cx))
            .child(Separator::horizontal())
            .child(self.render_mount(window, cx))
            .child(Separator::horizontal())
            .child(self.render_save_sync(window, cx))
            .child(Separator::horizontal())
            .child(self.render_downloads(window, cx))
            .child(Separator::horizontal())
            .child(self.render_log(window, cx))
    }
}

/// The app's dark palette — blue-gray tones per the mockup, applied over the
/// built-in dark theme so any token left unset keeps its library default.
fn apply_rommfs_theme(theme: &mut Theme) {
    fn color(hex: &str) -> Hsla {
        try_parse_color(hex).expect("static hex literal must parse")
    }
    theme.background = color("#1c1e2e");
    theme.foreground = color("#e2e5f3");
    theme.muted = color("#161826");
    theme.muted_foreground = color("#8b90b0");
    theme.border = color("#363a55");
    theme.input = color("#3c4165");
    theme.accent = color("#2a2e4a");
    theme.accent_foreground = color("#e2e5f3");
    theme.secondary = color("#2b3358");
    theme.secondary_foreground = color("#dfe2f4");
    theme.secondary_hover = color("#343d66");
    theme.popover = color("#252840");
    theme.popover_foreground = color("#e2e5f3");
    theme.primary = color("#3b82f6");
    theme.primary_foreground = color("#f5f8ff");
    theme.primary_hover = color("#4f8df7");
    theme.primary_active = color("#2f6de0");
    theme.button_primary = theme.primary;
    theme.button_primary_foreground = theme.primary_foreground;
    theme.button_primary_hover = theme.primary_hover;
    theme.button_primary_active = theme.primary_active;
    theme.ring = theme.primary;
    theme.selection = theme.primary.opacity(0.35);
    theme.success = color("#4ade80");
    theme.warning = color("#fbbf24");
    theme.danger = color("#f87171");
    theme.info = color("#60a5fa");
}

/// Launch the GPUI app; blocks until the window closes. Closing stops the
/// worker session and mount (PRD: no tray/background process).
pub fn run() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx: &mut App| {
            gpui_kit::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
            Theme::update(cx, apply_rommfs_theme);
            cx.bind_keys([
                KeyBinding::new("enter", ActivateControl, Some("Disclosure")),
                KeyBinding::new("space", ActivateControl, Some("Disclosure")),
            ]);

            let bounds = Bounds::centered(None, size(px(1160.0), px(920.0)), cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(gpui_kit::TitlebarOptions {
                        title: Some("RomMFS".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| RommfsWindow::new(window, cx)),
            )
            .expect("failed to open window");
            cx.activate(true);
        });
}
