use super::super::*;

/// Overview: the three truthful summaries (RomM connection, games folder,
/// save sync) each with the action appropriate to its current state
/// (UI-PLAN §3).
pub fn render(
    view: &mut RommfsWindow,
    window: &mut Window,
    cx: &mut Context<RommfsWindow>,
) -> impl IntoElement {
    view.render_overview(window, cx)
}
