//! RomMFS desktop app — one small GPUI window (PRD R5).
//! All logic lives in the lib target (`controller`, `window`).

// The GUI owns no console: without this the OS attaches a stray console window
// that renders as a titleless black window and swallows clicks.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    if let Some(code) = rommfs_app::retrobat_setup::windows::run_helper(std::env::args_os()) {
        std::process::exit(code);
    }
    rommfs_app::window::run();
}
