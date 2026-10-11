//! The single GPUI window (PRD R5): connection form, mount controls,
//! save-sync setup, download progress, bounded scrollable log. Renders
//! `UiState` only — no timers, no fabricated progress, no business logic here.
//!
//! Built on `gpui-kit` (GPUI plus `gpui_kit::component`): text editing,
//! keyboard activation, and focus traversal come from the component
//! library; the view composes sections and forwards `Command`s.

use crate::controller::{
    Command, ConnState, Controller, FolderSummary, MountState, SaveSyncSummary, UiState,
};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    collapsible::Collapsible,
    h_flex,
    input::{Input, InputContentType, InputEvent, InputState},
    label::Label,
    progress::Progress,
    radio::RadioGroup,
    select::{Select, SelectEvent, SelectState},
    separator::Separator,
    sidebar::{Sidebar, SidebarCollapsible, SidebarMenu, SidebarMenuItem},
    switch::Switch,
    try_parse_color, v_flex, ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _,
    StyledExt as _, Theme, ThemeMode,
};
use gpui_kit::{
    actions, div, point, prelude::*, px, size, AnyElement, App, Bounds, ClipboardItem, Context,
    Entity, Focusable as _, FontWeight, Hsla, KeyBinding, PathPromptOptions, ScrollHandle,
    SharedString, Subscription, WeakEntity, Window, WindowBounds, WindowOptions,
};
use rommfs_core::connection_settings::IntegrationMode;
use rommfs_core::events::{AppEvent, ForeignKind, Level, SaveSyncIncomingStatus};
use rommfs_core::save_sync::{ExistingSavePreview, ExistingSaveScanStatus};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

actions!(
    rommfs,
    [
        /// Keyboard activation for the disclosure row (Enter/Space).
        ActivateControl,
    ]
);

mod pages;

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

/// "Technical details" disclosure for failure blocks: a quiet toggle plus
/// the complete diagnostic text in mono. The full path and cause chain
/// stay reachable even where the summary line is shortened (UI-PLAN §6).
fn details_disclosure(
    id: impl Into<SharedString>,
    detail: String,
    window: &mut Window,
    cx: &mut Context<RommfsWindow>,
) -> impl IntoElement {
    let id = id.into();
    let open = window.use_keyed_state(
        SharedString::from(format!("{id}-open")),
        cx,
        |_, _| std::cell::Cell::new(false),
    );
    let is_open = open.read(cx).get();
    let toggle = open.clone();
    v_flex()
        .gap_1()
        .child(
            Button::new(id)
                .ghost()
                .small()
                .label(if is_open {
                    "Hide technical details"
                } else {
                    "Technical details"
                })
                .on_click(cx.listener(move |_, _, _, cx| {
                    toggle.update(cx, |open, _| open.set(!open.get()));
                    cx.notify();
                })),
        )
        .when(is_open, |this| {
            this.child(
                div()
                    .w_full()
                    .p_2()
                    .bg(cx.theme().muted)
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded(cx.theme().radius)
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_xs()
                    .child(detail),
            )
        })
}

/// The shared failure presentation (UI-PLAN §6): a title, the real reason
/// as the explanation, the relevant action, and expandable details —
/// placed at the top of the section the failure belongs to.
fn failure_block(
    id: &'static str,
    title: &'static str,
    explanation: String,
    action: Option<Button>,
    detail: String,
    window: &mut Window,
    cx: &mut Context<RommfsWindow>,
) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(cx.theme().danger)
                .child(title),
        )
        .child(muted(explanation, cx))
        .when_some(action, |this, button| {
            this.child(div().pt_1().child(button))
        })
        .child(details_disclosure(
            SharedString::from(format!("{id}-details")),
            detail,
            window,
            cx,
        ))
}

/// "start filesystem backend for mount \"…\" failed: <cause>" — keep the
/// cause as the explanation and the whole chain for the details lane.
fn failure_explanation(error: &str) -> String {
    error
        .split_once(" failed: ")
        .map(|(_, cause)| cause.to_string())
        .unwrap_or_else(|| error.to_string())
}

fn section_heading(text: &'static str) -> impl IntoElement {
    div()
        .text_base()
        .font_weight(FontWeight::SEMIBOLD)
        .child(text)
}

