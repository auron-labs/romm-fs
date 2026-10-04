//! The single GPUI window (PRD R5): connection form, mount controls,
//! download progress, bounded scrollable log. Renders `UiState` only —
//! no timers, no fabricated progress, no business logic here.
//!
//! Text input uses GPUI's real `EntityInputHandler`/`ElementInputHandler`
//! story (same pattern as gpui's own `examples/input.rs`), adapted without
//! `unicode-segmentation` (not a declared dependency) — cursor movement is
//! per Unicode scalar instead of grapheme cluster.

use crate::controller::{Command, ConnState, Controller, MountState, UiState};
use gpui::{
    actions, div, fill, hsla, point, prelude::*, px, relative, rgb, rgba, size, App, Application,
    Bounds, ClipboardItem, Context, CursorStyle, Div, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, FocusHandle, Focusable, FontWeight, GlobalElementId, IntoElement,
    KeyBinding, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad,
    Pixels, Point, Render, ScrollHandle, ShapedLine, SharedString, Style, TextRun, Timer,
    UTF16Selection, UnderlineStyle, WeakEntity, Window, WindowBounds, WindowOptions,
};
use rommfs_core::events::{AppEvent, Level};
use std::ops::Range;
use std::sync::mpsc;
use std::time::Duration;

actions!(
    text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        Paste,
        Cut,
        Copy,
    ]
);

const LOG_CAP: usize = 500;
const POLL_INTERVAL: Duration = Duration::from_millis(200);

// ---------------------------------------------------------------------------
// Minimal single-line text input (masked flag for the password field).
// ---------------------------------------------------------------------------

struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    masked: bool,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
}

impl TextInput {
    fn new(
        cx: &mut Context<Self>,
        placeholder: impl Into<SharedString>,
        initial: impl Into<SharedString>,
        masked: bool,
    ) -> Self {
        let content: SharedString = initial.into();
        let end = content.len();
        Self {
            focus_handle: cx.focus_handle(),
            content,
            placeholder: placeholder.into(),
            masked,
            selected_range: end..end,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
        }
    }

    fn value(&self) -> &str {
        &self.content
    }

    /// What is drawn — masked fields render one `*` per content byte so all
    /// index math stays 1:1 with the real content.
    fn display_text(&self) -> SharedString {
        if self.masked && !self.content.is_empty() {
            "*".repeat(self.content.len()).into()
        } else {
            self.content.clone()
        }
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_selecting = true;
        window.focus(&self.focus_handle(cx));

        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text.replace('\n', " "), window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        // Never leak masked (password) content to the clipboard.
        if self.masked {
            return;
        }
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if self.masked {
            return;
        }
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify()
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        line.closest_index_for_x(position.x - bounds.left())
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify()
    }

