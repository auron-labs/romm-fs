//! Windows Cloud Files adapter. Ownership validation remains portable.
mod root;
pub use root::{check_mount_root, claim_mount_root, RootCheck};
pub use root::{publish_frontend_metadata, read_frontend_metadata};

#[cfg(windows)]
mod imp;
#[cfg(windows)]
mod security;
#[cfg(windows)]
pub use imp::WindowsMount;
#[cfg(windows)]
pub use imp::{check_prerequisites, WindowsHydratedRemover};
