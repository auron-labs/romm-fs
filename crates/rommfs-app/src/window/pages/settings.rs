use super::super::*;

/// Settings page: connection + optional setup. UI-PLAN task 3 reorganizes it
/// into labelled sections.
pub fn render(
    view: &mut RommfsWindow,
    window: &mut Window,
    cx: &mut Context<RommfsWindow>,
) -> impl IntoElement {
    view.render_settings(window, cx)
}
