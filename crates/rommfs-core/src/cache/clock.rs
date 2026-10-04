//! Clock abstraction so tests inject time (PRD: "tests inject a clock/
//! shorter duration"). The default threshold is a simple setting, not a UI.

use std::sync::atomic::{AtomicU64, Ordering};

pub const DEFAULT_EVICTION_THRESHOLD_SECS: u64 = 14 * 24 * 60 * 60; // 14 days

/// Seconds since Unix epoch.
pub fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub trait Clock: Send + Sync {
    fn unix_secs(&self) -> u64;
}

#[derive(Default)]
pub struct SystemClock;
impl Clock for SystemClock {
    fn unix_secs(&self) -> u64 {
        now_unix_secs()
    }
}

/// Test clock; advance manually.
pub struct FakeClock {
    secs: AtomicU64,
}
impl FakeClock {
    pub fn new(start: u64) -> Self {
        Self { secs: AtomicU64::new(start) }
    }
    pub fn advance(&self, secs: u64) {
        self.secs.fetch_add(secs, Ordering::SeqCst);
    }
    pub fn set(&self, secs: u64) {
        self.secs.store(secs, Ordering::SeqCst);
    }
}
impl Clock for FakeClock {
    fn unix_secs(&self) -> u64 {
        self.secs.load(Ordering::SeqCst)
    }
}
