//! RomMFS core library: RomM client, catalogue, download/cache engine, and the
//! filesystem-facing facade consumed by the platform adapter and tests.
//!
//! The domain, HTTP, cache, and save-sync code is portable Rust testable against
//! `rommfs-fixture`. Remembered paired secrets use Windows Credential Manager;
//! non-Windows builds report that secure sign-in persistence is unavailable.
//! No GPU, GPUI, or native-mount code lives here.

pub mod cache;
pub mod catalogue;
pub mod catalogue_store;
pub mod connection_settings;
pub mod credential_store;
pub mod diagnostics;
pub mod download;
pub mod error;
pub mod events;
pub mod filesystem;
pub mod layout;
pub mod retrobat;
pub mod romm;
pub mod sanitize;
pub mod save_sync;
pub mod tree;

pub use error::{Error, Result};
