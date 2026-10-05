//! RomMFS core library: RomM client, catalogue, download/cache engine, and the
//! filesystem-facing facade consumed by the platform adapter and tests.
//!
//! No GPU, GPUI, or native-mount code lives here. Everything in this crate is
//! portable Rust testable against the `rommfs-fixture` HTTP server.

pub mod cache;
pub mod catalog;
pub mod download;
pub mod error;
pub mod events;
pub mod fscore;
pub mod romm;
pub mod sanitize;
pub mod save_sync;
pub mod tree;

pub use error::{Error, Result};
