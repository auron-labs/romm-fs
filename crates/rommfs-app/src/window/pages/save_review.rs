use super::super::*;

/// Save sync page. Currently the existing review body; UI-PLAN task 4
/// replaces it with the searchable, paginated review table.
pub fn render(
    view: &mut RommfsWindow,
    window: &mut Window,
    cx: &mut Context<RommfsWindow>,
) -> impl IntoElement {
    v_flex()
        .gap_4()
        .child(page_heading("Save sync"))
        .child(view.render_save_sync(window, cx))
}
