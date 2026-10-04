//! RomM API client — verified contract: `.planning/API-CONTRACT.md`
//! (RomM 5.3.1). Uses `isahc` blocking HTTP; the download path enforces
//! connect + no-progress (low-speed) timeouts so large but progressing
//! transfers are never killed by a total-time budget (PRD R3).

pub mod client;
pub mod types;

pub use client::{Credentials, DownloadConfig, MetadataConfig, RommClient};
pub use types::*;
