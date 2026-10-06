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
    actions, div, fill, hsla, point, prelude::*, px, relative, rems, rgb, rgba, size, App,
    Application, Bounds, ClipboardItem, Context, CursorStyle, Div, Element, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable, FontWeight,
    GlobalElementId, IntoElement, KeyBinding, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, PathPromptOptions, Pixels, Point, Render,
    ScrollHandle, ShapedLine, SharedString, Stateful, Style, TextRun, Timer, UTF16Selection,
    UnderlineStyle, WeakEntity, Window, WindowBounds, WindowOptions,
};
use rommfs_core::events::{AppEvent, Level, SaveSyncIncomingStatus};
use rommfs_core::save_sync::{
    ExistingSavePreview, ExistingSaveScanStatus, MAX_EXISTING_SAVE_SCAN_DEPTH,
    MAX_EXISTING_SAVE_SCAN_ENTRIES,
};
use std::ops::Range;
use std::path::PathBuf;
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
        SaveSyncApplyDebounce,
        ActivateControl,
        FocusNext,
        FocusPrevious,
    ]
);

const LOG_CAP: usize = 500;
const POLL_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Clone, Copy)]
enum PaletteRole {
    Accent,
    AccentHover,
    Focus,
    Border,
    Disabled,
    Muted,
    Danger,
    Caution,
    Success,
}

fn palette_color(role: PaletteRole) -> u32 {
    match role {
        PaletteRole::Accent => 0x2f6df6,
        PaletteRole::AccentHover => 0x3d7bff,
        PaletteRole::Focus => 0x4c8dff,
        PaletteRole::Border => 0x3a3f55,
        PaletteRole::Disabled => 0x3a3f55,
        PaletteRole::Muted => 0x8a8fa8,
        PaletteRole::Danger => 0xe06060,
        PaletteRole::Caution => 0xe0b45c,
        PaletteRole::Success => 0x4caf7d,
    }
}

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
            focus_handle: cx.focus_handle().tab_stop(true),
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

    fn set_value(&mut self, value: impl Into<SharedString>) {
        self.content = value.into();
        let end = self.content.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
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
                rgb(palette_color(PaletteRole::Focus))
            } else {
                rgb(palette_color(PaletteRole::Border))
            })
            .when(focused, |style| style.border_2())
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
    debounce_input: Entity<TextInput>,
    debounce_input_applied: String,
    debounce_error: Option<String>,
    log_scroll: ScrollHandle,
}

impl RommfsWindow {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (controller, event_rx) = Controller::spawn();
        let url_input = cx
            .new(|cx| TextInput::new(cx, "http://romm.local:8080", SharedString::default(), false));
        let user_input =
            cx.new(|cx| TextInput::new(cx, "username", SharedString::default(), false));
        let password_input =
            cx.new(|cx| TextInput::new(cx, "password", SharedString::default(), true));
        let mount_input = cx.new(|cx| TextInput::new(cx, "mount folder", "C:\\RomM", false));
        let debounce_input = cx.new(|cx| {
            TextInput::new(
                cx,
                "5",
                rommfs_core::save_sync::DEFAULT_DEBOUNCE_SECS.to_string(),
                false,
            )
        });

