use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

pub const DEFAULT_DEBOUNCE_SECS: u32 = 5;

/// Consent is scoped to the authenticated identity, server, installation,
/// and effective save root. The account ID is supplied by a verified auth
/// contract; it is never inferred from a username.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveSyncScope {
    pub server_id: String,
    pub account_id: String,
    pub installation_root: PathBuf,
    pub effective_saves_root: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsentSettings {
    pub enabled: bool,
    pub debounce_secs: u32,
}

impl Default for ConsentSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            debounce_secs: DEFAULT_DEBOUNCE_SECS,
        }
    }
}

/// SQLite settings are stored separately from ROM cache data. No credentials
/// or tokens are represented in this schema.
pub struct SaveSyncSettingsStore {
    connection: rusqlite::Connection,
}

impl SaveSyncSettingsStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let connection = rusqlite::Connection::open(path).map_err(sqlite)?;
        let has_effective_root = {
            let mut statement = connection
                .prepare("PRAGMA table_info(save_sync_consent)")
                .map_err(sqlite)?;
            let columns = statement
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(sqlite)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(sqlite)?;
            columns
                .iter()
                .any(|column| column == "effective_saves_root")
        };
        if !has_effective_root {
            // P1 consent did not include a resolved effective saves path.
            // Discard those rows instead of silently carrying consent over to
            // a possibly redirected location.
            connection
                .execute_batch("DROP TABLE IF EXISTS save_sync_consent;")
                .map_err(sqlite)?;
        }
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS save_sync_consent (
                    server_id TEXT NOT NULL,
                    account_id TEXT NOT NULL,
                    installation_root TEXT NOT NULL,
                    effective_saves_root TEXT NOT NULL,
                    enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
                    debounce_secs INTEGER NOT NULL DEFAULT 5 CHECK (debounce_secs >= 0),
                    PRIMARY KEY (server_id, account_id, installation_root, effective_saves_root)
                );
                CREATE TABLE IF NOT EXISTS save_sync_selection (
                    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                    installation_root TEXT NOT NULL
                );",
            )
            .map_err(sqlite)?;
        Ok(Self { connection })
    }

    /// Missing rows mean no consent, with the product's default debounce.
    /// Empty identity fields are rejected rather than creating a dangerously
    /// broad preference scope.
    pub fn load(&self, scope: &SaveSyncScope) -> Result<ConsentSettings> {
        validate_scope(scope)?;
        let row = self
            .connection
            .query_row(
                "SELECT enabled, debounce_secs FROM save_sync_consent
                 WHERE server_id = ?1 AND account_id = ?2 AND installation_root = ?3
                   AND effective_saves_root = ?4",
                rusqlite::params![
                    scope.server_id,
                    scope.account_id,
                    windows_path_key(&scope.installation_root),
                    windows_path_key(&scope.effective_saves_root),
                ],
                |row| Ok((row.get::<_, bool>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
            .map_err(sqlite)?;
        match row {
            Some((enabled, debounce_secs)) => Ok(ConsentSettings {
                enabled,
                debounce_secs: u32::try_from(debounce_secs).unwrap_or(DEFAULT_DEBOUNCE_SECS),
            }),
            None => Ok(ConsentSettings::default()),
        }
    }

    pub fn save(&self, scope: &SaveSyncScope, settings: ConsentSettings) -> Result<()> {
        validate_scope(scope)?;
        self.connection
            .execute(
                "INSERT INTO save_sync_consent
                    (server_id, account_id, installation_root, effective_saves_root, enabled, debounce_secs)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (server_id, account_id, installation_root, effective_saves_root) DO UPDATE SET
                    enabled = excluded.enabled,
                    debounce_secs = excluded.debounce_secs",
                rusqlite::params![
                    scope.server_id,
                    scope.account_id,
                    windows_path_key(&scope.installation_root),
                    windows_path_key(&scope.effective_saves_root),
                    settings.enabled,
                    settings.debounce_secs,
                ],
            )
            .map_err(sqlite)?;
        Ok(())
    }

    /// Remember only an explicitly chosen install path. This is independent
    /// of consent, so a missing prior selection can pause safely at next start
    /// instead of silently switching to a different discovered installation.
    pub fn save_selected_installation(&self, root: &Path) -> Result<()> {
        let root = root.to_str().ok_or_else(|| {
            Error::Unsupported("RetroBat installation path is not valid Unicode".into())
        })?;
        if root.trim().is_empty() {
            return Err(Error::Unsupported(
                "RetroBat installation path is empty".into(),
            ));
        }
        self.connection
            .execute(
                "INSERT INTO save_sync_selection (singleton, installation_root) VALUES (1, ?1)
                 ON CONFLICT (singleton) DO UPDATE SET installation_root = excluded.installation_root",
                [root],
            )
            .map_err(sqlite)?;
        Ok(())
    }

    pub fn selected_installation(&self) -> Result<Option<PathBuf>> {
        self.connection
            .query_row(
                "SELECT installation_root FROM save_sync_selection WHERE singleton = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map(|root| root.map(PathBuf::from))
            .map_err(sqlite)
    }
}

pub(crate) fn validate_scope(scope: &SaveSyncScope) -> Result<()> {
    if scope.server_id.trim().is_empty()
        || scope.account_id.trim().is_empty()
        || scope.installation_root.as_os_str().is_empty()
        || scope.effective_saves_root.as_os_str().is_empty()
    {
        return Err(Error::Unsupported(
            "save-sync consent requires a server, authenticated account, installation, and effective save root".into(),
        ));
    }
    Ok(())
}

/// Windows paths are case-insensitive and slash-insensitive. This key is used
/// only for preference scope identity; the original path remains the displayed
/// and selected path.
pub(crate) fn windows_path_key(path: &Path) -> String {
    let path = path.to_string_lossy().replace('/', "\\");
    let (prefix, remainder) = if path.starts_with("\\\\") {
        ("\\\\", path.trim_start_matches('\\'))
    } else if path.as_bytes().get(1) == Some(&b':') && path.as_bytes().get(2) == Some(&(b'\\')) {
        let drive = path[..1].to_ascii_lowercase();
        let prefix = format!("{drive}:\\");
        return normalize_windows_components(&prefix, &path[3..]);
    } else if path.starts_with('\\') {
        ("\\", path.trim_start_matches('\\'))
    } else {
        ("", path.as_str())
    };
    normalize_windows_components(prefix, remainder)
}

fn normalize_windows_components(prefix: &str, remainder: &str) -> String {
    let mut components: Vec<String> = Vec::new();
    for component in remainder
        .split('\\')
        .filter(|part| !part.is_empty() && *part != ".")
    {
        if component == ".." {
            if components.last().is_some_and(|part| part != "..") {
                components.pop();
            } else if prefix.is_empty() {
                components.push("..".into());
            }
        } else {
            components.push(component.to_lowercase());
        }
    }
    format!("{prefix}{}", components.join("\\"))
}

fn sqlite(error: rusqlite::Error) -> Error {
    Error::Io(std::io::Error::other(error))
}

use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(server_id: &str, account_id: &str, root: &str) -> SaveSyncScope {
        SaveSyncScope {
            server_id: server_id.into(),
            account_id: account_id.into(),
            installation_root: root.into(),
            effective_saves_root: PathBuf::from(r"D:\RetroBat\saves"),
        }
    }

    #[test]
    fn consent_is_off_by_default_and_isolated_by_server_account_and_install() {
        let dir = tempfile::tempdir().unwrap();
        let store = SaveSyncSettingsStore::open(dir.path().join("settings.db")).unwrap();
        let first = scope("https://one", "account-a", r"D:\RetroBat");
        assert_eq!(store.load(&first).unwrap(), ConsentSettings::default());

        store
            .save(
                &first,
                ConsentSettings {
                    enabled: true,
                    debounce_secs: 11,
                },
            )
            .unwrap();
        assert_eq!(
            store
                .load(&scope("https://one", "account-a", "d:/retrobat/"))
                .unwrap(),
            ConsentSettings {
                enabled: true,
                debounce_secs: 11,
            }
        );
        for other in [
            scope("https://two", "account-a", r"D:\RetroBat"),
            scope("https://one", "account-b", r"D:\RetroBat"),
            scope("https://one", "account-a", r"E:\RetroBat"),
            SaveSyncScope {
                effective_saves_root: PathBuf::from(r"D:\RetroBat\saves-redirected"),
                ..first.clone()
            },
        ] {
            assert_eq!(store.load(&other).unwrap(), ConsentSettings::default());
        }

        store
            .save(
                &first,
                ConsentSettings {
                    enabled: false,
                    debounce_secs: 11,
                },
            )
            .unwrap();
        assert!(!store.load(&first).unwrap().enabled);
    }

    #[test]
    fn missing_identity_cannot_create_broad_consent_and_selection_survives_missing_path() {
        let dir = tempfile::tempdir().unwrap();
        let store = SaveSyncSettingsStore::open(dir.path().join("settings.db")).unwrap();
        let anonymous = scope("https://one", "", r"D:\RetroBat");
        assert!(store
            .save(
                &anonymous,
                ConsentSettings {
                    enabled: true,
                    debounce_secs: 5,
                }
            )
            .is_err());

        let selected = dir.path().join("removed-install");
        store.save_selected_installation(&selected).unwrap();
        std::fs::create_dir(&selected).unwrap();
        std::fs::remove_dir(&selected).unwrap();
        assert_eq!(store.selected_installation().unwrap(), Some(selected));
    }

    #[test]
    fn consent_without_effective_root_is_not_migrated_or_reused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.db");
        let legacy = rusqlite::Connection::open(&path).unwrap();
        legacy
            .execute_batch(
                "CREATE TABLE save_sync_consent (
                    server_id TEXT NOT NULL, account_id TEXT NOT NULL,
                    installation_root TEXT NOT NULL, enabled INTEGER NOT NULL,
                    debounce_secs INTEGER NOT NULL,
                    PRIMARY KEY (server_id, account_id, installation_root));
                 INSERT INTO save_sync_consent VALUES ('https://one', 'account-a',
                    'd:\\retrobat', 1, 5);",
            )
            .unwrap();
        drop(legacy);

        let store = SaveSyncSettingsStore::open(&path).unwrap();
        assert_eq!(
            store
                .load(&scope("https://one", "account-a", r"D:\RetroBat"))
                .unwrap(),
            ConsentSettings::default()
        );
    }
}
