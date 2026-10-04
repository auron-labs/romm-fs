//! The single GPUI window (PRD R5): connection form, mount controls,
//! download progress, bounded scrollable log. Renders `UiState` only —
//! no timers, no fabricated progress, no business logic here.

/// Launch the GPUI app; blocks until the window closes. Closing stops the
/// worker session and mount (PRD: no tray/background process).
pub fn run() {
    todo!("gpui Application::new; open window; poll event channel -> UiState.apply -> notify; text inputs (masked password), Connect, Start/Stop, log list with copy-selectable lines")
}
