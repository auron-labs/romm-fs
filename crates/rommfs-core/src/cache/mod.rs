//! Persistent cache index (SQLite) + injected-clock eviction policy (PRD R4).
//!
//! - Completed entries + last-use records survive restarts; incomplete
//!   app-owned downloads are removed on recovery.
//! - Cache identity = `RomKey` (server + rom + file). `version` mismatches
//!   invalidate previously completed content at catalogue load.
//! - Eviction: entries idle past the threshold are removed — never active
//!   downloads or in-use files; check-then-delete races are closed by
//!   per-entry guards; failures are deferred to a later sweep, never forced.

pub mod clock;
pub mod index;
pub mod policy;

pub use clock::{now_unix_secs, Clock, FakeClock, SystemClock};
pub use index::{CacheIndex, CacheRecord, EntryState};
pub use policy::{ActiveGuard, EvictionOutcome, Evictor, LiveState};

/// What must be removed when an entry is evicted, from the adapter's view.
pub trait HydratedRemover: Send + Sync {
    /// Remove the platform-managed hydrated copy for this relative path
    /// (e.g. `nes/Example Game.nes`). Return false/err to defer the whole
    /// eviction — the private copy must be kept too.
    fn remove_hydrated(&self, rel_path: &str) -> std::io::Result<()>;
}

/// No-op remover for portable tests / non-Windows runs.
pub struct NoopHydratedRemover;
impl HydratedRemover for NoopHydratedRemover {
    fn remove_hydrated(&self, _rel_path: &str) -> std::io::Result<()> {
        Ok(())
    }
}