/// ~24px page title (UI-PLAN §2 visual scale).
fn page_heading(text: &'static str) -> impl IntoElement {
    div()
        .text_2xl()
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

/// "refreshed 12 min ago" instead of a raw Unix timestamp.
fn relative_time(unix_secs: u64) -> String {
    let now = rommfs_core::cache::now_unix_secs();
    let ago = now.saturating_sub(unix_secs);
    if ago < 60 {
        "just now".into()
    } else if ago < 3600 {
        format!("{} min ago", ago / 60)
    } else if ago < 86_400 {
        format!("{} h {} min ago", ago / 3600, ago % 3600 / 60)
    } else {
        format!("{} days ago", ago / 86_400)
    }
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

#[derive(Default)]
struct UnmatchedSaveGroup {
    count: usize,
    extensions: Vec<String>,
    mappable: bool,
    reason: String,
}

struct RommfsWindow {
    controller: Rc<Controller>,
    session: Rc<RefCell<crate::session::ApplicationSession>>,
    state: UiState,
    url_input: Entity<InputState>,
    user_input: Entity<InputState>,
    password_input: Entity<InputState>,
    mount_input: Entity<InputState>,
    install_select: Entity<SelectState<Vec<SharedString>>>,
    save_folder_input: Entity<InputState>,
    save_file_input: Entity<InputState>,
    save_game_select: Entity<SelectState<Vec<SharedString>>>,
    debounce_input: Entity<InputState>,
    debounce_input_applied: String,
    debounce_error: Option<String>,
    active_view: usize,
    password_sign_in: bool,
    integration_choice: usize,
    /// Settings-tab installation picker; fed by the same discovery candidates
    /// as `install_select` but bound to the integration's RetroBat root.
    settings_install_select: Entity<SelectState<Vec<SharedString>>>,
    completed_open: bool,
    mapping_open: bool,
    /// Settings > Advanced disclosure (save-change delay).
    advanced_open: bool,
    /// Settings > optional RetroBat setup & support files disclosure.
    optional_setup_open: bool,
    /// Settings > "Use a different save folder" disclosure.
    custom_saves_open: bool,
    // --- Save review page state (window/pages/save_review.rs) ---
    /// Search box filtering the review table by name, file, or target path.
    #[allow(dead_code)] // read by pages/save_review.rs (UI-PLAN task 4)
    save_review_search: Entity<InputState>,
    /// Zero-based page of the review table.
    #[allow(dead_code)] // read by pages/save_review.rs
    save_review_page: usize,
    /// Row key expanded to its detail block (file id or title key).
    #[allow(dead_code)] // read by pages/save_review.rs
    save_review_expanded: Option<SharedString>,
    // --- Activity page state (window/pages/activity.rs) ---
    /// Minimum log severity shown: 0 = everything, 1 = warnings+errors,
    /// 2 = errors only.
    #[allow(dead_code)] // read by pages/activity.rs (UI-PLAN task 5)
    activity_min_level: usize,
    /// Follow the newest log entry; scrolling up unsets it.
    #[allow(dead_code)] // read by pages/activity.rs
    activity_follow_log: bool,
    log_scroll: ScrollHandle,
    /// True when a system tray exists and closing the window only hides it —
    /// close-vs-quit copy is derived from this, never assumed (UI-PLAN §3).
    tray_hides: bool,
    _subscriptions: Vec<Subscription>,
}

impl RommfsWindow {
    fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        session: Rc<RefCell<crate::session::ApplicationSession>>,
        tray_hides: bool,
    ) -> Self {
        let controller = session.borrow().controller();

        let url_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("http://romm.local:8080"));
        let user_input = cx.new(|cx| InputState::new(window, cx).placeholder("username"));
        let password_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("password")
                .masked(true)
        });
        // Drive roots reject directory creation for standard users on many
        // systems; the profile directory is always writable. Off Windows a
        // home-relative default keeps the first-start error honest instead
        // of offering C:\RomM on a Linux box.
        #[cfg(windows)]
        let default_mount = std::env::var_os("USERPROFILE")
            .map(|profile| std::path::PathBuf::from(profile).join("RomM"))
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "C:\\RomM".to_string());
        #[cfg(not(windows))]
        let default_mount = std::env::var_os("HOME")
            .map(|home| std::path::PathBuf::from(home).join("RomM"))
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "~/RomM".to_string());
        let mount_input = cx.new(|cx| InputState::new(window, cx).default_value(default_mount));
        let install_select =
            cx.new(|cx| SelectState::new(Vec::<SharedString>::new(), None, window, cx));
        let save_folder_input = cx
            .new(|cx| InputState::new(window, cx).placeholder("Folder containing only save data"));
        let save_file_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Relative path, for example switch/user/title/0")
        });
        let save_game_select =
            cx.new(|cx| SelectState::new(Vec::<SharedString>::new(), None, window, cx));
        let debounce_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(rommfs_core::save_sync::DEFAULT_DEBOUNCE_SECS.to_string())
        });

        let settings_install_select =
            cx.new(|cx| SelectState::new(Vec::<SharedString>::new(), None, window, cx));
        let save_review_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search saves"));
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

        // The save-review table re-filters live and restarts at page one
        // whenever the query changes.
        subscriptions.push(cx.subscribe_in(
            &save_review_search,
            window,
            |view, _state, event, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    view.save_review_page = 0;
                    cx.notify();
                }
            },
        ));

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

        // Picking a discovered installation in Settings applies the RetroBat
        // layout immediately; failures revert the radio via IntegrationUpdated.
        subscriptions.push(cx.subscribe_in(
            &settings_install_select,
            window,
            |view, _state, event, _window, cx| {
                let SelectEvent::Confirm(value) = event;
                let Some(value) = value else { return };
                let value = value.to_string();
                if view
                    .state
                    .retrobat_root
                    .as_deref()
                    .is_some_and(|root| root.eq_ignore_ascii_case(&value))
                {
                    return;
                }
                view.controller.send(Command::SetIntegration {
                    mode: IntegrationMode::RetroBat,
                    retrobat_root: Some(PathBuf::from(value)),
                });
                cx.notify();
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
            session,
            state: UiState::new(LOG_CAP),
            url_input,
            user_input,
            password_input,
            mount_input,
            install_select,
            save_folder_input,
            save_file_input,
            save_game_select,
            debounce_input,
            debounce_input_applied: rommfs_core::save_sync::DEFAULT_DEBOUNCE_SECS.to_string(),
            debounce_error: None,
            // Fresh installs start on the guided setup view (UI-PLAN §3);
            // the first connection event re-routes to Overview.
            active_view: 4,
            password_sign_in: false,
            integration_choice: 0,
            settings_install_select,
            completed_open: false,
            mapping_open: false,
            advanced_open: false,
            optional_setup_open: false,
            custom_saves_open: false,
            save_review_search,
            save_review_page: 0,
            save_review_expanded: None,
            activity_min_level: 0,
            activity_follow_log: true,
            log_scroll: ScrollHandle::new(),
            tray_hides,
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
        let events = self.session.borrow_mut().drain();
        for event in events {
            if matches!(event, AppEvent::Log(_)) {
                new_logs = true;
            }
            match &event {
                AppEvent::SaveFileMappings {
                    session_id, games, ..
                } if *session_id == self.state.save_sync_session_id => {
                    let items = games
                        .iter()
                        .map(|(id, name)| SharedString::from(format!("{id} · {name}")))
                        .collect::<Vec<_>>();
                    self.save_game_select
                        .update(cx, |state, cx| state.set_items(items, window, cx));
                }
                AppEvent::SaveSyncUpdated { .. } | AppEvent::SaveSyncSessionChanged { .. } => {
                    save_sync_updated = true;
                }
                _ => {}
            }
            match &event {
                AppEvent::IntegrationUpdated { mode, .. } => {
                    self.integration_choice = usize::from(*mode == IntegrationMode::RetroBat);
                    save_sync_updated = true;
                }
                AppEvent::PairingReady { url, .. } => cx.open_url(url),
                AppEvent::Connected => self.active_view = 0,
                // SignedOut with no remembered account is the unconfigured
                // state — guided setup, not the full settings page.
                AppEvent::SignedOut => self.active_view = 4,
                // A remembered account that needs re-auth IS configured —
                // send it to Overview where the sign-in action lives
                // (existing config must never replay onboarding).
                AppEvent::SignInRequired { .. } => self.active_view = 0,
                // A remembered account whose server is temporarily down is
                // still configured — Overview, never guided setup again
                // (UI-PLAN §4).
                AppEvent::ConnectFailed { .. } if self.state.connected_server.is_some() => {
                    self.active_view = 0;
                }
                AppEvent::ConnectionRestored { server, mount_root } => {
                    self.url_input
                        .update(cx, |input, cx| input.set_value(server.clone(), window, cx));
                    if let Some(root) = mount_root {
                        self.mount_input
                            .update(cx, |input, cx| input.set_value(root.clone(), window, cx));
                    }
                }
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
            self.sync_settings_install_select(window, cx);
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

    /// The Settings-tab picker lists the same discovered installations, plus
    /// the configured RetroBat root when discovery did not report it (a
    /// non-standard location). Its selection applies SetIntegration; a failed
    /// apply leaves `state.retrobat_root` untouched so the next sync reverts
    /// the selection back to the persisted root.
    fn sync_settings_install_select(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut items: Vec<SharedString> = self
            .state
            .save_sync_candidates
            .iter()
            .map(|candidate| {
                SharedString::from(candidate.info.install_root.to_string_lossy().into_owned())
            })
            .collect();
        if let Some(root) = &self.state.retrobat_root {
            if !items.iter().any(|item| item.eq_ignore_ascii_case(root)) {
                items.push(SharedString::from(root.clone()));
            }
        }
        let selected = self.state.retrobat_root.as_deref().and_then(|root| {
            items
                .iter()
                .find(|item| item.eq_ignore_ascii_case(root))
                .cloned()
        });
        self.settings_install_select.update(cx, |state, cx| {
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
        if !self.password_sign_in {
            self.controller.send(Command::BeginPairing {
                url: self.url_input.read(cx).value().to_string(),
            });
            return;
        }
        self.controller.send(Command::Connect {
            url: self.url_input.read(cx).value().to_string(),
            username: self.user_input.read(cx).value().to_string(),
            password: self.password_input.read(cx).value().to_string(),
        });
    }

    fn on_start_mount(&mut self, cx: &mut Context<Self>) {
        let path = self.mount_input.read(cx).value().to_string();
        self.start_mount(path, cx);
    }

    /// Start a mount at an explicit path — shared by the folder field's
    /// Start action and the per-state actions on Overview (UI-PLAN §3).
    fn start_mount(&mut self, path: String, cx: &mut Context<Self>) {
        if matches!(self.state.mount, MountState::Mounting | MountState::Mounted) {
            return;
        }
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

    /// The RetroBat installation the Settings tab currently points at: the
    /// picker's selection, falling back to the already-applied root.
    fn settings_install_root(&self, cx: &App) -> Option<PathBuf> {
        self.settings_install_select
            .read(cx)
            .selected_value()
            .map(|value| PathBuf::from(value.to_string()))
            .or_else(|| self.state.retrobat_root.as_deref().map(PathBuf::from))
    }

    /// Folder picker for a RetroBat installation discovery missed. Applies the
    /// RetroBat layout with that root immediately — the same thing a pick in
    /// the select does.
    fn browse_settings_install(&mut self, cx: &mut Context<Self>) {
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
                            view.controller.send(Command::SetIntegration {
                                mode: IntegrationMode::RetroBat,
                                retrobat_root: Some(path),
                            });
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

    /// §4 step 2 offers a folder picker beside the free-text path.
    fn browse_games_folder(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select the games folder".into()),
        });
        cx.spawn(
            async move |this: WeakEntity<RommfsWindow>, cx| match paths.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        let _ = this.update_in(cx, |view, window, cx| {
                            let text = path.display().to_string();
                            view.mount_input.update(cx, |input, cx| {
                                input.set_value(text, window, cx);
                            });
                            cx.notify();
                        });
                    }
                }
                Ok(Ok(None)) => {}
                Ok(Err(error)) => {
                    let _ = this.update(cx, |view, _| {
                        view.show_picker_error(format!("Games folder picker failed: {error}"))
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

    /// The header marker is explicitly the *RomM connection* status — a
    /// bare "Connected" reads as app-wide health (UI-PLAN §3).
    fn conn_status(&self, cx: &App) -> (SharedString, StatusKind) {
        match self.state.conn {
            ConnState::Idle => ("RomM · not connected".into(), StatusKind::Muted),
            ConnState::Connecting => ("RomM · connecting…".into(), StatusKind::Warning),
            ConnState::Connected => {
                let user = self.user_input.read(cx).value();
                if user.is_empty() {
                    ("RomM · connected".into(), StatusKind::Success)
                } else {
                    (
                        format!("RomM · connected · {user}").into(),
                        StatusKind::Success,
                    )
                }
            }
            ConnState::Failed => ("RomM · connection failed".into(), StatusKind::Danger),
            ConnState::SignInRequired => ("RomM · sign-in required".into(), StatusKind::Danger),
        }
    }

    /// The folder marker derives from `UiState::folder_summary` — real mount
    /// events plus persisted config, never the connection state.
    fn mount_status(&self, _cx: &App) -> (SharedString, StatusKind) {
        let summary = self.state.folder_summary();
        let kind = match summary {
            FolderSummary::SetupNeeded | FolderSummary::Stopped => StatusKind::Muted,
            FolderSummary::Starting => StatusKind::Warning,
            FolderSummary::Ready => StatusKind::Success,
            FolderSummary::Failed => StatusKind::Danger,
        };
        (summary.label().into(), kind)
    }

    /// The single derived save-sync status (UI-PLAN §5): zero linked files
    /// is "No files ready to sync", never "Up to date".
    fn save_sync_status(&self) -> (&'static str, StatusKind) {
        let summary = self.state.save_sync_summary();
        let kind = match summary {
            SaveSyncSummary::AuthRequired
            | SaveSyncSummary::Offline
            | SaveSyncSummary::NeedsAttention
            | SaveSyncSummary::Paused => StatusKind::Danger,
            SaveSyncSummary::NeedsSetup
            | SaveSyncSummary::Checking
            | SaveSyncSummary::NoLinkedFiles
            | SaveSyncSummary::Reconciling
            | SaveSyncSummary::Syncing => StatusKind::Warning,
            SaveSyncSummary::Off => StatusKind::Muted,
            SaveSyncSummary::UpToDate => StatusKind::Success,
        };
        (summary.label(), kind)
    }

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

    /// The ~190px left navigation: four destinations, and the server host
    /// plus a labelled RomM connection status pinned at the bottom
    /// (UI-PLAN §2). Nav clicks reach the view through its WeakEntity.
    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.weak_entity();
        let (conn_text, conn_kind) = self.conn_status(cx);
        let host = self
            .state
            .connected_server
            .clone()
            .filter(|server| !server.is_empty())
            .unwrap_or_else(|| self.url_input.read(cx).value().to_string());
        let active = self.active_view;
        let mut menu = SidebarMenu::new();
        for (index, label) in ["Overview", "Save sync", "Activity", "Settings"]
            .into_iter()
            .enumerate()
        {
            let weak = weak.clone();
            menu = menu.child(
                SidebarMenuItem::new(label)
                    .active(active == index)
                    .on_click(move |_, _, cx| {
                        if weak
                            .update(cx, |view, _cx| view.active_view = index)
                            .is_ok()
                        {
                            cx.notify(weak.entity_id());
                        }
                    }),
            );
        }
        Sidebar::new("app-nav")
            .w(px(190.))
            .collapsible(SidebarCollapsible::None)
            .header(
                div()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("RomMFS"),
            )
            .child(menu)
            .footer(
                v_flex()
                    .gap_1()
                    .child(muted_xs(
                        if host.is_empty() {
                            "No server configured".to_string()
                        } else {
                            host
                        },
                        cx,
                    ))
                    .child(status_marker(conn_kind, conn_text, cx)),
            )
    }

    /// A button that just switches destination — for "go fix this" actions on
    /// summary cards.
    fn nav_button(
        &mut self,
        id: &'static str,
        label: &'static str,
        view: usize,
        primary: bool,
        cx: &mut Context<Self>,
    ) -> Button {
        let button = Button::new(id).label(label);
        let button = if primary {
            button.primary()
        } else {
            button.outline()
        };
        button.on_click(cx.listener(move |this, _, _, cx| {
            this.active_view = view;
            cx.notify();
        }))
    }

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
                    .when_some(self.state.save_sync_problem.clone(), |this, problem| {
                        this.child(muted(problem, cx))
                    })
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
            ("Installing…", StatusKind::Warning)
        } else if self.state.retrobat_setup_error.is_some() {
            ("Setup failed", StatusKind::Danger)
        } else if self.state.retrobat_root.is_some() {
            ("Installed", StatusKind::Success)
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
            .when_some(
                self.state.retrobat_setup_detail.clone(),
                |this, detail| {
                    if self.state.retrobat_setup_running {
                        this.child(muted(detail, cx))
                    } else {
                        this
                    }
                },
            )
            .when_some(self.state.retrobat_setup_error.clone(), |this, error| {
                this.child(error_line(error, cx))
            })
            .child(self.nav_button(
                "overview-retrobat",
                "RetroBat settings",
                3,
                false,
                cx,
            ))
    }

    fn render_activity(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_4()
            .flex_1()
            .min_h_0()
            .child(page_heading("Activity"))
            .child(self.render_downloads(window, cx))
            .child(self.render_log(window, cx))
    }

    /// Settings in the plan's order (UI-PLAN §Settings organization):
    /// everyday settings first — connection, folders, background — then
    /// disclosures for optional setup and advanced tuning.
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
            .child(
                v_flex()
                    .gap_3()
                    .child(section_heading("Background operation"))
                    .child(self.render_background_panel(cx)),
            )
            .child(Separator::horizontal())
            .child(
                Collapsible::new()
                    .open(self.optional_setup_open)
                    .child(self.disclosure_row(
                        "optional-setup-disclosure",
                        "RetroBat setup & support files",
                        self.optional_setup_open,
                        |view| view.optional_setup_open = !view.optional_setup_open,
                        window,
                        cx,
                    ))
                    .content(
                        v_flex()
                            .gap_3()
                            .pt_2()
                            .child(section_heading("RetroBat"))
                            .child(self.retrobat_setup_panel(locked, cx))
                            .child(Separator::horizontal())
                            .child(section_heading("Support files"))
                            .child(self.support_files_panel(locked, cx)),
                    ),
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
            .child(self.layout_radio(locked, cx))
            .when(self.integration_choice == 1, |this| {
                this.child(self.retrobat_install_row(cx))
            })
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
                                    .disabled(self.state.conn != ConnState::Connected)
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

    /// First-run setup for an unconfigured install (UI-PLAN §3): connect to
    /// RomM, then choose the games folder. Optional integrations stay in
    /// Settings — they are never prerequisites here.
    fn render_setup(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locked = self.state.retrobat_setup_running
            || self.state.auxiliary_running
            || matches!(self.state.mount, MountState::Mounted | MountState::Mounting);
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
                            .child(
                                div().flex_1().min_w_0().child(field_column(
                                    "Games folder",
                                    Input::new(&self.mount_input).w_full(),
                                    cx,
                                )),
                            )
                            .child(
                                Button::new("setup-browse-folder")
                                    .label("Browse…")
                                    .outline()
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.browse_games_folder(cx)
                                    })),
                            ),
                    )
                    .child(self.layout_radio(locked, cx))
                    .when(self.integration_choice == 1, |this| {
                        this.child(self.retrobat_install_row(cx))
                    })
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
                        this.child(error_line(error, cx))
                    })
                    .when_some(self.render_foreign_remediation(cx), |this, block| {
                        this.child(block)
                    })
                    .child(self.nav_button(
                        "setup-advanced",
                        "More options in Settings",
                        3,
                        false,
                        cx,
                    )),
            )
    }

    /// Only the controls meaningful for the *current* auth state render —
    /// a signed-in session never sits beside its own sign-in buttons
    /// (UI-PLAN screenshot finding), and failures keep their real reason.
    /// The account block: real server/account when signed in, sign-in
    /// controls only when the state calls for them. The identity never
    /// falls back to a raw database id (UI-PLAN §Settings > Connection).
    fn render_connection(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let conn = self.state.conn;
        let connecting = conn == ConnState::Connecting;
        let connected = conn == ConnState::Connected;
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
    fn layout_radio(&mut self, locked: bool, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_2()
            .child(section_heading("Library layout"))
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
    }

    /// RetroBat installation picker — shared by Settings > Folders and the
    /// first-run folder step.
    fn retrobat_install_row(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(div().w_40().flex_shrink_0().child("RetroBat installation"))
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
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.browse_settings_install(cx)
                            })),
                    ),
            )
            .child(muted(
                "Requires a fresh installation configured through RomMFS setup. Its ROM directory must be empty or already managed by RomMFS.",
                cx,
            ))
    }

    /// The focused RetroBat setup panel (UI-PLAN §RetroBat setup): the
    /// installer download, the link action and its result, and the
    /// foreign-item remediation — optional, never a prerequisite.
    fn retrobat_setup_panel(&mut self, locked: bool, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("download-retrobat")
                            .label("Download official installer")
                            .disabled(self.state.installer_running)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.controller.send(Command::DownloadRetroBatInstaller);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("launch-retrobat-installer")
                            .label("Launch installer…")
                            .disabled(!self.state.installer_ready || self.state.installer_running)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.controller.send(Command::LaunchRetroBatInstaller);
                                cx.notify();
                            })),
                    )
                    .when(self.state.installer_running, |this| {
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
            .when_some(self.state.installer_detail.clone(), |this, detail| {
                this.child(muted(detail, cx))
            })
            .when_some(self.state.installer_error.clone(), |this, error| {
                this.child(error_line(error, cx))
            })
            .when_some(self.state.installer_progress.1, |this, total| {
                this.child(Progress::new("installer-progress").value(if total == 0 {
                    0.
                } else {
                    100. * self.state.installer_progress.0 as f32 / total as f32
                }))
            })
            .child(muted(
                "Setup links RetroBat's ROM folder to the games folder and configures Switch saves. Stop RetroBat and its emulators first. Unowned ROM files are never moved or removed.",
                cx,
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("setup-retrobat")
                            .label("Set up RetroBat")
                            .disabled(
                                locked
                                    || self.state.conn != ConnState::Connected
                                    || self.settings_install_root(cx).is_none(),
                            )
                            .on_click(cx.listener(|view, _, _, cx| {
                                if let Some(installation) = view.settings_install_root(cx) {
                                    view.controller.send(Command::SetupRetroBat {
                                        installation,
                                        managed_root: PathBuf::from(
                                            view.mount_input.read(cx).value().trim(),
                                        ),
                                        remove_known: false,
                                    });
                                }
                                cx.notify();
                            })),
                    )
                    .when(self.state.retrobat_setup_removable > 0, |this| {
                        this.child(
                            Button::new("setup-retrobat-remove-known")
                                .label(format!(
                                    "Remove {} known files/links and retry",
                                    self.state.retrobat_setup_removable
                                ))
                                .outline()
                                .disabled(
                                    locked
                                        || self.state.conn != ConnState::Connected
                                        || self.settings_install_root(cx).is_none(),
                                )
                                .on_click(cx.listener(|view, _, _, cx| {
                                    if let Some(installation) = view.settings_install_root(cx) {
                                        view.controller.send(Command::SetupRetroBat {
                                            installation,
                                            managed_root: PathBuf::from(
                                                view.mount_input.read(cx).value().trim(),
                                            ),
                                            remove_known: true,
                                        });
                                    }
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .when(locked, |this| {
                this.child(muted(
                    "Stop the mount before changing its layout.",
                    cx,
                ))
            })
            .when(
                !locked
                    && (self.state.conn != ConnState::Connected
                        || self.settings_install_root(cx).is_none()),
                |this| {
                    // §6: the disabled setup action explains why.
                    this.child(muted_xs(
                        if self.state.conn != ConnState::Connected {
                            "Connect to RomM first."
                        } else {
                            "Choose a RetroBat installation above."
                        },
                        cx,
                    ))
                },
            )
            .when_some(self.state.integration_notice.clone(), |this, note| {
                this.child(muted(note, cx))
            })
            .when_some(self.state.integration_error.clone(), |this, error| {
                this.child(error_line(error, cx))
            })
            .when_some(self.render_foreign_remediation(cx), |this, block| {
                this.child(block)
            })
            .when_some(self.state.retrobat_setup_detail.clone(), |this, detail| {
                this.child(muted(detail, cx))
            })
            .when_some(self.state.retrobat_setup_error.clone(), |this, error| {
                this.child(error_line(error, cx))
            })
    }

    /// Optional support-file downloads (BIOS/firmware/keys/cheats) — the
    /// auxiliary pipeline with its own progress and errors, never save data.
    fn support_files_panel(&mut self, locked: bool, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("materialize-auxiliary")
                            .label("Install library BIOS, keys and cheats")
                            .disabled(locked || self.state.conn != ConnState::Connected)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.controller.send(Command::MaterializeAuxiliary);
                                cx.notify();
                            })),
                    )
                    .when(self.state.auxiliary_running, |this| {
                        // §1: Cancel appears only while the operation runs.
                        this.child(
                            Button::new("cancel-auxiliary")
                                .label("Cancel")
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.controller.send(Command::CancelAuxiliary);
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .when(
                locked || self.state.conn != ConnState::Connected,
                |this| {
                    // §6: a disabled action carries its reason next to it.
                    this.child(muted_xs(
                        if locked {
                            "Unavailable while the games folder or setup is running — stop it first."
                        } else {
                            "Connect to RomM first."
                        },
                        cx,
                    ))
                },
            )
            .child(muted(
                "Downloads user-library BIOS, individual Switch firmware/keys, and RetroArch .cht cheats. No archives are extracted, no unowned or modified file is overwritten, and these files are never save data. Stop emulators before continuing.",
                cx,
            ))
            .when_some(self.state.auxiliary_detail.clone(), |this, detail| {
                this.child(muted(detail, cx))
            })
            .when_some(self.state.auxiliary_error.clone(), |this, error| {
                this.child(error_line(error, cx))
            })
            .when_some(
                self.state.frontend_metadata_detail.clone(),
                |this, detail| this.child(muted(detail, cx)),
            )
            .when_some(
                self.state.frontend_metadata_error.clone(),
                |this, error| this.child(error_line(error, cx)),
            )
    }

    /// Start-at-login plus the honest close/quit description.
    fn render_background_panel(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let login_supported = cfg!(windows);
        let needs_connect =
            !self.state.start_at_login && self.state.conn != ConnState::Connected;
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
                    "{} Login startup reconnects and mounts before resuming previously authorized save sync.",
                    close_behavior_copy(self.tray_hides)
                ),
                cx,
            ))
            .when_some(self.state.startup_error.clone(), |this, error| {
                this.child(error_line(error, cx))
            })
    }

    /// Foreign entries reported under a refused mount/setup root: the classified
    /// list plus the safe remediation (delete only symlinks/junctions — never
    /// real user files) and an Escape hatch to open the folder for review.
    fn render_foreign_remediation(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
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
    fn render_mount(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let summary = self.state.folder_summary();
        let (mount_text, mount_kind) = self.mount_status(cx);
        let configured = self
            .state
            .mount_path
            .clone()
            .or_else(|| self.state.configured_mount_root.clone());
        let ready = summary == FolderSummary::Ready;
        let connected = self.state.conn == ConnState::Connected;

        // The primary action this state offers (UI-PLAN §3 truth table).
        let primary: Option<Button> = match summary {
            FolderSummary::SetupNeeded => Some(self.nav_button(
                "overview-setup-folder",
                "Set up games folder",
                4,
                true,
                cx,
            )),
            FolderSummary::Stopped => configured.clone().map(|path| {
                Button::new("start-mount")
                    .label("Start games folder")
                    .primary()
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.start_mount(path.clone(), cx)
                    }))
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
                    "A complete refresh is ready; stop and start the mount to publish it.",
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
            .map(|version| format!("Save folder checked · {version}"))
            .unwrap_or_else(|| "Choose a save folder".into());
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
            // Status card: where sync points, who it syncs for, and how much
            // of the folder is understood — before any configuration detail.
            .child(
                div()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().border)
                    .p_3()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(if enabled { "Enabled" } else { "Not enabled" }),
                            )
                            .child(muted_xs(format!("Folder: {saves_folder}"), cx))
                            .child(muted_xs(scope, cx))
                            .child(muted_xs(
                                format!(
                                    "{} mapped files · {} unmatched files",
                                    fmt_count(self.state.save_sync_mapped_targets),
                                    fmt_count(self.state.save_sync_catalogue_unmapped),
                                ),
                                cx,
                            ))
                            .child(muted_xs(profile, cx)),
                    ),
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
            .child(field_column("Custom save-data folder",Input::new(&self.save_folder_input).w_full(),cx))
            .child(Button::new("choose-save-folder").label("Use save folder")
                .disabled(self.state.conn != ConnState::Connected)
                .on_click(cx.listener(|view,_,_,cx| {
                    view.controller.send(Command::SelectSaveFolder { path: PathBuf::from(view.save_folder_input.read(cx).value().trim()) }); cx.notify();
                })))
            .child(muted("Select saves only, not an emulator's entire portable folder. All file formats and companions are supported. Shared saves and memory cards remain excluded.",cx))
            .child(self.render_mapping_disclosure(window, cx))
            .child(self.render_advanced_disclosure(window, cx))
    }

    fn render_advanced_disclosure(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let open = self.advanced_open;
        Collapsible::new()
            .open(open)
            .child(self.disclosure_row(
                "save-sync-advanced-disclosure",
                "Advanced / How sync works",
                open,
                |view| view.advanced_open = !view.advanced_open,
                window,
                cx,
            ))
            .content(self.render_advanced_detail(window, cx))
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
            .child(muted_xs("Sync continues while RomMFS runs, including in the tray.", cx))
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
                |view| view.mapping_open = !view.mapping_open,
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
        let has_games = !self.state.save_file_games.is_empty();

        let mut detail = v_flex()
            .gap_2()
            .pt_2()
            .child(muted_xs(
                "Files are associated by a unique game filename, a server-provided save target, or an explicit file mapping. Ambiguous files are not guessed.",
                cx,
            ))
            .child(muted_xs(existing_save_inventory_summary(existing_saves.as_ref()), cx));

        if has_games {
            detail = detail
                .child(field_column(
                    "Game",
                    Select::new(&self.save_game_select)
                        .w_full()
                        .accessibility_label("Save file game")
                        .placeholder("Select the owning game…"),
                    cx,
                ))
                .child(muted_xs(
                    "Unmatched files grouped by folder. Pick a game above, then map a whole group or a single path.",
                    cx,
                ));
        }
        // Group by containing directory: "naomi/reicast — 24 .nvmem files"
        // instead of 24 identical rows. The groups render even before the
        // game list arrives; the map action waits for it.
        let mut groups: std::collections::BTreeMap<String, UnmatchedSaveGroup> = Default::default();
        for file in &self.state.save_file_unmatched {
            let dir = file
                .path
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            let entry = groups.entry(dir).or_default();
            entry.count += 1;
            let ext = file
                .path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| format!(".{}", e.to_lowercase()))
                .unwrap_or_else(|| "file".into());
            if !entry.extensions.contains(&ext) {
                entry.extensions.push(ext);
            }
            entry.mappable |= file.mappable;
            if entry.reason.is_empty() {
                entry.reason = file.reason.clone();
            }
        }
        for (dir, group) in groups.iter().take(32) {
            let dir_display = if dir.is_empty() {
                "(save folder root)"
            } else {
                dir.as_str()
            };
            let mut row = v_flex().gap_1().child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(div().truncate().text_sm().child(format!(
                                "{dir_display} — {} {} {}",
                                group.count,
                                group.extensions.join("/"),
                                if group.count == 1 { "file" } else { "files" },
                            ))),
                    )
                    .when(group.mappable && has_games, |this| {
                        let prefix = dir.clone();
                        this.child(
                            Button::new(format!("map-group-{dir}"))
                                .label("Map group to game…")
                                .outline()
                                .small()
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    let id = view
                                        .save_game_select
                                        .read(cx)
                                        .selected_value()
                                        .and_then(|s| s.split_once(" · "))
                                        .and_then(|(id, _)| id.parse::<i64>().ok());
                                    if let Some(rom_id) = id {
                                        view.controller.send(Command::AssociateSaveFolder {
                                            prefix: PathBuf::from(prefix.clone()),
                                            rom_id,
                                        });
                                    }
                                    cx.notify();
                                })),
                        )
                    }),
            );
            row = row.child(muted_xs(group.reason.clone(), cx));
            detail = detail.child(row);
        }
        if groups.len() > 32 {
            detail = detail.child(muted_xs(format!("…{} more folders", groups.len() - 32), cx));
        }
        if has_games {
            detail = detail
                .child(section_heading("Map a single file"))
                .child(field_column(
                    "Relative save path",
                    Input::new(&self.save_file_input).w_full(),
                    cx,
                ))
                .child(
                    Button::new("associate-save-file")
                        .label("Map file")
                        .on_click(cx.listener(|view, _, _, cx| {
                            let id = view
                                .save_game_select
                                .read(cx)
                                .selected_value()
                                .and_then(|s| s.split_once(" · "))
                                .and_then(|(id, _)| id.parse::<i64>().ok());
                            if let Some(rom_id) = id {
                                view.controller.send(Command::AssociateSaveFile {
                                    relative_path: PathBuf::from(
                                        view.save_file_input.read(cx).value().trim(),
                                    ),
                                    rom_id,
                                });
                            }
                            cx.notify();
                        })),
                );
        }

        for path in preview {
            detail = detail.child(
                div()
                    .text_xs()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_color(cx.theme().muted_foreground)
                    .child(path),
            );
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
                    div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Affected files"),
                )
                .child(
                    div()
                        .text_sm()
                        .child(format!(
                            "{} mapped · {} reconciled · {} pending uploads · {} incoming · {} files need attention",
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
        let active: Vec<_> = self
            .state
            .downloads
            .iter()
            .filter(|d| d.finished.is_none())
            .cloned()
            .collect();
        let history: Vec<_> = self
            .state
            .downloads
            .iter()
            .filter(|d| d.finished.is_some())
            .cloned()
            .collect();
        v_flex()
            .gap_2()
            .child(section_heading("Active transfers"))
            .child(
                div()
                    .id("active-downloads")
                    .v_flex()
                    .gap_2()
                    .max_h_40()
                    .when(active.is_empty(), |this| {
                        this.child(muted("No active transfers", cx))
                    })
                    .children(
                        active
                            .into_iter()
                            .map(|d| self.render_download_row(d, cx).into_any_element()),
                    )
                    .overflow_y_scrollbar(),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("toggle-completed")
                            .label(if self.completed_open {
                                "Collapse history"
                            } else {
                                "Show history"
                            })
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.completed_open = !view.completed_open;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("clear-completed")
                            .label("Clear completed")
                            .disabled(!history.iter().any(|d| matches!(d.finished, Some(Ok(())))))
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.state.clear_completed_downloads();
                                cx.notify();
                            })),
                    ),
            )
            .when(self.completed_open, |this| {
                this.child(
                    div()
                        .id("completed-downloads")
                        .v_flex()
                        .gap_2()
                        .max_h_40()
                        .when(history.is_empty(), |this| {
                            this.child(muted("No completed or failed transfers", cx))
                        })
                        .children(
                            history
                                .into_iter()
                                .map(|d| self.render_download_row(d, cx).into_any_element()),
                        )
                        .overflow_y_scrollbar(),
                )
            })
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
                                Progress::new(("download", download.file_id))
                                    .value(percent)
                                    .loading(
                                        download.finished.is_none() && download.total.is_none(),
                                    )
                                    .xsmall(),
                            ),
                        )
                        .child(
                            div()
                                .w_10()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                    if download.finished.is_none() && download.total.is_none() {
                                        "—".into()
                                    } else {
                                        format!("{percent:.0}%")
                                    },
                                ),
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
                        Button::new("open-log-folder")
                            .label("Open log folder")
                            .outline()
                            .small()
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
                    )
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
        return "Refresh to discover new or unmatched save files.".into();
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
                "Existing files ({completeness}): {} recognized files; {} skipped{skipped}.{diagnostics}",
                preview.supported_files, preview.skipped_files
            )
        }
    }
}

