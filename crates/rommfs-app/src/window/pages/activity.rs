use super::super::*;

/// Activity page. Currently downloads + log; UI-PLAN task 5 reshapes it into
/// an operations-first view.
pub fn render(
    view: &mut RommfsWindow,
    window: &mut Window,
    cx: &mut Context<RommfsWindow>,
) -> impl IntoElement {
    view.render_activity(window, cx)
}
