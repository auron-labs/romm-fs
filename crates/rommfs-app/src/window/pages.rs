//! Per-destination page bodies. The shell in `window.rs` dispatches to these
//! so each page can be developed independently. A page is a free function —
//! `render(&mut RommfsWindow, &mut Window, &mut Context<RommfsWindow>)` —
//! that may access the view's private fields and the shared helpers in
//! `crate::window` (they are visible to descendant modules).
pub mod activity;
pub mod overview;
pub mod save_review;
pub mod settings;
pub mod setup;
