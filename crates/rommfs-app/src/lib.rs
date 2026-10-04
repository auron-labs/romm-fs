//! rommfs-app internals as a lib so tests can drive `controller` headless
//! (GPUI window itself stays in main/window and is not required for tests).

pub mod controller;
#[cfg(feature = "ui")]
pub mod window;
