//! Shared presentation helpers for the window's destination pages.

use super::*;

/// Semantic status roles mapped onto the active theme — no raw colors in
/// application code.
#[derive(Clone, Copy)]
pub(super) enum StatusKind {
    Muted,
    Success,
    Warning,
    Danger,
}

impl StatusKind {
    pub(super) fn color(self, cx: &App) -> Hsla {
        let theme = cx.theme();
        match self {
            StatusKind::Muted => theme.muted_foreground,
            StatusKind::Success => theme.success,
            StatusKind::Warning => theme.warning,
            StatusKind::Danger => theme.danger,
        }
    }
}

pub(super) fn status_dot(color: Hsla) -> impl IntoElement {
    div().size_2().flex_shrink_0().rounded_full().bg(color)
}

/// Leading dot + state text used in the header and section status lanes.
pub(super) fn status_marker(
    kind: StatusKind,
    text: impl Into<SharedString>,
    cx: &App,
) -> impl IntoElement {
    h_flex()
        .gap_2()
        .child(status_dot(kind.color(cx)))
        .child(div().whitespace_nowrap().child(text.into()))
}

pub(super) fn muted(text: impl Into<String>, cx: &App) -> impl IntoElement {
    div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

pub(super) fn muted_xs(text: impl Into<String>, cx: &App) -> impl IntoElement {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

pub(super) fn error_line(text: impl Into<String>, cx: &App) -> impl IntoElement {
    div()
        .text_sm()
        .text_color(cx.theme().danger)
        .child(text.into())
}

/// "3 systems" — counts read as numbers, never "system(s)" (phase 2a rule 6).
pub(super) fn plural(count: usize, singular: &str) -> String {
    let suffix = if count == 1 { "" } else { "s" };
    format!("{} {singular}{suffix}", fmt_count(count))
}

/// A quiet ⓘ affordance: hover or keyboard focus reveals the explanation that
/// would otherwise be a sentence in the page body (phase 2a rule 2).
pub(super) fn info_button(id: &'static str, tooltip: &'static str) -> Button {
    Button::new(id)
        .ghost()
        .small()
        .icon(Icon::new(IconName::Info))
        .tooltip(tooltip)
        .accessibility_label("More information")
}

/// A short count or state chip (phase 2a rule 6: numbers, not prose).
pub(super) fn chip(text: impl Into<SharedString>) -> Tag {
    Tag::secondary().small().child(text.into())
}

/// "Technical details" disclosure for failure blocks: a quiet toggle plus
/// the complete diagnostic text in mono. The full path and cause chain
/// stay reachable even where the summary line is shortened (UI-PLAN §6).
pub(super) fn details_disclosure(
    id: impl Into<SharedString>,
    detail: String,
    window: &mut Window,
    cx: &mut Context<RommfsWindow>,
) -> impl IntoElement {
    let id = id.into();
    let open = window.use_keyed_state(SharedString::from(format!("{id}-open")), cx, |_, _| {
        std::cell::Cell::new(false)
    });
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

/// A problem in the shared shape (UI-PLAN §4): bold title, one-line cause,
/// optional action, and the raw text behind "Technical details".
pub(super) fn problem_block(
    id: &'static str,
    problem: &UserProblem,
    action: Option<Button>,
    window: &mut Window,
    cx: &mut Context<RommfsWindow>,
) -> impl IntoElement {
    let color = match problem.severity {
        Severity::Error => cx.theme().danger,
        Severity::Warning => cx.theme().warning,
    };
    v_flex()
        .gap_1()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color)
                .child(problem.title.clone()),
        )
        .when_some(problem.cause.clone(), |this, cause| {
            this.child(muted(cause, cx))
        })
        .when_some(action, |this, button| {
            this.child(div().pt_1().child(button))
        })
        .when_some(problem.technical.clone(), |this, technical| {
            this.child(details_disclosure(
                SharedString::from(format!("{id}-details")),
                technical,
                window,
                cx,
            ))
        })
}

/// A quiet toggle that expands to a plain list of lines.
pub(super) fn notes_disclosure(
    id: &'static str,
    label: String,
    lines: Vec<String>,
    window: &mut Window,
    cx: &mut Context<RommfsWindow>,
) -> impl IntoElement {
    let open = window.use_keyed_state(SharedString::from(format!("{id}-open")), cx, |_, _| {
        std::cell::Cell::new(false)
    });
    let is_open = open.read(cx).get();
    let toggle = open.clone();
    v_flex()
        .gap_1()
        .child(
            Button::new(id)
                .ghost()
                .small()
                .label(if is_open { "Hide".to_string() } else { label })
                .on_click(cx.listener(move |_, _, _, cx| {
                    toggle.update(cx, |open, _| open.set(!open.get()));
                    cx.notify();
                })),
        )
        .when(is_open, |this| {
            this.children(lines.into_iter().map(|line| muted_xs(line, cx)))
        })
}

/// The shared failure presentation (UI-PLAN §6): a title, the real reason
/// as the explanation, the relevant action, and expandable details —
/// placed at the top of the section the failure belongs to.
pub(super) fn failure_block(
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
pub(super) fn failure_explanation(error: &str) -> String {
    error
        .split_once(" failed: ")
        .map(|(_, cause)| cause.to_string())
        .unwrap_or_else(|| error.to_string())
}

pub(super) fn section_heading(text: &'static str) -> impl IntoElement {
    div()
        .text_base()
        .font_weight(FontWeight::SEMIBOLD)
        .child(text)
}

/// ~24px page title (UI-PLAN §2 visual scale).
pub(super) fn page_heading(text: &'static str) -> impl IntoElement {
    div()
        .text_2xl()
        .font_weight(FontWeight::SEMIBOLD)
        .child(text)
}

/// Label above a control, used by the connection row.
pub(super) fn field_column(
    label: &'static str,
    control: impl IntoElement,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(
            Label::new(label)
                .text_xs()
                .text_color(cx.theme().muted_foreground),
        )
        .child(control)
}

pub(super) fn operation_progress(
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