impl Render for RommfsWindow {
    /// The four-destination shell (UI-PLAN §2): persistent left nav, one
    /// changing detail region, content kept to a readable ~960px measure.
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = |id: &'static str, body: AnyElement| {
            div()
                .id(id)
                .flex_1()
                .min_w_0()
                .min_h_0()
                .overflow_y_scrollbar()
                .child(v_flex().p_6().w_full().max_w(px(960.)).child(body))
        };
        let content = match self.active_view {
            0 => page(
                "overview-view",
                pages::overview::render(self, window, cx).into_any_element(),
            ),
            1 => page(
                "save-sync-view",
                pages::save_review::render(self, window, cx).into_any_element(),
            ),
            2 => page(
                "activity-view",
                pages::activity::render(self, window, cx).into_any_element(),
            ),
            4 => page(
                "setup-view",
                self.render_setup(window, cx).into_any_element(),
            ),
            _ => page(
                "settings-view",
                pages::settings::render(self, window, cx).into_any_element(),
            ),
        };
        h_flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_sm()
            .child(self.render_sidebar(cx))
            .child(content)
    }
}

fn operation_progress(
    id: &'static str,
    stage: &str,
    completed: u64,
    total: Option<u64>,
    elapsed_ms: u64,
    cx: &App,
) -> impl IntoElement {
    let count = total.map_or_else(
        || completed.to_string(),
        |total| format!("{completed} / {total}"),
    );
    let percent = total
        .filter(|t| *t > 0)
        .map_or(0.0, |t| (100.0 * completed as f32 / t as f32).min(100.0));
    v_flex()
        .gap_1()
        .child(muted(
            format!("{stage} · {count} · {:.1}s", elapsed_ms as f64 / 1000.0),
            cx,
        ))
        .child(
            Progress::new(id)
                .value(percent)
                .loading(total.is_none())
                .small(),
        )
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

/// One truthful sentence about what closing the window does — repeated
/// wherever mount/background copy mentions it, so close-vs-quit can never
/// contradict the real tray behaviour (UI-PLAN §3).
fn close_behavior_copy(tray_hides: bool) -> &'static str {
    if tray_hides {
        "Closing the window hides RomMFS to the tray; the games folder and sync keep running. Quit from the tray menu to stop them."
    } else {
        "Closing RomMFS stops the games folder and all background work."
    }
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
            let instance = match crate::desktop::Instance::acquire() {
                Ok(Some(instance)) => instance,
                Ok(None) => {
                    cx.quit();
                    return;
                }
                Err(error) => {
                    eprintln!("RomMFS could not establish single-instance ownership: {error}");
                    cx.quit();
                    return;
                }
            };
            let session = Rc::new(RefCell::new(crate::session::ApplicationSession::new()));
            let quit_session = session.clone();
            let instance = Rc::new(instance);
            let quit_instance = instance.clone();
            cx.on_app_quit(move |_| {
                let _instance = &quit_instance;
                quit_session.borrow().shutdown();
                std::future::ready(())
            })
            .detach();
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            let startup = std::env::args().any(|a| a == "--startup");
            #[cfg(windows)]
            let tray = match crate::desktop::tray::Tray::new() {
                Ok(tray) => Some(tray),
                Err(error) => {
                    session.borrow().controller().sink().emit(AppEvent::log(
                        Level::Error,
                        "tray",
                        format!("Tray unavailable; the window will stay accessible: {error}"),
                    ));
                    None
                }
            };
            #[cfg(windows)]
            let tray_hides = tray.is_some();
            #[cfg(not(windows))]
            let tray_hides = false;

            let bounds = Bounds::centered(None, size(px(1160.0), px(920.0)), cx);
            let view_session = session.clone();
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(720.0), px(480.0))),
                    titlebar: Some(gpui_kit::TitlebarOptions {
                        title: Some("RomMFS".into()),
                        ..Default::default()
                    }),
                    show: {
                        #[cfg(windows)]
                        {
                            !startup || tray.is_none()
                        }
                        #[cfg(not(windows))]
                        {
                            let _ = startup;
                            true
                        }
                    },
                    ..Default::default()
                },
                cx,
                move |window, cx| {
                    #[cfg(windows)]
                    {
                        let hwnd = match raw_window_handle::HasWindowHandle::window_handle(window)
                            .map(|h| h.as_raw())
                        {
                            Ok(raw_window_handle::RawWindowHandle::Win32(handle)) => {
                                handle.hwnd.get() as usize
                            }
                            _ => panic!("Windows window has no HWND"),
                        };
                        crate::desktop::tray::set_window_icon(hwnd);
                        if tray.is_some() {
                            window.on_window_should_close(cx, move |_, _| {
                                crate::desktop::tray::hide(hwnd);
                                false
                            });
                        }
                        let tray_session = view_session.clone();
                        cx.spawn(async move |cx| {
                            let _instance = instance;
                            let mut tray = tray;
                            loop {
                                cx.background_executor().timer(POLL_INTERVAL).await;
                                if let Some(tray) = &mut tray {
                                    let status = tray_session.borrow().status();
                                    tray.update_status(status);
                                    for action in tray.actions() {
                                        match action {
                                            crate::desktop::tray::Action::Open => {
                                                crate::desktop::tray::show(hwnd)
                                            }
                                            crate::desktop::tray::Action::Quit => {
                                                cx.update(|cx| cx.quit());
                                                return;
                                            }
                                        }
                                    }
                                }
                            }
                        })
                        .detach();
                    }
                    #[cfg(not(windows))]
                    let _instance = instance;
                    cx.new(|cx| RommfsWindow::new(window, cx, view_session, tray_hides))
                },
            )
            .expect("failed to open window");
            if !startup {
                cx.activate(true);
            }
        });
}
