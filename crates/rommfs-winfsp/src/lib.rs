//! WinFsp mount adapter. Root validation remains portable for headless tests.
mod root;
pub use root::{check_mount_root, claim_mount_root, RootCheck};

#[cfg(windows)]
mod imp;
#[cfg(windows)]
pub use imp::WindowsMount;
