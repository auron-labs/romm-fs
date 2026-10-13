//! rommfs-app internals as a lib so tests can drive `controller` headless
//! (GPUI window itself stays in main/window and is not required for tests).

pub mod controller;
mod desktop;
#[cfg(windows)]
mod icon;
pub mod problem;
#[cfg(windows)]
mod retrobat_discovery_windows;
mod save_sync_agent;
#[cfg(feature = "ui")]
mod session;
mod trace_bridge;
#[cfg(feature = "ui")]
pub mod window;
