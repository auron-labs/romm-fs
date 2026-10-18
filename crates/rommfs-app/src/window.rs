//! The single GPUI window (PRD R5): connection form, mount controls,
//! save-sync setup, download progress, bounded scrollable log. Renders
//! `UiState` only — no timers, no fabricated progress, no business logic here.
//!
//! Built on `gpui-kit` (GPUI plus `gpui_kit::component`): text editing,
//! keyboard activation, and focus traversal come from the component
//! library; the view composes sections and forwards `Command`s.

use crate::controller::{
    Command, ConnectionState, Controller, FolderSummary, MountState, SaveSyncSummary, UiState,
};
use crate::problem::{auxiliary_headline, describe_auxiliary, Severity, UserProblem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    collapsible::Collapsible,
    h_flex,
    input::{Input, InputContentType, InputEvent, InputState},
    label::Label,
    progress::Progress,
    radio::{Radio, RadioGroup},
    select::{SearchableVec, Select, SelectEvent, SelectState},
    separator::Separator,
    sidebar::{Sidebar, SidebarCollapsible, SidebarMenu, SidebarMenuItem},
    switch::Switch,
    tab::{Tab, TabBar},
    tag::Tag,
    try_parse_color, v_flex, ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _,
    StyledExt as _, Theme, ThemeMode,
};
use gpui_kit::{
    actions, div, prelude::*, px, size, AnyElement, App, Bounds, ClipboardItem, Context, Entity,
    Focusable as _, FontWeight, Hsla, KeyBinding, PathPromptOptions, ScrollHandle, SharedString,
    Subscription, WeakEntity, Window, WindowBounds, WindowOptions,
};
use rommfs_core::connection_settings::IntegrationMode;
use rommfs_core::events::{
    AppEvent, ForeignKind, Level, MemoryCardConflictStatus, SaveSyncIncomingStatus,
};
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

mod actions;
mod folder_controls;
mod pages;

const LOG_CAP: usize = 500;
const POLL_INTERVAL: Duration = Duration::from_millis(200);

mod components;
use components::*;

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

