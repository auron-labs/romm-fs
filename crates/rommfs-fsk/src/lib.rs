//! Windows `fsk` (ProjFS) adapter — the ONLY Windows-specific code.
//! Gates: whole crate is `cfg(windows)`; on other targets it's an empty lib.
//!
//! See `.planning/BACKEND-DECISION.md` for the verified fsk 0.0.9 behavior
//! this relies on:
//! - reads go through `Filesystem::read` (first read = download trigger);
//! - open/close arrive as `Operation::Notification` raw events with
//!   `PRJ_NOTIFY_*` bits merged into `flags`; mutation previews are vetoed;
//! - `namespace_context` captured during raw events feeds `PrjDeleteFile`
//!   for hydrated-copy eviction (never retain borrowed callback pointers).

#[cfg(windows)]
mod imp;

#[cfg(windows)]
pub use imp::*;

#[cfg(not(windows))]
compile_error!("rommfs-fsk is a Windows-only adapter; keep it out of portable builds");
