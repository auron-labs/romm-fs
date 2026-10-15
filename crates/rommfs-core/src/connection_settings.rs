//! Ordinary connection preferences; secrets belong only in Credential Manager.
use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum IntegrationMode {
    #[default]
    Generic,
    RetroBat,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionPreferences {
    pub server: String,
    pub account_id: i64,
    pub device_identifier: String,
    pub mount_root: Option<PathBuf>,
    #[serde(default)]
    pub integration: IntegrationMode,
    pub retrobat_root: Option<PathBuf>,
    #[serde(default)]
    pub start_at_login: bool,
    #[serde(default)]
    pub save_root: Option<PathBuf>,
    /// Hide RomM's copy of a game the user already has locally. On by default;
    /// the user turns it off in the RetroBat tab.
    #[serde(default = "hide_owned_by_default")]
    pub hide_owned_copies: bool,
}

fn hide_owned_by_default() -> bool {
    true
}

pub struct ConnectionSettings(Connection);
impl ConnectionSettings {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = Connection::open(path).map_err(sqlite)?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS connection_preferences (id INTEGER PRIMARY KEY CHECK (id=1), payload TEXT NOT NULL);").map_err(sqlite)?;
        Ok(Self(db))
    }
    pub fn load(&self) -> Result<Option<ConnectionPreferences>> {
        let payload: Option<String> = self
            .0
            .query_row(
                "SELECT payload FROM connection_preferences WHERE id=1",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(sqlite)?;
        payload
            .map(|s| {
                serde_json::from_str(&s)
                    .map_err(|e| Error::InvalidCatalogue(format!("connection preferences: {e}")))
            })
            .transpose()
    }
    pub fn save(&self, preferences: &ConnectionPreferences) -> Result<()> {
        let json = serde_json::to_string(preferences)
            .map_err(|e| Error::InvalidCatalogue(e.to_string()))?;
        self.0.execute("INSERT INTO connection_preferences VALUES (1, ?) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload", [json]).map_err(sqlite)?;
        Ok(())
    }
    pub fn clear(&self) -> Result<()> {
        self.0
            .execute("DELETE FROM connection_preferences", [])
            .map_err(sqlite)?;
        Ok(())
    }
}
fn sqlite(e: rusqlite::Error) -> Error {
    Error::Io(std::io::Error::other(e))
}
