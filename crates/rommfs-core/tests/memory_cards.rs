//! Combined memory-card discovery, archive and consent regression tests.
//! The lifecycle goes through the real client and the fixture's stateful
//! memory-card routes; nothing here asserts implementation structure.

use rommfs_core::catalogue::build_catalogue;
use rommfs_core::romm::RommClient;
#[cfg(unix)]
use rommfs_core::save_sync::read_local_card_hash;
use rommfs_core::save_sync::{
    conflict_sibling, discover_card_folders, extract_card_archive, hash_local_card,
    plan_card_action, remote_card_restore_target, write_card_archive, CardBaseline, CardDecision,
    MemoryCardEmulator, RemoteCardVersion, SaveFileRules, SaveSyncScope, SaveSyncSettingsStore,
    MEMORY_CARD_MAX_BYTES,
};
use rommfs_fixture::FixtureServer;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn scope(root: &Path, account: &str) -> SaveSyncScope {
    SaveSyncScope {
        server_id: "https://romm.example".into(),
        account_id: account.into(),
        installation_root: root.to_path_buf(),
        effective_saves_root: root.to_path_buf(),
    }
}

#[test]
fn discovers_duckstation_and_pcsx2_file_and_folder_cards() {
    let saves = tempfile::tempdir().unwrap();
    let duck = saves
        .path()
        .join(MemoryCardEmulator::Duckstation.relative_dir());
    let pcsx2 = saves.path().join(MemoryCardEmulator::Pcsx2.relative_dir());
    std::fs::create_dir_all(&duck).unwrap();
    std::fs::create_dir_all(pcsx2.join("FolderCard")).unwrap();
    std::fs::write(duck.join("Mcd001.mcd"), b"card one").unwrap();
    std::fs::write(duck.join("notes.txt"), b"ignored").unwrap();
    // A conflict sibling and a staging artifact are never cards.
    std::fs::write(duck.join("Mcd001.rommfs-conflict.mcd"), b"remote copy").unwrap();
    std::fs::write(duck.join(".rommfs-card-staging.zip"), b"staging").unwrap();
    std::fs::write(pcsx2.join("FolderCard/Mcd001.ps2"), b"folder data").unwrap();
    std::fs::write(pcsx2.join("Mcd002.ps2"), b"card two").unwrap();

    let folders = discover_card_folders(saves.path()).unwrap();
    assert_eq!(folders.len(), 2);

    let duck = &folders[0];
    assert_eq!(duck.emulator, MemoryCardEmulator::Duckstation);
    assert_eq!(duck.label, "PS1 memory cards (DuckStation)");
    assert_eq!(duck.cards.len(), 1);
    assert_eq!(duck.cards[0].name, "Mcd001");
    assert!(!duck.cards[0].folder);

    let pcsx2 = &folders[1];
    assert_eq!(pcsx2.emulator, MemoryCardEmulator::Pcsx2);
    assert_eq!(pcsx2.cards.len(), 2);
    let folder_card = pcsx2
        .cards
        .iter()
        .find(|card| card.folder)
        .expect("folder card");
    assert_eq!(folder_card.name, "FolderCard");
    assert!(pcsx2
        .cards
        .iter()
        .any(|card| card.name == "Mcd002" && !card.folder));
}

#[test]
fn file_and_folder_cards_round_trip_through_the_romm_routes() {
    let fixture = FixtureServer::start();
    fixture.use_romm_memory_card_store(42);
    let client = RommClient::new(fixture.url()).unwrap();
    let active = Arc::new(AtomicBool::new(true));
    let work = tempfile::tempdir().unwrap();

    // File card (DuckStation).
    let file_card = work.path().join("Mcd001.mcd");
    std::fs::write(&file_card, vec![0x5a; 96 * 1024]).unwrap();
    upload_card_round_trip(
        &client,
        &active,
        work.path(),
        "Mcd001",
        MemoryCardEmulator::Duckstation.name(),
        &file_card,
        false,
    );

    // Folder card (PCSX2): multiple entries must survive.
    let folder_card = work.path().join("FolderCard");
    std::fs::create_dir_all(folder_card.join("nested")).unwrap();
    std::fs::write(folder_card.join("Mcd001.ps2"), b"main").unwrap();
    std::fs::write(folder_card.join("nested/extra.bin"), b"extra").unwrap();
    upload_card_round_trip(
        &client,
        &active,
        work.path(),
        "FolderCard",
        MemoryCardEmulator::Pcsx2.name(),
        &folder_card,
        true,
    );

    // The hash is stable across an unchanged re-read and moves on a write.
    let before = hash_local_card(&file_card).unwrap();
    assert_eq!(hash_local_card(&file_card).unwrap(), before);
    std::fs::write(&file_card, vec![0x00; 96 * 1024]).unwrap();
    assert_ne!(hash_local_card(&file_card).unwrap(), before);

    // The real client created exactly two cards under their emulators.
    let cards = fixture.memory_cards();
    assert_eq!(cards.len(), 2);
    assert!(cards
        .iter()
        .any(|card| card.emulator == "duckstation" && card.name == "Mcd001"));
    assert!(cards
        .iter()
        .any(|card| card.emulator == "pcsx2" && card.name == "FolderCard"));
}