    // UTF-8/UTF-16 boundary mapping (the platform IME speaks UTF-16).
    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    /// Nearest previous char boundary (scalar boundary — no grapheme
    /// clustering; that is sufficient for URL/user/password fields).
    fn previous_boundary(&self, offset: usize) -> usize {
        let offset = offset.min(self.content.len());
        self.content[..offset]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .char_indices()
            .find(|(i, _)| *i > offset)
            .map(|(i, _)| i)
            .unwrap_or(self.content.len())
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        *actual_range = Some(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.marked_range.take();
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        if !new_text.is_empty() {
            self.marked_range = Some(range.start..range.start + new_text.len());
        } else {
            self.marked_range = None;
        }
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range| new_range.start + range.start..new_range.end + range.end)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(
                bounds.left() + last_layout.x_for_index(range.start),
                bounds.top(),
            ),
            point(
                bounds.left() + last_layout.x_for_index(range.end),
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let line_point = self.last_bounds?.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;
        // The shaped line holds the *display* text (masked for passwords).
        if last_layout.text != self.display_text() {
            return None;
        }
        let utf8_index = last_layout.index_for_x(point.x - line_point.x)?;
        Some(self.offset_to_utf16(utf8_index))
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let display_text = input.display_text();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let style = window.text_style();

        let (display_text, text_color) = if input.content.is_empty() {
            (input.placeholder.clone(), hsla(0., 0., 0., 0.45))
        } else {
            (display_text, style.color)
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked_range) = input.marked_range.as_ref() {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked_range.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);

        let cursor_pos = line.x_for_index(cursor);
        let (selection, cursor) = if selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + cursor_pos, bounds.top()),
                        size(px(2.), bounds.bottom() - bounds.top()),
                    ),
                    gpui::blue(),
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + line.x_for_index(selected_range.start),
                            bounds.top(),
                        ),
                        point(
                            bounds.left() + line.x_for_index(selected_range.end),
                            bounds.bottom(),
                        ),
                    ),
                    rgba(0x4c8dff55),
                )),
                None,
            )
        };
        PrepaintState {
            line: Some(line),
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection)
        }
        let line = prepaint.line.take().unwrap();
        line.paint(bounds.origin, window.line_height(), window, cx)
            .unwrap();

        if focus_handle.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }

        self.input.update(cx, |input, _cx| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus_handle.is_focused(window);
        div()
            .flex()
            .key_context("TextInput")
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .bg(rgb(0x14141f))
            .border_1()
            .border_color(if focused {
                rgb(0x4c8dff)
            } else {
                rgb(0x3a3f55)
            })
            .rounded_md()
            .w_full()
            .h(px(30.))
            .px_2()
            .items_center()
            .child(TextElement { input: cx.entity() })
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

// ---------------------------------------------------------------------------
// The window view: renders `UiState`, forwards clicks as `Command`s.
// ---------------------------------------------------------------------------

struct RommfsWindow {
    controller: Controller,
    event_rx: mpsc::Receiver<AppEvent>,
    state: UiState,
    url_input: Entity<TextInput>,
    user_input: Entity<TextInput>,
    password_input: Entity<TextInput>,
    mount_input: Entity<TextInput>,
    focus_handle: FocusHandle,
    log_scroll: ScrollHandle,
}

impl RommfsWindow {
    fn new(cx: &mut Context<Self>) -> Self {
        let (controller, event_rx) = Controller::spawn();
        let url_input = cx
            .new(|cx| TextInput::new(cx, "http://romm.local:8080", SharedString::default(), false));
        let user_input =
            cx.new(|cx| TextInput::new(cx, "username", SharedString::default(), false));
        let password_input =
            cx.new(|cx| TextInput::new(cx, "password", SharedString::default(), true));
        let mount_input = cx.new(|cx| TextInput::new(cx, "mount folder", "C:\\RomM", false));

        // Worker events arrive on the channel; poll it on the UI executor —
        // every applied event is a real fact, nothing fabricated (R5).
        cx.spawn(async move |this: WeakEntity<RommfsWindow>, cx| loop {
            Timer::after(POLL_INTERVAL).await;
            if this.update(cx, |view, cx| view.drain_events(cx)).is_err() {
                break;
            }
        })
        .detach();

        Self {
            controller,
            event_rx,
            state: UiState::new(LOG_CAP),
            url_input,
            user_input,
            password_input,
            mount_input,
            focus_handle: cx.focus_handle(),
            log_scroll: ScrollHandle::new(),
        }
    }

    /// Drain pending worker events into `UiState` and repaint if needed.
    fn drain_events(&mut self, cx: &mut Context<Self>) {
        let mut new_logs = false;
        let mut changed = false;
        while let Ok(event) = self.event_rx.try_recv() {
            if matches!(event, AppEvent::Log(_)) {
                new_logs = true;
            }
            self.state.apply(&event);
            changed = true;
        }
        if new_logs {
            self.log_scroll.scroll_to_bottom();
        }
        if changed {
            cx.notify();
        }
    }

    fn on_connect(&mut self, _: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.state.conn == ConnState::Connecting {
            return;
        }
        self.controller.send(Command::Connect {
            url: self.url_input.read(cx).value().to_string(),
            username: self.user_input.read(cx).value().to_string(),
            password: self.password_input.read(cx).value().to_string(),
        });
    }

    fn on_start_mount(&mut self, _: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.state.mount, MountState::Mounting | MountState::Mounted) {
            return;
        }
        self.controller.send(Command::StartMount {
            path: self.mount_input.read(cx).value().to_string(),
        });
    }

    fn on_stop_mount(&mut self, _: &MouseUpEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        if self.state.mount != MountState::Mounted {
            return;
        }
        self.controller.send(Command::StopMount);
    }

    fn on_copy_log(&mut self, _: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let body = self
            .state
            .log
            .lines()
            .map(|l| {
                format!(
                    "{} {:<5} [{}] {}",
                    fmt_time(l.unix_secs),
                    level_str(l.level),
                    l.op,
                    l.message
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        cx.write_to_clipboard(ClipboardItem::new_string(body));
    }

    // --- render helpers ---

    fn conn_status(&self) -> (&'static str, u32) {
        match self.state.conn {
            ConnState::Idle => ("not connected", 0x8a8fa8),
            ConnState::Connecting => ("connecting…", 0xd8a94a),
            ConnState::Connected => ("connected", 0x4caf7d),
            ConnState::Failed => ("connection failed", 0xe06060),
            ConnState::SignInRequired => ("sign-in required", 0xe06060),
        }
    }

    fn mount_status(&self) -> (String, u32) {
        match self.state.mount {
            MountState::NotMounted => ("not mounted".to_string(), 0x8a8fa8),
            MountState::Mounting => ("mounting…".to_string(), 0xd8a94a),
            MountState::Mounted => (
                format!(
                    "mounted at {}",
                    self.state.mount_path.as_deref().unwrap_or("")
                ),
                0x4caf7d,
            ),
            MountState::Failed => ("mount failed".to_string(), 0xe06060),
        }
    }

    fn section<'a>(&self, title: &'a str) -> impl IntoElement + 'a {
        div()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(0x8a8fa8))
            .child(SharedString::from(title.to_uppercase()))
    }

    fn button<'a>(
        &self,
        label: &'a str,
        enabled: bool,
        on: Option<impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static>,
    ) -> Div {
        let mut el = div()
            .px_3()
            .h(px(30.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .text_sm()
            .child(SharedString::from(label.to_string()));
        if enabled {
            el = el
                .bg(rgb(0x2f6df6))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x3d7bff)));
            if let Some(on) = on {
                el = el.on_mouse_up(MouseButton::Left, on);
            }
        } else {
            el = el.bg(rgb(0x3a3f55)).text_color(rgb(0x8a8fa8));
        }
        el
    }
}

