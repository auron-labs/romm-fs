//! Windows Cloud Files adapter. Ownership validation remains portable.
mod root;
pub use root::{check_mount_root, claim_mount_root, RootCheck};
pub use root::{foreign_entries, remove_foreign_links, UnownedEntries, UnownedEntry, UnownedKind};
pub use root::{publish_frontend_metadata, read_frontend_metadata};

#[cfg(windows)]
mod security;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::WindowsMount;
#[cfg(windows)]
pub use windows::{check_prerequisites, WindowsHydratedRemover};