        // Worker events arrive on the channel; poll it on the UI executor —
        // every applied event is a real fact, nothing fabricated (R5).
        cx.spawn(async move |this: WeakEntity<RommfsWindow>, cx| loop {
            Timer::after(POLL_INTERVAL).await;
            if this.update(cx, |view, cx| view.drain_events(cx)).is_err() {
                break;
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
            debounce_input,
            debounce_input_applied: rommfs_core::save_sync::DEFAULT_DEBOUNCE_SECS.to_string(),
            debounce_error: None,
            log_scroll: ScrollHandle::new(),
        };
        window.focus(&view.url_input.read(cx).focus_handle);
        view
    }

    /// Drain pending worker events into `UiState` and repaint if needed.
    fn drain_events(&mut self, cx: &mut Context<Self>) {
        let mut new_logs = false;
        let mut changed = false;
        while let Ok(event) = self.event_rx.try_recv() {
            if matches!(event, AppEvent::Log(_)) {
                new_logs = true;
            }
            match &event {
                AppEvent::SaveSyncSessionChanged { .. } => {
                    if self.debounce_input.read(cx).value() == self.debounce_input_applied {
                        let default = rommfs_core::save_sync::DEFAULT_DEBOUNCE_SECS.to_string();
                        self.debounce_input
                            .update(cx, |input, _| input.set_value(default.clone()));
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
                            .update(cx, |input, _| input.set_value(next.clone()));
                    }
                    self.debounce_input_applied = next;
                }
                _ => {}
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

    fn on_connect(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.state.conn == ConnState::Connecting {
            return;
        }
        self.controller.send(Command::Connect {
            url: self.url_input.read(cx).value().to_string(),
            username: self.user_input.read(cx).value().to_string(),
            password: self.password_input.read(cx).value().to_string(),
        });
    }

    fn on_start_mount(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.state.mount, MountState::Mounting | MountState::Mounted) {
            return;
        }
        let path = self.mount_input.read(cx).value().to_string();
        self.state.request_mount(path.clone());
        cx.notify();
        self.controller.send(Command::StartMount { path });
    }

    fn on_stop_mount(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
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
                    let _ = this.update(cx, |view, _| view.show_picker_error(error.to_string()));
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

    fn on_apply_debounce_action(
        &mut self,
        _: &SaveSyncApplyDebounce,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_save_sync_debounce(cx);
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
        let prompt = cx.prompt_for_new_path(&directory, Some(&suggested_name));
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

    fn on_copy_log(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
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

    fn focus_next(&mut self, _: &FocusNext, window: &mut Window, _: &mut Context<Self>) {
        window.focus_next();
    }

    fn focus_previous(&mut self, _: &FocusPrevious, window: &mut Window, _: &mut Context<Self>) {
        window.focus_prev();
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
            .text_color(rgb(palette_color(PaletteRole::Muted)))
            .child(SharedString::from(title.to_uppercase()))
    }

    fn command_control(
        window: &mut Window,
        cx: &mut Context<Self>,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        enabled: bool,
        bordered: bool,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + Clone + 'static,
    ) -> Stateful<Div> {
        let key = id.into();
        let focus_handle = Self::control_focus_handle(window, cx, key.clone(), enabled);
        // Keep focus visible if this control becomes disabled while focused.
        let focused = focus_handle.is_focused(window);
        let mut control = div()
            .id(key)
            .track_focus(&focus_handle)
            .key_context("CommandControl")
            .px_3()
            .h(rems(1.875))
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .text_sm()
            .child(label.into());
        if bordered {
            control = control.border_1().border_color(if focused {
                rgb(palette_color(PaletteRole::Focus))
            } else {
                rgb(palette_color(PaletteRole::Border))
            });
        }
        control = control.when(focused, |style| {
            style
                .border_2()
                .border_color(rgb(palette_color(PaletteRole::Focus)))
        });
        if enabled {
            let pointer_action = action.clone();
            let keyboard_action = action;
            control = control
                .bg(rgb(palette_color(PaletteRole::Accent)))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(palette_color(PaletteRole::AccentHover))))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |view, _, window, cx| pointer_action(view, window, cx)),
                )
                .on_action(cx.listener(move |view, _: &ActivateControl, window, cx| {
                    keyboard_action(view, window, cx);
                }));
        } else {
            control = control
                .bg(rgb(palette_color(PaletteRole::Disabled)))
                .text_color(rgb(palette_color(PaletteRole::Muted)))
                .cursor_default();
        }
        control
    }

    fn control_focus_handle(
        window: &mut Window,
        cx: &mut Context<Self>,
        key: SharedString,
        enabled: bool,
    ) -> FocusHandle {
        let focus = window.use_keyed_state(key, cx, |_, cx| cx.focus_handle().tab_stop(enabled));
        if focus.read(cx).tab_stop != enabled {
            focus.update(cx, |handle, _| {
                *handle = handle.clone().tab_stop(enabled);
            });
        }
        focus.read(cx).clone()
    }

    fn save_sync_status(&self) -> (&'static str, PaletteRole) {
        if self.state.conn == ConnState::SignInRequired
            || self.state.save_sync_authentication_required
        {
            return ("authentication required", PaletteRole::Danger);
        }
        if self.state.conn == ConnState::Failed {
            return ("offline", PaletteRole::Danger);
        }
        if !self.state.save_sync_enabled {
            return ("disabled", PaletteRole::Muted);
        }
        if self
            .state
            .save_sync_queue
            .as_ref()
            .is_some_and(|queue| queue.network_paused)
        {
            return ("paused", PaletteRole::Danger);
        }
        if !self.state.save_sync_available {
            return ("not ready", PaletteRole::Danger);
        }
        let Some(queue) = self.state.save_sync_queue.as_ref() else {
            return ("reconciling", PaletteRole::Caution);
        };
        if queue.actor_failed {
            return ("failed", PaletteRole::Danger);
        }
        if queue.failure.is_some() && queue.reconciled_games < queue.mapped_games {
            return ("retrying", PaletteRole::Caution);
        }
        if queue.reconciled_games < queue.mapped_games {
            return ("reconciling", PaletteRole::Caution);
        }
        if queue.attention_games > 0 || queue.pending_incoming > 0 {
            return ("review needed", PaletteRole::Caution);
        }
        if queue.failure.is_some() {
            return ("retrying", PaletteRole::Caution);
        }
        if queue.pending_outbound > 0 {
            return ("syncing", PaletteRole::Caution);
        }
        if queue.mapped_games > 0 && queue.reconciled_games == queue.mapped_games {
            return ("up to date", PaletteRole::Success);
        }
        ("waiting", PaletteRole::Muted)
    }

    fn render_save_sync(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let candidates = self.state.save_sync_candidates.clone();
        let preview = self.state.save_sync_preview.clone();
        let existing_saves = self.state.save_sync_existing_saves.clone();
        let queue = self.state.save_sync_queue.clone();
        let (status, status_color) = self.save_sync_status();
        let candidate_summary = if candidates.is_empty() {
            "No ready RetroBat installations found".to_string()
        } else {
            format!("{} ready installation(s)", candidates.len())
        };
        let selected = self
            .state
            .save_sync_selected_root
            .as_deref()
            .map(|path| format!("Selected installation: {path}"))
            .unwrap_or_else(|| "Selected installation: none".into());
        let effective_root = self
            .state
            .save_sync_effective_saves_root
            .as_deref()
            .map(|path| format!("Verified effective saves folder: {path}"))
            .unwrap_or_else(|| "Effective saves folder: not verified".into());
        let profile = self
            .state
            .save_sync_profile_version
            .as_deref()
            .map(|version| format!("Verified RetroBat profile: {version}, Game Boy/Gambatte"))
            .unwrap_or_else(|| "Verified RetroBat profile: unavailable".into());
        let account = self
            .state
            .save_sync_account_id
            .map(|id| format!("Authenticated RomM account ID: {id}"))
            .unwrap_or_else(|| "Authenticated RomM account ID: unavailable".into());
        let server = self
            .state
            .save_sync_server_id
            .as_deref()
            .map(|id| format!("Server scope: {id}"))
            .unwrap_or_else(|| "Server scope: unavailable".into());
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
        let last_failure = queue
            .as_ref()
            .and_then(|queue| queue.failure.as_ref())
            .or(self.state.save_sync_failure.as_ref())
            .cloned();
        let problem = self.state.save_sync_problem.clone();
        let incoming_rows = queue
            .as_ref()
            .map(|queue| queue.incoming.clone())
            .unwrap_or_default();
        let games = queue
            .as_ref()
            .map(|queue| queue.games.clone())
            .unwrap_or_else(|| self.state.save_sync_games.clone());
        let can_export = self.state.save_sync_effective_saves_root.is_some()
            && self.state.save_sync_account_id.is_some();

        div()
            .flex()
            .flex_col()
            .gap_1()
            .max_h(rems(18.75))
            .id("save-sync-panel")
            .overflow_y_scroll()
            .py_1()
            .child(self.section("Save sync"))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(Self::command_control(
                        window,
                        cx,
                        "save-sync-control-browse",
                        "Browse…",
                        true,
                        true,
                        |view, _window, cx| view.browse_save_sync(cx),
                    ))
                    .child(Self::command_control(
                        window,
                        cx,
                        "save-sync-control-refresh",
                        "Refresh",
                        true,
                        true,
                        |view, _window, _cx| view.controller.send(Command::RefreshSaveSync),
                    ))
                    .child(Self::command_control(
                        window,
                        cx,
                        "save-sync-control-toggle",
                        if self.state.save_sync_enabled {
                            "Save sync: on (disable)"
                        } else {
                            "Enable save sync"
                        },
                        self.state.save_sync_available || self.state.save_sync_enabled,
                        true,
                        |view, _window, cx| {
                            let enabled = !view.state.save_sync_enabled;
                            if !enabled {
                                view.state.save_sync_enabled = false;
                                cx.notify();
                            }
                            view.controller
                                .send(Command::SetSaveSyncEnabled { enabled });
                        },
                    ))
                    .child(
                        div()
                            .text_color(rgb(palette_color(status_color)))
                            .child(format!("Save sync: {status}")),
                    ),
            )
            .child(div().text_xs().child(server))
            .child(div().text_xs().child(selected))
            .child(div().text_xs().child(effective_root))
            .child(div().text_xs().child(profile))
            .child(div().text_xs().child(account))
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(palette_color(PaletteRole::Muted)))
                    .child(format!(
                        "{candidate_summary}; {} mapped game targets; {} catalogue entries excluded; {} installations skipped",
                        self.state.save_sync_mapped_targets,
                        self.state.save_sync_catalogue_unmapped,
                        self.state.save_sync_skipped,
                    )),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(palette_color(PaletteRole::Muted)))
                    .child(existing_save_inventory_summary(existing_saves.as_ref())),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(palette_color(PaletteRole::Muted)))
                    .child(format!(
                        "Read-only filename/metadata scan; save contents are not opened. Bounded to {MAX_EXISTING_SAVE_SCAN_ENTRIES} entries and {MAX_EXISTING_SAVE_SCAN_DEPTH} directory levels."
                    )),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(palette_color(PaletteRole::Muted)))
                    .child("Preview only: gb/*.gb → gb/<visible ROM stem>.srm. RTC companions, .gbc, and other profiles are skipped. Existing remote differences require review."),
            )
            .children(preview.into_iter().map(|path| {
                div()
                    .text_xs()
                    .text_color(rgb(palette_color(PaletteRole::Muted)))
                    .child(format!("Target preview: {path}"))
            }))
            .child(
                div()
                    .text_xs()
                    .child("Opt-in uploads local SRAM snapshots. Remote saves are installed automatically only at paths that have never existed; existing saves are never replaced."),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(div().text_xs().child("Debounce (seconds)"))
                    .child(
                        div()
                            .w(rems(5.25))
                            .key_context("SaveSyncDebounce")
                            .on_action(cx.listener(Self::on_apply_debounce_action))
                            .child(self.debounce_input.clone()),
                    )
                    .child(Self::command_control(
                        window,
                        cx,
                        "save-sync-control-apply-debounce",
                        "Apply",
                        self.state.save_sync_available || self.state.save_sync_enabled,
                        true,
                        |view, _window, cx| view.apply_save_sync_debounce(cx),
                    )),
            )
            .children(self.debounce_error.clone().map(|error| {
                div()
                    .text_xs()
                    .text_color(rgb(palette_color(PaletteRole::Danger)))
                    .child(error)
            }))
            .children(candidates.into_iter().map(|candidate| {
                let path = candidate.info.install_root.clone();
                let selected = self
                    .state
                    .save_sync_selected_root
                    .as_deref()
                    .is_some_and(|root| root.eq_ignore_ascii_case(&path.to_string_lossy()));
                let label = format!(
                    "{}{} — {:?}",
                    if selected { "Selected: " } else { "Use: " },
                    path.display(),
                    candidate.sources
                );
                Self::command_control(
                    window,
                    cx,
                    format!("save-sync-control-candidate-{}", path.display()),
                    label,
                    true,
                    true,
                    move |view, _window, cx| {
                        view.controller
                            .send(Command::SelectSaveSyncInstallation { path: path.clone() });
                        cx.notify();
                    },
                )
            }))
            .children(problem.map(|problem| {
                div()
                    .text_xs()
                    .text_color(rgb(palette_color(PaletteRole::Caution)))
                    .child(format!("Paused: {problem}"))
            }))
            .children(last_failure.map(|failure| {
                div()
                    .text_xs()
                    .text_color(rgb(palette_color(PaletteRole::Danger)))
                    .child(format!("Last failure: {failure}"))
            }))
            .child(
                self.section("Affected games")
            )
            .child(
                div()
                    .text_xs()
                    .child(queue.as_ref().map_or_else(
                        || "No save reconciliation yet".to_string(),
                        |queue| {
                            format!(
                                "{} mapped · {} reconciled · {} pending uploads · {} incoming · {} games need attention",
                                queue.mapped_games,
                                queue.reconciled_games,
                                queue.pending_outbound,
                                queue.pending_incoming,
                                queue.attention_games,
                            )
                        },
                    )),
            )
            .children(last_action.map(|action| {
                div()
                    .text_xs()
                    .text_color(rgb(palette_color(PaletteRole::Muted)))
                    .child(format!("Last action: {action}"))
            }))
            .child(
                div()
                    .id("save-sync-games")
                    .max_h(rems(4.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(games.into_iter().map(|game| {
                        let hashes = format!(
                            "local {} · remote ID {} · remote {}",
                            game.local_hash.as_deref().unwrap_or("unknown"),
                            game.remote_id.as_deref().unwrap_or("none"),
                            game.remote_hash.as_deref().unwrap_or("unknown"),
                        );
                        div()
                            .flex()
                            .flex_col()
                            .text_xs()
                            .child(format!("{} (ROM {}) · {hashes}", game.rom_name, game.rom_id))
                            .children(game.issue.map(|issue| {
                                div()
                                    .text_color(rgb(palette_color(PaletteRole::Caution)))
                                    .child(format!("Attention: {issue}"))
                            }))
                    })),
            )
            .child(
                self.section("Incoming saves · export only")
            )
            .child(
                div()
                    .id("save-sync-incoming")
                    .max_h(rems(5.5))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(incoming_rows.iter().cloned().map(|incoming| {
                        let id = incoming.incoming_id.clone();
                        let label = format!(
                            "{} · RomM save {} · pending revision {} · {} · {} · {}",
                            incoming.rom_name,
                            incoming.remote_id,
                            incoming.incoming_id,
                            incoming.state,
                            incoming.reason,
                            incoming.content_hash,
                        );
                        let error = self.state.save_sync_export_errors.get(&id).cloned();
                        let feedback = self.state.save_sync_export_feedback.get(&id).cloned();
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_xs().child(label))
                            .child(Self::command_control(
                                window,
                                cx,
                                format!("save-sync-control-export-{id}"),
                                "Export…",
                                can_export,
                                true,
                                move |view, _window, cx| {
                                    view.prompt_export(incoming.clone(), cx);
                                },
                            ))
                            .children(error.map(|error| {
                                div()
                                    .text_xs()
                                    .text_color(rgb(palette_color(PaletteRole::Danger)))
                                    .child(format!("Export failed: {error}"))
                            }))
                            .children(feedback.map(|path| {
                                div()
                                    .text_xs()
                                    .text_color(rgb(palette_color(PaletteRole::Success)))
                                    .child(format!("Exported to {path}; review remains pending."))
                            }))
                    })),
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
            .key_context("RommfsWindow")
            .on_action(cx.listener(Self::focus_next))
            .on_action(cx.listener(Self::focus_previous))
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
                            .child(Self::command_control(
                                window,
                                cx,
                                "button-connect",
                                "Connect",
                                !connecting,
                                false,
                                |view, window, cx| view.on_connect(window, cx),
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
                            .child(Self::command_control(
                                window,
                                cx,
                                "button-start-mount",
                                "Start",
                                !mounted && !mounting,
                                false,
                                |view, window, cx| view.on_start_mount(window, cx),
                            ))
                            .child(Self::command_control(
                                window,
                                cx,
                                "button-stop-mount",
                                "Stop",
                                mounted,
                                false,
                                |view, window, cx| view.on_stop_mount(window, cx),
                            )),
                    ),
            )
            .child(self.render_save_sync(window, cx))
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
                                Self::command_control(
                                    window,
                                    cx,
                                    "button-copy-log",
                                    "Copy",
                                    true,
                                    false,
                                    |view, window, cx| view.on_copy_log(window, cx),
                                )
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
            KeyBinding::new("tab", FocusNext, Some("RommfsWindow")),
            KeyBinding::new("shift-tab", FocusPrevious, Some("RommfsWindow")),
            KeyBinding::new("enter", ActivateControl, Some("CommandControl")),
            KeyBinding::new("space", ActivateControl, Some("CommandControl")),
            KeyBinding::new("enter", SaveSyncApplyDebounce, Some("SaveSyncDebounce")),
        ]);

        let bounds = Bounds::centered(None, size(px(720.0), px(680.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("RomMFS".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| RommfsWindow::new(window, cx)),
        )
        .unwrap();
        cx.activate(true);
    });
}