fn level_str(level: Level) -> &'static str {
    match level {
        Level::Info => "INFO",
        Level::Warn => "WARN",
        Level::Error => "ERROR",
    }
}

fn level_color(level: Level) -> u32 {
    match level {
        Level::Info => 0x9aa4c0,
        Level::Warn => 0xd8a94a,
        Level::Error => 0xe06060,
    }
}

/// `HH:MM:SS` (UTC) from unix seconds — no clock dependency needed for the
/// PoC log view.
fn fmt_time(unix_secs: u64) -> String {
    let secs = unix_secs % 86_400;
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
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

impl Render for RommfsWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (conn_text, conn_color) = self.conn_status();
        let (mount_text, mount_color) = self.mount_status();
        let connecting = self.state.conn == ConnState::Connecting;
        let mounted = self.state.mount == MountState::Mounted;
        let mounting = self.state.mount == MountState::Mounting;

        div()
            .flex()
            .flex_col()
            .size_full()
            .p_4()
            .gap_3()
            .bg(rgb(0x1e1e2e))
            .text_color(rgb(0xdde1ee))
            .text_sm()
            .track_focus(&self.focus_handle(cx))
            .child(
                // Header + live status line.
                div()
                    .flex()
                    .flex_row()
                    .items_baseline()
                    .justify_between()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .child("RomMFS"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_4()
                            .child(div().text_color(rgb(conn_color)).child(conn_text))
                            .child(div().text_color(rgb(mount_color)).child(mount_text)),
                    ),
            )
            .child(
                // Connection area.
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(self.section("Connection"))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .child(div().flex_1().child(self.url_input.clone()))
                            .child(div().w(px(160.)).child(self.user_input.clone()))
                            .child(div().w(px(160.)).child(self.password_input.clone()))
                            .child(self.button(
                                "Connect",
                                !connecting,
                                Some(cx.listener(Self::on_connect)),
                            )),
                    ),
            )
            .child(
                // Mount area.
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(self.section("Mount"))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .child(div().flex_1().child(self.mount_input.clone()))
                            .child(self.button(
                                "Start",
                                !mounted && !mounting,
                                Some(cx.listener(Self::on_start_mount)),
                            ))
                            .child(self.button(
                                "Stop",
                                mounted,
                                Some(cx.listener(Self::on_stop_mount)),
                            )),
                    ),
            )
            .child(
                // Downloads: real worker events only (R5).
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(self.section("Downloads"))
                    .child(
                        div()
                            .id("downloads")
                            .flex()
                            .flex_col()
                            .gap_1()
                            .max_h(px(120.))
                            .overflow_y_scroll()
                            .children(if self.state.downloads.is_empty() {
                                vec![div().text_color(rgb(0x6b7089)).child("No downloads yet")]
                            } else {
                                self.state
                                    .downloads
                                    .iter()
                                    .map(|d| {
                                        let status = match &d.finished {
                                            None => ("downloading", 0xd8a94a),
                                            Some(Ok(())) => ("done", 0x4caf7d),
                                            Some(Err(_)) => ("failed", 0xe06060),
                                        };
                                        let bytes = match (d.total, &d.finished) {
                                            (Some(total), _) => format!(
                                                "{} / {} ({:.0}%)",
                                                fmt_bytes(d.received),
                                                fmt_bytes(total),
                                                100.0 * d.received as f64 / total.max(1) as f64
                                            ),
                                            (None, _) => format!("{} —", fmt_bytes(d.received)),
                                        };
                                        div()
                                            .flex()
                                            .flex_row()
                                            .justify_between()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_row()
                                                    .gap_2()
                                                    .min_w_0()
                                                    .child(
                                                        div()
                                                            .whitespace_nowrap()
                                                            .child(d.file_name.clone()),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_color(rgb(0x8a8fa8))
                                                            .whitespace_nowrap()
                                                            .child(bytes),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_color(rgb(status.1))
                                                    .whitespace_nowrap()
                                                    .child(status.0),
                                            )
                                    })
                                    .collect::<Vec<_>>()
                            }),
                    ),
            )
            .child(
                // Log area: bounded, scrollable, copyable.
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .items_center()
                            .child(self.section("Log"))
                            .child(
                                self.button("Copy", true, Some(cx.listener(Self::on_copy_log)))
                                    .h(px(22.))
                                    .px_2()
                                    .text_xs(),
                            ),
                    )
                    .child(
                        div()
                            .id("log")
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .bg(rgb(0x12121c))
                            .rounded_md()
                            .p_2()
                            .overflow_y_scroll()
                            .track_scroll(&self.log_scroll)
                            .children(self.state.log.lines().map(|l| {
                                div()
                                    .text_xs()
                                    .whitespace_nowrap()
                                    .text_color(rgb(level_color(l.level)))
                                    .child(format!(
                                        "{} {:<5} [{}] {}",
                                        fmt_time(l.unix_secs),
                                        level_str(l.level),
                                        l.op,
                                        l.message
                                    ))
                            })),
                    ),
            )
            .children(
                // Error line(s): connection and mount errors are always
                // visible, never silently dropped (R1/R5).
                [
                    self.state.conn_error.clone(),
                    self.state.mount_error.clone(),
                ]
                .into_iter()
                .flatten()
                .map(|err| {
                    div()
                        .text_xs()
                        .text_color(rgb(0xe06060))
                        .child(format!("error: {err}"))
                })
                .collect::<Vec<_>>(),
            )
    }
}

impl Focusable for RommfsWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Launch the GPUI app; blocks until the window closes. Closing stops the
/// worker session and mount (PRD: no tray/background process).
pub fn run() {
    Application::new().run(|cx: &mut App| {
        // "secondary" is ctrl on non-macOS, cmd on macOS.
        cx.bind_keys([
            KeyBinding::new("backspace", Backspace, None),
            KeyBinding::new("delete", Delete, None),
            KeyBinding::new("left", Left, None),
            KeyBinding::new("right", Right, None),
            KeyBinding::new("shift-left", SelectLeft, None),
            KeyBinding::new("shift-right", SelectRight, None),
            KeyBinding::new("secondary-a", SelectAll, None),
            KeyBinding::new("secondary-v", Paste, None),
            KeyBinding::new("secondary-c", Copy, None),
            KeyBinding::new("secondary-x", Cut, None),
            KeyBinding::new("home", Home, None),
            KeyBinding::new("end", End, None),
        ]);

        let bounds = Bounds::centered(None, size(px(720.0), px(560.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("RomMFS".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new(RommfsWindow::new),
        )
        .unwrap();
        cx.activate(true);
    });
}