fn upload_card_round_trip(
    client: &RommClient,
    active: &Arc<AtomicBool>,
    work: &Path,
    name: &str,
    emulator: &str,
    card_path: &Path,
    folder: bool,
) {
    let remote = client.create_memory_card(name, emulator).unwrap();
    let archive = work.join(format!("{name}.card.zip"));
    {
        let file = std::fs::File::create(&archive).unwrap();
        write_card_archive(card_path, file).unwrap();
    }
    let version = client
        .upload_memory_card_version(remote.id, &format!("{name}.card.zip"), &archive, active)
        .unwrap();
    let versions = client.memory_card_versions(remote.id).unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].id, version.id);

    let download = work.join(format!("{name}.downloaded.zip"));
    {
        let mut file = std::fs::File::create(&download).unwrap();
        client
            .download_memory_card_version(
                version.id,
                versions[0].file_size_bytes,
                &mut file,
                active,
            )
            .unwrap();
    }
    let restored = work.join(format!("{name}.restored"));
    if folder {
        extract_card_archive(&download, &restored, true).unwrap();
        assert_eq!(std::fs::read(restored.join("Mcd001.ps2")).unwrap(), b"main");
        assert_eq!(
            std::fs::read(restored.join("nested/extra.bin")).unwrap(),
            b"extra"
        );
    } else {
        let destination = restored.join(format!("{name}.mcd"));
        extract_card_archive(&download, &destination, false).unwrap();
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            std::fs::read(card_path).unwrap()
        );
    }
}

#[test]
fn archive_rejects_unsafe_entries_and_the_transfer_limit() {
    let work = tempfile::tempdir().unwrap();
    let destination = work.path().join("out.mcd");

    // A traversal entry is refused and nothing is written.
    let evil = work.path().join("evil.zip");
    {
        let file = std::fs::File::create(&evil).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("../evil.mcd", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"payload").unwrap();
        zip.finish().unwrap();
    }
    assert!(extract_card_archive(&evil, &destination, false).is_err());
    assert!(!destination.exists());

    // A card larger than the 256 MB transfer limit is refused before it is read.
    let oversized = work.path().join("Oversized.mcd");
    std::fs::File::create(&oversized)
        .unwrap()
        .set_len(MEMORY_CARD_MAX_BYTES + 1)
        .unwrap();
    assert!(write_card_archive(&oversized, std::io::Cursor::new(Vec::new())).is_err());
}

#[test]
fn card_consent_is_off_by_default_and_scoped_per_card_folder() {
    let directory = tempfile::tempdir().unwrap();
    let store = SaveSyncSettingsStore::open(directory.path().join("settings.db")).unwrap();
    let saves = directory.path().join("saves");
    let scoped = scope(&saves, "42");
    let duck_dir = PathBuf::from(MemoryCardEmulator::Duckstation.relative_dir());
    let pcsx2_dir = PathBuf::from(MemoryCardEmulator::Pcsx2.relative_dir());

    assert!(!store
        .load_card_folder(&scoped, MemoryCardEmulator::Duckstation.name(), &duck_dir)
        .unwrap());
    store
        .save_card_folder(
            &scoped,
            MemoryCardEmulator::Duckstation.name(),
            &duck_dir,
            true,
        )
        .unwrap();
    assert!(store
        .load_card_folder(&scoped, MemoryCardEmulator::Duckstation.name(), &duck_dir)
        .unwrap());

    // A different card folder, account, or server never inherits consent.
    assert!(!store
        .load_card_folder(&scoped, MemoryCardEmulator::Pcsx2.name(), &pcsx2_dir)
        .unwrap());
    assert!(!store
        .load_card_folder(
            &scope(&saves, "7"),
            MemoryCardEmulator::Duckstation.name(),
            &duck_dir
        )
        .unwrap());
    let mut other_server = scoped.clone();
    other_server.server_id = "https://other.example".into();
    assert!(!store
        .load_card_folder(
            &other_server,
            MemoryCardEmulator::Duckstation.name(),
            &duck_dir
        )
        .unwrap());
}

