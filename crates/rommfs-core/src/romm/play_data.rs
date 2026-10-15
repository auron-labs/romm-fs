//! Play data (favourites, last played, play count/time, hidden) at the verified
//! RomM contract revision. Favourites live in a per-user collection with
//! `is_favorite`; RomM has no play-count field, so count and time are derived
//! from play sessions (`duration_ms`) and last played is a `RomUser` timestamp.
//! See `docs/contracts/romm-api.md`.

use super::client::RommClient;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// The four scopes play-data sync needs. Each is requested separately so a
/// limited account keeps working for everything else.
pub const PLAY_DATA_SCOPES: [&str; 4] = [
    "roms.user.read",
    "roms.user.write",
    "collections.read",
    "collections.write",
];

/// One `RomUser` row, the subset that travels in the ROM catalogue.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RomUserDto {
    #[serde(default)]
    pub last_played: Option<String>,
    #[serde(default)]
    pub hidden: bool,
}

/// A named collection. The user's favourites are the one with `is_favorite`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct CollectionDto {
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub is_favorite: bool,
    #[serde(default)]
    pub rom_ids: Vec<i64>,
}

/// One recorded play session, as returned by `GET /api/play-sessions`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct PlaySessionDto {
    pub id: i64,
    #[serde(default)]
    pub rom_id: Option<i64>,
    #[serde(default)]
    pub start_time: String,
    #[serde(default)]
    pub end_time: String,
    #[serde(default)]
    pub duration_ms: i64,
}

/// A session this device recorded locally, uploaded to RomM.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlaySessionUpload {
    pub rom_id: i64,
    pub start_time: String,
    pub end_time: String,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PlaySessionIngest {
    #[serde(default)]
    created_count: u64,
    #[serde(default)]
    skipped_count: u64,
}

/// What RomM reports for one game, derived from collections, sessions and
/// `RomUser`. Kept in core so the merge rules stay portable. `hidden` is not
/// part of play-data sync; the "hide RomM copies" feature owns it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemotePlayData {
    pub favorite: bool,
    pub play_count: u64,
    pub play_time_ms: u64,
    pub last_played: Option<i64>,
}

impl RemotePlayData {
    /// Fold one play session into the running totals for its ROM.
    pub fn add_session(&mut self, session: &PlaySessionDto) {
        if session.duration_ms > 0 {
            self.play_count += 1;
            self.play_time_ms += session.duration_ms as u64;
        }
    }
}

impl RommClient {
    /// `GET /api/collections` (`collections.read`).
    pub fn collections(&self) -> Result<Vec<CollectionDto>> {
        self.send_json("GET", "/api/collections", None, None)
    }

    /// Create the user's favourites collection (`collections.write`). RomM
    /// keeps one per user; the caller looks it up first.
    pub fn create_favorite_collection(&self, name: &str) -> Result<CollectionDto> {
        let body = format!("name={}&is_favorite=true", super::client::form_encode(name));
        self.send_json(
            "POST",
            "/api/collections",
            Some(&body),
            Some("application/x-www-form-urlencoded"),
        )
    }

    /// Atomically add ROMs to a collection (`collections.write`).
    pub fn add_collection_roms(&self, collection_id: i64, rom_ids: &[i64]) -> Result<()> {
        let payload = serde_json::json!({ "rom_ids": rom_ids });
        let _: CollectionDto = self.send_json(
            "POST",
            &format!("/api/collections/{collection_id}/roms"),
            Some(&payload.to_string()),
            Some("application/json"),
        )?;
        Ok(())
    }

    /// Atomically remove ROMs from a collection (`collections.write`).
    pub fn remove_collection_roms(&self, collection_id: i64, rom_ids: &[i64]) -> Result<()> {
        let payload = serde_json::json!({ "rom_ids": rom_ids });
        let _: CollectionDto = self.send_json(
            "DELETE",
            &format!("/api/collections/{collection_id}/roms"),
            Some(&payload.to_string()),
            Some("application/json"),
        )?;
        Ok(())
    }

    /// Every play session for the account (`roms.user.read`). Passing an
    /// early `start_after` makes RomM return the whole history in one page
    /// instead of the default 50.
    pub fn all_play_sessions(&self) -> Result<Vec<PlaySessionDto>> {
        self.send_json(
            "GET",
            "/api/play-sessions?start_after=1970-01-01T00:00:00Z",
            None,
            None,
        )
    }

    /// Upload locally recorded sessions. RomM dedups on `(rom_id, device, start
    /// time)`, and applying a session also advances `last_played`.
    pub fn ingest_play_sessions(&self, sessions: &[PlaySessionUpload]) -> Result<(u64, u64)> {
        if sessions.is_empty() {
            return Ok((0, 0));
        }
        let payload = serde_json::json!({ "sessions": sessions });
        let result: PlaySessionIngest = self.send_json(
            "POST",
            "/api/play-sessions",
            Some(&payload.to_string()),
            Some("application/json"),
        )?;
        Ok((result.created_count, result.skipped_count))
    }

    /// Set or clear `RomUser.hidden` (`roms.user.write`).
    // `set_rom_hidden` is not used by play-data sync: the "hide RomM copies"
    // feature owns `hidden`. The route is kept for that feature.
    #[allow(dead_code)]
    pub fn set_rom_hidden(&self, rom_id: i64, hidden: bool) -> Result<()> {
        let payload = serde_json::json!({ "hidden": hidden });
        let _: serde_json::Value = self.send_json(
            "PUT",
            &format!("/api/roms/{rom_id}/props"),
            Some(&payload.to_string()),
            Some("application/json"),
        )?;
        Ok(())
    }
}

/// Which play-data scopes a verified session holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlayDataPermissions {
    pub user_read: bool,
    pub user_write: bool,
    pub collections_read: bool,
    pub collections_write: bool,
}

impl PlayDataPermissions {
    pub fn from_scopes(scopes: &[String]) -> Self {
        let has = |scope: &str| scopes.iter().any(|held| held == scope);
        Self {
            user_read: has("roms.user.read"),
            user_write: has("roms.user.write"),
            collections_read: has("collections.read"),
            collections_write: has("collections.write"),
        }
    }

    /// Reading needs the user-data and collection read scopes; writing needs
    /// all four, since a pull without the write scopes cannot be pushed back.
    pub fn can_sync(&self) -> bool {
        self.user_read && self.user_write && self.collections_read && self.collections_write
    }
}

impl RommClient {
    /// Verify the account and read the scopes already carried by the session.
    pub fn play_data_permissions(&self) -> Result<PlayDataPermissions> {
        let identity = self.save_sync_identity()?.ok_or_else(|| {
            Error::Auth("the server did not return the authenticated account".into())
        })?;
        Ok(PlayDataPermissions::from_scopes(&identity.scopes))
    }
}
