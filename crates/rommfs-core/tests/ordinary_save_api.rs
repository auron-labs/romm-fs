use rommfs_core::romm::RommClient;
use rommfs_core::save_sync::{file_slot, path_from_file_slot};
use rommfs_fixture::{FixtureServer, ResponseSpec};
use std::path::Path;
use std::sync::atomic::AtomicBool;

#[test]
fn clean_client_reconstructs_nested_companions_extensionless_large_and_empty_files() {
    let fx = FixtureServer::start();
    fx.on(
        "POST",
        "/api/saves",
        ResponseSpec::RomMSaveUpload {
            status: 200,
            user_id: 7,
        },
    );
    fx.on("GET", "/api/saves?", ResponseSpec::RomMSaveInventory);
    fx.on("GET", "/api/saves/", ResponseSpec::RomMSaveContent);
    let source = tempfile::tempdir().unwrap();
    let destination = tempfile::tempdir().unwrap();
    let client = RommClient::new(fx.url()).unwrap();
    let active = std::sync::Arc::new(AtomicBool::new(true));
    let cases = [
        ("gb/Game.srm", b"ordinary bytes".to_vec()),
        ("gb/Game.rtc", b"companion".to_vec()),
        ("switch/0100ABCD/user/0", vec![0x93; 2 * 1024 * 1024 + 17]),
        ("switch/0100ABCD/user/empty", Vec::new()),
    ];
    for (name, bytes) in &cases {
        let path = source.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        let revision = uuid::Uuid::new_v4();
        let remote = client
            .upload_save_file(
                42,
                &file_slot(Path::new(name)).unwrap(),
                &format!("rommfs-{revision}.bin"),
                &path,
                &active,
            )
            .unwrap();
        assert_ne!(remote.file_name, format!("rommfs-{revision}.bin"));
    }
    // No shared local mappings, cache, or journal: only returned server slots.
    let second = RommClient::new(fx.url()).unwrap();
    let inventory = second.save_inventory(42, "").unwrap();
    assert_eq!(inventory.len(), cases.len());
    for remote in inventory {
        let relative = path_from_file_slot(remote.slot.as_deref().unwrap()).unwrap();
        let path = destination.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut file = std::fs::File::create(path).unwrap();
        second
            .download_save_to(remote.id, remote.file_size_bytes, &mut file, &active)
            .unwrap();
    }
    for (name, bytes) in cases {
        assert_eq!(std::fs::read(destination.path().join(name)).unwrap(), bytes);
    }
}

#[test]
fn uploads_can_be_cancelled_while_waiting_for_headers_or_response_bodies() {
    use rommfs_fixture::FixtureBodyBarrier;
    use std::sync::{atomic::Ordering, mpsc, Arc};
    use std::time::Duration;
    for status in [None, Some(200), Some(503)] {
        let fx = FixtureServer::start();
        let barrier = FixtureBodyBarrier::new();
        let spec = match status {
            None => ResponseSpec::HeldRomMSaveUpload {
                user_id: 42,
                barrier: barrier.clone(),
            },
            Some(status) => ResponseSpec::HeldBytes {
                status,
                bytes: b"response held open".to_vec(),
                barrier: barrier.clone(),
            },
        };
        fx.on("POST", "/api/saves", spec);
        let source = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(source.path(), b"save bytes").unwrap();
        let client = RommClient::new(fx.url()).unwrap();
        let path = source.path().to_path_buf();
        let active = Arc::new(AtomicBool::new(true));
        let worker_active = active.clone();
        let (tx, rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = client.upload_save_file(
                42,
                &file_slot(Path::new("gb/Game.srm")).unwrap(),
                "revision.bin",
                &path,
                &worker_active,
            );
            tx.send(result).unwrap();
        });
        assert!(barrier.wait_until_blocked(Duration::from_secs(5)));
        active.store(false, Ordering::Release);
        let result = rx.recv_timeout(Duration::from_secs(2));
        // Release after observing the result, so a failed assertion also cleans up the fixture.
        barrier.release();
        let failure = result
            .expect("upload did not stop until its HTTP response was released")
            .unwrap_err();
        assert!(matches!(failure.error, rommfs_core::Error::Cancelled));
        worker.join().unwrap();
    }
}

#[test]
fn downloads_can_be_cancelled_during_a_blocked_body_read() {
    use rommfs_fixture::FixtureBodyBarrier;
    use std::sync::{atomic::Ordering, mpsc, Arc};
    use std::time::Duration;
    let fx = FixtureServer::start();
    let barrier = FixtureBodyBarrier::new();
    fx.on(
        "GET",
        "/api/saves/1/content",
        ResponseSpec::HeldBytes {
            status: 200,
            bytes: b"save".to_vec(),
            barrier: barrier.clone(),
        },
    );
    let client = RommClient::new(fx.url()).unwrap();
    let active = Arc::new(AtomicBool::new(true));
    let worker_active = active.clone();
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut destination = Vec::new();
        let result = client.download_save_to(1, 4, &mut destination, &worker_active);
        tx.send((result, destination)).unwrap();
    });
    assert!(barrier.wait_until_blocked(Duration::from_secs(5)));
    active.store(false, Ordering::Release);
    let result = rx.recv_timeout(Duration::from_secs(2));
    barrier.release();
    let (result, destination) = result.expect("blocked download did not stop");
    assert!(matches!(
        result.unwrap_err().error,
        rommfs_core::Error::Cancelled
    ));
    assert!(destination.is_empty());
    worker.join().unwrap();
}