struct RommfsWindow {
    controller: Rc<Controller>,
    session: Rc<RefCell<crate::session::ApplicationSession>>,
    state: UiState,
    url_input: Entity<InputState>,
    user_input: Entity<InputState>,
    password_input: Entity<InputState>,
    mount_input: Entity<InputState>,
    install_select: Entity<SelectState<SearchableVec<SharedString>>>,
    save_folder_input: Entity<InputState>,
    save_file_input: Entity<InputState>,
    save_game_select: Entity<SelectState<SearchableVec<SharedString>>>,
    debounce_input: Entity<InputState>,
    debounce_input_applied: String,
    debounce_error: Option<String>,
    active_view: usize,
    /// Selected Settings tab (Account, Games folder, RetroBat, Saves, App).
    settings_tab: usize,
    password_sign_in: bool,
    integration_choice: usize,
    /// Settings-tab installation picker; fed by the same discovery candidates
    /// as `install_select` but bound to the integration's RetroBat root.
    settings_install_select: Entity<SelectState<SearchableVec<SharedString>>>,
    completed_open: bool,
    /// One scroll handle per destination so each page keeps its own offset.
    page_scrolls: Vec<ScrollHandle>,
    /// One scroll handle per Settings tab so switching tabs keeps their offsets.
    settings_tab_scrolls: Vec<ScrollHandle>,
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
    /// Whether files that match no game in the library are listed.
    #[allow(dead_code)] // read by pages/save_review.rs
    save_review_show_ignored: bool,
    save_review_attention_only: bool,
    save_review_skipped_page: usize,
    // --- Activity page state (window/pages/activity.rs) ---
    /// Minimum log severity shown: 0 = everything, 1 = warnings+errors,
    /// 2 = errors only.
    #[allow(dead_code)] // read by pages/activity.rs (UI-PLAN task 5)
    activity_min_level: usize,
    /// Follow the newest log entry; scrolling up unsets it.
    #[allow(dead_code)] // read by pages/activity.rs
    activity_follow_log: bool,
    activity_log_open: bool,
    /// Substring filter for the technical log (UI-PLAN §6 search).
    #[allow(dead_code)] // read by pages/activity.rs
    activity_search: Entity<InputState>,
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
        let page_scrolls: Vec<ScrollHandle> = (0..5).map(|_| ScrollHandle::new()).collect();

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
        let install_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::from(Vec::<SharedString>::new()),
                None,
                window,
                cx,
            )
            .searchable(true)
        });
        let save_folder_input = cx
            .new(|cx| InputState::new(window, cx).placeholder("Folder containing only save data"));
        let save_file_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Relative path, for example switch/user/title/0")
        });
        let save_game_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::from(Vec::<SharedString>::new()),
                None,
                window,
                cx,
            )
            .searchable(true)
        });
        let debounce_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(rommfs_core::save_sync::DEFAULT_DEBOUNCE_SECS.to_string())
        });

        let settings_install_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::from(Vec::<SharedString>::new()),
                None,
                window,
                cx,
            )
            .searchable(true)
        });
        let save_review_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search saves"));
        let activity_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search log"));
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

        // Typing in the log search re-filters live.
        subscriptions.push(cx.subscribe_in(
            &activity_search,
            window,
            |_view, _state, event, _window, cx| {
                if matches!(event, InputEvent::Change) {
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
            settings_tab: 0,
            password_sign_in: false,
            integration_choice: 0,
            settings_install_select,
            completed_open: false,
            page_scrolls: page_scrolls.clone(),
            settings_tab_scrolls: (0..5).map(|_| ScrollHandle::new()).collect(),
            save_review_search,
            save_review_page: 0,
            save_review_expanded: None,
            save_review_show_ignored: false,
            save_review_attention_only: false,
            save_review_skipped_page: 0,
            activity_min_level: 0,
            activity_follow_log: true,
            activity_log_open: false,
            activity_search,
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
                    self.save_game_select.update(cx, |state, cx| {
                        state.set_items(SearchableVec::from(items), window, cx)
                    });
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
                // An applied folder normalizes the field it was typed in.
                AppEvent::MountRootConfigured { mount_root, .. } => {
                    self.mount_input.update(cx, |input, cx| {
                        input.set_value(mount_root.clone(), window, cx)
                    });
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
            self.state.apply_event(&event);
            changed = true;
        }
        if save_sync_updated {
            self.sync_install_select(window, cx);
            self.sync_settings_install_select(window, cx);
        }
        if new_logs && self.activity_follow_log {
            let max = self.log_scroll.max_offset();
            if self.log_scroll.offset().y <= -max.y + px(2.) {
                self.log_scroll.scroll_to_bottom();
            } else {
                self.activity_follow_log = false;
            }
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
            state.set_items(SearchableVec::from(items), window, cx);
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
            state.set_items(SearchableVec::from(items), window, cx);
            match selected {
                Some(value) => state.set_selected_value(&value, window, cx),
                None => state.set_selected_index(None, window, cx),
            }
        });
    }

    // --- render helpers ---

    /// The header marker is explicitly the *RomM connection* status — a
    /// bare "Connected" reads as app-wide health (UI-PLAN §3).
    fn conn_status(&self, cx: &App) -> (SharedString, StatusKind) {
        match self.state.conn {
            ConnectionState::Idle => ("RomM · not connected".into(), StatusKind::Muted),
            ConnectionState::Connecting => ("RomM · connecting…".into(), StatusKind::Warning),
            ConnectionState::Connected => {
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
            ConnectionState::Failed => ("RomM · connection failed".into(), StatusKind::Danger),
            ConnectionState::SignInRequired => {
                ("RomM · sign-in required".into(), StatusKind::Danger)
            }
        }
    }

    /// The folder marker derives from `UiState::folder_summary` — real mount
    /// events plus persisted config, never the connection state.
    fn mount_status(&self, _cx: &App) -> (SharedString, StatusKind) {
        if self.state.mount_stopping {
            return ("Stopping…".into(), StatusKind::Muted);
        }
        let summary = self.state.folder_summary();
        let kind = match summary {
            FolderSummary::SetupNeeded | FolderSummary::Stopped => StatusKind::Muted,
            FolderSummary::Starting => StatusKind::Muted,
            FolderSummary::Ready => StatusKind::Success,
            FolderSummary::Failed => StatusKind::Danger,
        };
        (summary.label().into(), kind)
    }

    /// The single derived save-sync status (UI-PLAN §5): zero linked files
    /// is "No files ready to sync", never "Up to date".
    fn save_sync_status(&self) -> (String, StatusKind) {
        let summary = self.state.save_sync_summary();
        let kind = match summary {
            SaveSyncSummary::AuthRequired | SaveSyncSummary::Offline => StatusKind::Danger,
            SaveSyncSummary::NeedsAttention => {
                if self.state.save_sync_review_count() > 0 {
                    StatusKind::Warning
                } else {
                    StatusKind::Danger
                }
            }
            SaveSyncSummary::NeedsSetup | SaveSyncSummary::Paused => StatusKind::Warning,
            SaveSyncSummary::Checking | SaveSyncSummary::Reconciling | SaveSyncSummary::Syncing => {
                StatusKind::Muted
            }
            // Nothing to sync yet is not a warning — it is simply empty (WS6.6).
            SaveSyncSummary::NoLinkedFiles => StatusKind::Muted,
            SaveSyncSummary::Off => StatusKind::Muted,
            SaveSyncSummary::UpToDate => StatusKind::Success,
        };
        (self.state.save_sync_summary_text(), kind)
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
        for (index, label) in ["Overview", "Saves", "Activity", "Settings"]
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
}

impl Render for RommfsWindow {
    /// The four-destination shell (UI-PLAN §2): persistent left nav, one
    /// changing detail region, content kept to a readable ~960px measure.
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.state.closing {
            return v_flex().size_full().p_6().gap_3().bg(cx.theme().background).text_color(cx.theme().foreground)
                .child(page_heading("Closing RomM-FS…"))
                .child(muted("Stopping the games folder and finishing cleanup. Keep this window open until RomM-FS exits.", cx))
                .child(Progress::new("shutdown-progress").loading(true)).into_any_element();
        }
        // Each destination tracks its own scroll handle: with one shared
        // call-site scroll state, every page inherited the previous page's
        // offset and Settings appeared to jump between screens.
        let scrolls = self.page_scrolls.clone();
        let page = move |index: usize, id: &'static str, max_width: f32, body: AnyElement| {
            let handle = scrolls[index].clone();
            div()
                .id(id)
                .flex_1()
                .min_w_0()
                .min_h_0()
                .track_scroll(&handle)
                .overflow_y_scroll()
                .vertical_scrollbar(&handle)
                .child(
                    v_flex()
                        .p_6()
                        .w_full()
                        .max_w(px(max_width))
                        .mx_auto()
                        .child(body),
                )
        };
        // Log and table views need the extra room; the reading measure stays
        // narrow for prose-heavy pages.
        let wide = 1440.;
        let content = match self.active_view {
            0 => page(
                0,
                "overview-view",
                960.,
                pages::overview::render(self, window, cx).into_any_element(),
            ),
            1 => page(
                1,
                "save-sync-view",
                wide,
                pages::save_review::render(self, window, cx).into_any_element(),
            ),
            2 => page(
                2,
                "activity-view",
                wide,
                pages::activity::render(self, window, cx).into_any_element(),
            ),
            4 => page(
                4,
                "setup-view",
                960.,
                pages::setup::render(self, window, cx).into_any_element(),
            ),
            // Settings owns its tab bar and per-tab scrolling, so it is not
            // wrapped in the shared page scroll region (phase 2c).
            _ => div()
                .id("settings-view")
                .flex_1()
                .min_w_0()
                .min_h_0()
                .child(pages::settings::render(self, window, cx)),
        };
        h_flex()
            .items_stretch()
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_sm()
            .child(self.render_sidebar(cx))
            .child(content)
            .into_any_element()
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
            cx.on_app_quit(move |cx| {
                let _instance = &quit_instance;
                let worker = quit_session.borrow().controller().begin_shutdown();
                cx.background_spawn(async move {
                    if let Some(worker) = worker {
                        let _ = worker.join();
                    }
                })
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
                                                let worker = tray_session
                                                    .borrow()
                                                    .controller()
                                                    .begin_shutdown();
                                                if let Some(worker) = worker {
                                                    crate::desktop::tray::show(hwnd);
                                                    cx.background_spawn(async move {
                                                        let _ = worker.join();
                                                    })
                                                    .await;
                                                    cx.update(|cx| cx.quit());
                                                    return;
                                                }
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
