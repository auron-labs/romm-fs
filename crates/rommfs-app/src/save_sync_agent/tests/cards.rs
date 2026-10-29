// Memory-card transfer tests through the fixture RomM server.
use super::*;
use rommfs_core::save_sync::{discover_card_folders, write_card_archive, MemoryCardJournal};

fn card_case(
    directory: &Path,
    fixture: &FixtureServer,
) -> (AgentCase, MemoryCardConfig, MemoryCardJournal) {
    let case = case(directory, fixture, false);
    let journal_path = directory.join("cards-journal.db");
    let journal = MemoryCardJournal::open(&journal_path, case.scope.clone()).unwrap();
    let folders = discover_card_folders(&case.scope.effective_saves_root).unwrap();
    let config = MemoryCardConfig {
        scope: case.scope.clone(),
        folders,
        identity: SaveSyncIdentity {
            account_id: ACCOUNT_ID,
            scopes: vec!["assets.read".into(), "assets.write".into()],
        },
        client: authenticated_client(fixture),
        journal_path,
        debounce_secs: 0,
        session_id: 1,
    };
    (case, config, journal)
}

#[test]
fn a_local_card_uploads_its_archive_to_the_server() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_memory_card_store(ACCOUNT_ID);
    let memcards = directory
        .path()
        .join("RetroBat/saves/psx/duckstation/memcards");
    fs::create_dir_all(&memcards).unwrap();
    fs::write(memcards.join("Mcd001.mcd"), vec![0xabu8; 128 * 1024]).unwrap();

    let (_case, config, journal) = card_case(directory.path(), &fixture);
    let (sink, _events) = channel();
    let report = reconcile_once(
        &config,
        &journal,
        false,
        &Arc::new(AtomicBool::new(true)),
        &sink,
    );

    // On Windows the staging archive must already be closed when the upload
    // re-opens it by path; a still-open handle is a sharing violation.
    assert!(report.problem.is_none(), "{:?}", report.problem);
    assert_eq!(fixture.memory_cards().len(), 1);
    let versions = fixture.memory_card_versions();
    assert_eq!(versions.len(), 1, "card version must reach the server");
    let mut written = std::io::Cursor::new(Vec::new());
    write_card_archive(
        &directory
            .path()
            .join("RetroBat/saves/psx/duckstation/memcards/Mcd001.mcd"),
        &mut written,
    )
    .unwrap();
    assert_eq!(versions[0].bytes, written.into_inner());
}

#[test]
fn a_remote_only_card_is_restored_into_an_empty_card_folder() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = FixtureServer::start();
    fixture.use_romm_memory_card_store(ACCOUNT_ID);
    let saves_root = directory.path().join("RetroBat/saves");
    fs::create_dir_all(saves_root.join("psx/duckstation/memcards")).unwrap();

    // Build the remote archive from a staging card so the format round-trips.
    let staging = tempfile::tempdir().unwrap();
    let source_card = staging.path().join("RemoteCard.mcd");
    fs::write(&source_card, vec![0x5au8; 64 * 1024]).unwrap();
    let mut archive = std::io::Cursor::new(Vec::new());
    write_card_archive(&source_card, &mut archive).unwrap();
    let archive = archive.into_inner();

    let (_case, config, journal) = card_case(directory.path(), &fixture);
    let remote = config
        .client
        .create_memory_card("RemoteCard", "duckstation")
        .unwrap();
    fixture.seed_memory_card_version(remote.id, archive);
    assert!(config
        .folders
        .iter()
        .any(|folder| folder.relative_dir.ends_with("psx/duckstation/memcards")));

    let (sink, _events) = channel();
    let report = reconcile_once(
        &config,
        &journal,
        false,
        &Arc::new(AtomicBool::new(true)),
        &sink,
    );

    assert!(report.problem.is_none(), "{:?}", report.problem);
    let restored = saves_root.join("psx/duckstation/memcards/RemoteCard.mcd");
    assert_eq!(fs::read(&restored).unwrap(), vec![0x5au8; 64 * 1024]);
}