#[test]
fn ordinary_mapping_still_excludes_memory_card_paths() {
    let saves = tempfile::tempdir().unwrap();
    let memcards = saves.path().join(MemoryCardEmulator::Pcsx2.relative_dir());
    std::fs::create_dir_all(&memcards).unwrap();
    std::fs::write(memcards.join("Mcd001.ps2"), b"card").unwrap();

    let catalogue = build_catalogue("server", &[], &[], |_| {}).expect("empty catalogue");
    let rules = SaveFileRules::from_catalogue(&catalogue, &[], BTreeMap::new()).unwrap();
    let plan = rules.scan(saves.path()).unwrap();

    assert!(plan.report.mappings.is_empty());
    let excluded = plan
        .unmatched
        .iter()
        .find(|file| file.path.to_string_lossy().contains("memcards"))
        .expect("memory card path is reported unmatched");
    assert!(!excluded.mappable);
}

#[test]
fn create_refuses_an_invalid_emulator_name_before_the_request() {
    let fixture = FixtureServer::start();
    fixture.use_romm_memory_card_store(42);
    let client = RommClient::new(fixture.url()).unwrap();
    assert!(client.create_memory_card("Mcd001", "bad/name").is_err());
    assert_eq!(fixture.request_count("POST", "/api/memory-cards"), 0);
}

#[test]
fn empty_folder_is_discovered_and_remote_targets_and_conflicts_are_versioned() {
    let saves = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(saves.path().join(MemoryCardEmulator::Pcsx2.relative_dir())).unwrap();
    let folders = discover_card_folders(saves.path()).unwrap();
    assert_eq!(folders.len(), 1);
    assert!(folders[0].cards.is_empty());

    let dir = tempfile::tempdir().unwrap();
    let file_card = dir.path().join("Mcd001.mcd");
    std::fs::write(&file_card, b"card").unwrap();
    let mut cursor = std::io::Cursor::new(Vec::new());
    write_card_archive(&file_card, &mut cursor).unwrap();
    let file_archive = dir.path().join("file.zip");
    std::fs::write(&file_archive, cursor.into_inner()).unwrap();
    let restore =
        remote_card_restore_target(&file_archive, MemoryCardEmulator::Duckstation, "Mcd001")
            .unwrap();
    assert!(!restore.folder);
    assert_eq!(
        restore.relative_path,
        PathBuf::from("psx/duckstation/memcards/Mcd001.mcd")
    );

    let folder_card = dir.path().join("FolderCard");
    std::fs::create_dir_all(&folder_card).unwrap();
    std::fs::write(folder_card.join("a.ps2"), b"a").unwrap();
    std::fs::write(folder_card.join("b.bin"), b"b").unwrap();
    let mut cursor = std::io::Cursor::new(Vec::new());
    write_card_archive(&folder_card, &mut cursor).unwrap();
    let folder_archive = dir.path().join("folder.zip");
    std::fs::write(&folder_archive, cursor.into_inner()).unwrap();
    let restore =
        remote_card_restore_target(&folder_archive, MemoryCardEmulator::Pcsx2, "FolderCard")
            .unwrap();
    assert!(restore.folder);
    assert_eq!(
        restore.relative_path,
        PathBuf::from("ps2/pcsx2/memcards/FolderCard")
    );

    // A server-supplied card name is validated before it becomes a path.
    assert!(
        remote_card_restore_target(&file_archive, MemoryCardEmulator::Duckstation, "../evil")
            .is_err()
    );

    // The conflict sibling is version-specific.
    assert_eq!(
        conflict_sibling(Path::new("psx/duckstation/memcards/Mcd001.mcd"), false, 7).unwrap(),
        PathBuf::from("psx/duckstation/memcards/Mcd001.rommfs-conflict-v7.mcd")
    );
    assert_eq!(
        conflict_sibling(Path::new("ps2/pcsx2/memcards/FolderCard"), true, 9).unwrap(),
        PathBuf::from("ps2/pcsx2/memcards/FolderCard.rommfs-conflict-v9")
    );
}

#[test]
fn single_file_pcsx2_folder_restores_as_a_folder_and_duckstation_rejects_folders() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("FolderCard");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("readme.txt"), b"data").unwrap();
    let mut cursor = std::io::Cursor::new(Vec::new());
    write_card_archive(&folder, &mut cursor).unwrap();
    let archive = dir.path().join("folder.zip");
    std::fs::write(&archive, cursor.into_inner()).unwrap();

    // A single arbitrary file is a folder card for PCSX2, never a `.ps2` file.
    let restore =
        remote_card_restore_target(&archive, MemoryCardEmulator::Pcsx2, "FolderCard").unwrap();
    assert!(restore.folder);
    assert_eq!(
        restore.relative_path,
        PathBuf::from("ps2/pcsx2/memcards/FolderCard")
    );
    // DuckStation has no folder cards.
    assert!(
        remote_card_restore_target(&archive, MemoryCardEmulator::Duckstation, "Mcd001").is_err()
    );
}

