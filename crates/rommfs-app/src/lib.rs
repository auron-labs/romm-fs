//! rommfs-app internals as a lib so tests can drive `controller` headless
//! (GPUI window itself stays in main/window and is not required for tests).

pub mod controller;
mod desktop;
#[cfg(windows)]
mod icon;
pub mod problem;
mod save_sync_agent;
#[cfg(windows)]
mod save_sync_native;
#[cfg(feature = "ui")]
mod session;
mod trace_bridge;
#[cfg(feature = "ui")]
pub mod window;
