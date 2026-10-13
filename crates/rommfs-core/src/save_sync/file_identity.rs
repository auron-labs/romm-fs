//! Reversible remote identities stored in RomM's `slot`, not rewritten filenames.
use super::path::validate_windows_path_component;
use crate::catalogue::RomKey;
use crate::{Error, Result};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

pub const FILE_SLOT_PREFIX: &str = "rommfs-file-v2:";
pub const GENERIC_FILE_PROFILE: &str = "rommfs-files-v2";

/// Canonical server-portable relative path. The server's slot field is limited
/// to 255 characters; reject paths beyond that contract rather than truncate.
pub fn file_slot(path: &Path) -> Result<String> {
    let relative = portable_save_path(path)?;
    let slot = format!("{FILE_SLOT_PREFIX}{relative}");
    if slot.chars().count() > 255 {
        return Err(Error::Unsupported(
            "Save path exceeds RomM's 255-character slot limit; choose a shorter relative path"
                .into(),
        ));
    }
    Ok(slot)
}

pub fn path_from_file_slot(slot: &str) -> Result<PathBuf> {
    let relative = slot.strip_prefix(FILE_SLOT_PREFIX).ok_or_else(|| {
        Error::Unsupported("Remote save is not a RomMFS ordinary-file save".into())
    })?;
    let path = PathBuf::from(relative);
    if file_slot(&path)? != slot {
        return Err(Error::Unsupported(
            "Remote save path is not canonical".into(),
        ));
    }
    Ok(path)
}

pub fn portable_save_path(path: &Path) -> Result<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        let Component::Normal(value) = component else {
            return Err(Error::Unsupported(
                "Save path must be relative without traversal".into(),
            ));
        };
        let value = value
            .to_str()
            .ok_or_else(|| Error::Unsupported("Save path is not valid Unicode".into()))?;
        validate_windows_path_component(value).map_err(Error::Unsupported)?;
        parts.push(value);
    }
    if parts.is_empty() {
        return Err(Error::Unsupported("Save path is empty".into()));
    }
    Ok(parts.join("/"))
}

/// The journal's existing file-id column also carries ordinary save identities.
/// Negative IDs occupy a namespace disjoint from positive original-ROM files.
/// Callers must reject a hash collision against a different path (never merge).
pub fn save_file_key(server: &str, rom_id: i64, path: &Path) -> Result<RomKey> {
    if server.is_empty() || rom_id <= 0 {
        return Err(Error::Unsupported(
            "Save file needs a verified server and game".into(),
        ));
    }
    let normalized = portable_save_path(path)?.to_lowercase();
    let digest = Sha256::digest(normalized.as_bytes());
    let mut bytes = [0; 8];
    bytes.copy_from_slice(&digest[..8]);
    let id = (i64::from_le_bytes(bytes) & i64::MAX).max(1);
    Ok(RomKey {
        server_id: server.into(),
        rom_id,
        file_id: -id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_extensionless_unicode_paths_reconstruct_without_a_journal() {
        for name in [
            "switch/0100ABCD/user/0",
            "gb/Game.rtc",
            "game/日本語/save.dat",
            "empty",
            "a/b/c/d/save",
        ] {
            let path = Path::new(name);
            let slot = file_slot(path).unwrap();
            assert_eq!(path_from_file_slot(&slot).unwrap(), path);
            assert!(save_file_key("server", 5, path).unwrap().file_id < 0);
        }
        assert_eq!(
            save_file_key("server", 5, Path::new("gb/Game.rtc")).unwrap(),
            save_file_key("server", 5, Path::new("GB/game.RTC")).unwrap()
        );
        assert_ne!(
            save_file_key("server", 5, Path::new("a/save")).unwrap(),
            save_file_key("server", 5, Path::new("b/save")).unwrap()
        );
    }
    #[test]
    fn hostile_and_unrepresentable_remote_paths_are_rejected() {
        for name in [
            "",
            "/absolute",
            "../escape",
            "a/../escape",
            "a//b",
            "a/./b",
            "a/CON",
            "a:stream",
            "a\\b",
            "a/b.",
        ] {
            assert!(
                path_from_file_slot(&format!("{FILE_SLOT_PREFIX}{name}")).is_err(),
                "{name}"
            );
        }
        assert!(file_slot(Path::new(&format!("a/{}", "x".repeat(256)))).is_err());
    }
}