#[test]
fn archive_extraction_rejects_windows_unsafe_paths_and_validates_all_entries() {
    let work = tempfile::tempdir().unwrap();
    let destination = work.path().join("out");

    for name in [
        "..\\..\\evil",
        "CON",
        "Mcd001.mcd:stream",
        "Mcd001.mcd\u{1}",
    ] {
        let archive = work.path().join("bad.zip");
        {
            let file = std::fs::File::create(&archive).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"payload").unwrap();
            zip.finish().unwrap();
        }
        assert!(
            extract_card_archive(&archive, &destination, true).is_err(),
            "accepted unsafe entry {name}"
        );
        assert!(!destination.exists());
    }

    // A single-file archive that also carries a symlink entry is rejected even
    // though only the file would be written.
    let archive = work.path().join("symlink.zip");
    {
        let file = std::fs::File::create(&archive).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("Mcd001.mcd", options).unwrap();
        zip.write_all(b"card").unwrap();
        zip.start_file("link.mcd", options.unix_permissions(0o120777))
            .unwrap();
        zip.write_all(b"target").unwrap();
        zip.finish().unwrap();
    }
    assert!(extract_card_archive(&archive, &destination, false).is_err());
}

#[cfg(unix)]
#[test]
fn unreadable_or_linked_cards_are_rejected_not_treated_as_absent() {
    use std::os::unix::fs::symlink;

    let saves = tempfile::tempdir().unwrap();
    let memcards = saves
        .path()
        .join(MemoryCardEmulator::Duckstation.relative_dir());
    std::fs::create_dir_all(&memcards).unwrap();
    // A symlink at the card path is an error, not "no card".
    let link = memcards.join("Mcd001.mcd");
    symlink(saves.path().join("outside.mcd"), &link).unwrap();
    assert!(hash_local_card(&link).is_err());
    assert!(read_local_card_hash(&link).is_err());

    // A folder card containing a symlink is rejected rather than silently
    // truncated and then replaced.
    let folder = memcards.join("FolderCard");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("a.bin"), b"a").unwrap();
    symlink(folder.join("a.bin"), folder.join("link.bin")).unwrap();
    assert!(hash_local_card(&folder).is_err());
    assert!(write_card_archive(&folder, &mut std::io::Cursor::new(Vec::new())).is_err());
}

#[test]
fn card_decisions_cover_baseline_changes_and_the_running_emulator() {
    let baseline = CardBaseline {
        version_id: Some(7),
        local_hash: Some("sha256:local".into()),
        remote_hash: Some("remote".into()),
    };
    let remote_v7 = RemoteCardVersion {
        id: 7,
        content_hash: Some("remote".into()),
    };
    let remote_v8 = RemoteCardVersion {
        id: 8,
        content_hash: Some("newer".into()),
    };

    // Emulator running wins over every other difference.
    assert_eq!(
        plan_card_action(Some("sha256:changed"), &baseline, Some(&remote_v8), true),
        CardDecision::EmulatorRunning
    );
    assert_eq!(
        plan_card_action(Some("sha256:local"), &baseline, Some(&remote_v7), false),
        CardDecision::Settled
    );
    // Local changed, remote unchanged: upload.
    assert_eq!(
        plan_card_action(Some("sha256:changed"), &baseline, Some(&remote_v7), false),
        CardDecision::Upload
    );
    // Remote newer, local unchanged: download.
    assert_eq!(
        plan_card_action(Some("sha256:local"), &baseline, Some(&remote_v8), false),
        CardDecision::Download { version_id: 8 }
    );
    // Both changed: conflict, keeping the local card.
    assert_eq!(
        plan_card_action(Some("sha256:changed"), &baseline, Some(&remote_v8), false),
        CardDecision::Conflict { version_id: 8 }
    );
    // First sync (no baseline, no remote): upload a present local card.
    let empty = CardBaseline::default();
    assert_eq!(
        plan_card_action(Some("sha256:new"), &empty, None, false),
        CardDecision::Upload
    );
    // A machine with no local card adopts the remote.
    assert_eq!(
        plan_card_action(None, &empty, Some(&remote_v8), false),
        CardDecision::Download { version_id: 8 }
    );
}
