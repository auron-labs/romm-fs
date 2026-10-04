//! Behavioral tests for `DownloadManager` (PRD R3, §6): one transfer per
//! ROM even under concurrency, atomic publish (never a visible `.part`),
//! cached reuse, failure without a ready entry, and retry. Fixture request
//! counts and source call counters are the proof.

use rommfs_core::cache::{CacheIndex, LiveState};
use rommfs_core::catalog::{RomEntry, RomKey, VersionKey};
use rommfs_core::download::{ContentSource, DownloadManager};
use rommfs_core::error::{Error, Result};
use rommfs_core::events::{channel, AppEvent};
use rommfs_core::romm::RommClient;
use rommfs_fixture::{FixtureServer, ResponseSpec};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

const BYTES: &[u8] = b"ROM-BYTES-\x00\x01\x02\x03-exact-content-0123456789";

fn key(rom_id: i64, file_id: i64) -> RomKey {
    RomKey {
        server_id: "http://fixture".into(),
        rom_id,
        file_id,
    }
}

fn entry(k: RomKey, file_name: &str, size: u64, version: Option<&str>) -> RomEntry {
    RomEntry {
        key: k,
        platform_dir: "nes".into(),
        file_name: file_name.into(),
        size,
        content_name: file_name.into(),
        version: version.map(|v| VersionKey(v.to_string())),
    }
}

/// Serves fixed bytes, counts fetches, optionally signals it entered
/// `fetch` then parks on a gate until released (for the single-flight proof).
struct BytesSource {
    bytes: Vec<u8>,
    calls: AtomicUsize,
    gate: Option<Arc<Barrier>>,
    entered: Option<std::sync::mpsc::Sender<()>>,
}

impl ContentSource for BytesSource {
    fn fetch(
        &self,
        _key: &RomKey,
        writer: &mut dyn std::io::Write,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<u64> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(tx) = &self.entered {
            let _ = tx.send(());
        }
        if let Some(gate) = &self.gate {
            gate.wait(); // hold the transfer open until the test releases it
        }
        writer.write_all(&self.bytes).map_err(Error::Io)?;
        progress(self.bytes.len() as u64, Some(self.bytes.len() as u64));
        Ok(self.bytes.len() as u64)
    }
}

/// Fails the first call, then serves bytes — the retry path.
struct FlakySource {
    bytes: Vec<u8>,
    calls: AtomicUsize,
    broken: AtomicBool,
}

impl ContentSource for FlakySource {
    fn fetch(
        &self,
        _key: &RomKey,
        writer: &mut dyn std::io::Write,
        _progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<u64> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.broken.swap(false, Ordering::SeqCst) {
            return Err(Error::Transport("connection reset".into()));
        }
        writer.write_all(&self.bytes).map_err(Error::Io)?;
        Ok(self.bytes.len() as u64)
    }
}

/// Writes fewer bytes than the catalogue size and reports success — the
/// manager must still detect the size mismatch.
struct ShortSource {
    bytes: Vec<u8>,
    calls: AtomicUsize,
}

impl ContentSource for ShortSource {
    fn fetch(
        &self,
        _key: &RomKey,
        writer: &mut dyn std::io::Write,
        _progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<u64> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        writer.write_all(&self.bytes).map_err(Error::Io)?;
        Ok(self.bytes.len() as u64)
    }
}

fn manager(
    source: Arc<dyn ContentSource>,
    dir: &std::path::Path,
    entries: &[RomEntry],
) -> (Arc<DownloadManager>, std::sync::mpsc::Receiver<AppEvent>) {
    let index = CacheIndex::open(dir).unwrap();
    let live = Arc::new(LiveState::default());
    let (sink, rx) = channel();
    let expected: HashMap<RomKey, u64> = entries.iter().map(|e| (e.key.clone(), e.size)).collect();
    let versions: HashMap<RomKey, Option<String>> = entries
        .iter()
        .map(|e| (e.key.clone(), e.version.as_ref().map(|v| v.0.clone())))
        .collect();
    (
        Arc::new(DownloadManager::new(
            index, live, source, sink, expected, versions,
        )),
        rx,
    )
}

fn drain(rx: &std::sync::mpsc::Receiver<AppEvent>) -> Vec<AppEvent> {
    let mut out = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        out.push(ev);
    }
    out
}

#[test]
fn ensure_ready_fetches_once_then_serves_the_cached_bin() {
    let dir = tempfile::tempdir().unwrap();
    let k = key(7, 70);
    let e = entry(k.clone(), "Game.nes", BYTES.len() as u64, Some("v1"));
    let source = Arc::new(BytesSource {
        bytes: BYTES.to_vec(),
        calls: AtomicUsize::new(0),
        gate: None,
        entered: None,
    });
    let (dm, _rx) = manager(source.clone(), dir.path(), std::slice::from_ref(&e));

    let path = dm.ensure_ready(&e).unwrap();
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    // The published file is byte-exact and no staging file remains.
    assert_eq!(std::fs::read(&path).unwrap(), BYTES);
    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|f| f.path().extension().is_some_and(|x| x == "part"))
        .collect();
    assert!(leftovers.is_empty(), ".part must never remain visible");

    // Second call: cached path, no new transfer.
    let again = dm.ensure_ready(&e).unwrap();
    assert_eq!(again, path);
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    assert!(dm.is_ready(&k));
}

#[test]
fn concurrent_callers_share_a_single_fetch() {
    let dir = tempfile::tempdir().unwrap();
    let k = key(7, 70);
    let e = entry(k.clone(), "Game.nes", BYTES.len() as u64, Some("v1"));
    const N: usize = 8;
    // `start` releases all callers into ensure_ready at once; `gate` holds
    // the one in-flight transfer open until the test is sure callers have
    // attached. Waiters park on the shared-flight condvar, not the gate —
    // only the leader and this thread trip it.
    let start = Arc::new(Barrier::new(N + 1));
    let gate = Arc::new(Barrier::new(2));
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let source = Arc::new(BytesSource {
        bytes: BYTES.to_vec(),
        calls: AtomicUsize::new(0),
        gate: Some(gate.clone()),
        entered: Some(entered_tx),
    });
    let (dm, _rx) = manager(source.clone(), dir.path(), std::slice::from_ref(&e));

    let mut handles = Vec::new();
    for _ in 0..N {
        let dm = Arc::clone(&dm);
        let e = e.clone();
        let start = Arc::clone(&start);
        handles.push(std::thread::spawn(move || {
            start.wait();
            dm.ensure_ready(&e)
        }));
    }
    start.wait();
    // Leader is inside the transfer; waiters attach while it is held open.
    entered_rx.recv().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));
    gate.wait();

    let mut paths = std::collections::HashSet::new();
    for h in handles {
        let p = h.join().unwrap().unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), BYTES);
        paths.insert(p);
    }
    assert_eq!(paths.len(), 1, "every caller shares the one cache file");
    assert_eq!(
        source.calls.load(Ordering::SeqCst),
        1,
        "N concurrent reads caused exactly one transfer"
    );
}

#[test]
fn failed_download_leaves_no_ready_entry_and_retry_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let k = key(7, 70);
    let e = entry(k.clone(), "Game.nes", BYTES.len() as u64, Some("v1"));
    let source = Arc::new(FlakySource {
        bytes: BYTES.to_vec(),
        calls: AtomicUsize::new(0),
        broken: AtomicBool::new(true),
    });
    let (dm, rx) = manager(source.clone(), dir.path(), std::slice::from_ref(&e));

    let err = dm.ensure_ready(&e).unwrap_err();
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    let _ = err;
    assert!(!dm.is_ready(&k));
    assert!(dm
        .index()
        .lock()
        .ready_if_current(&k, Some("v1"))
        .unwrap()
        .is_none());
    // Failure emitted the terminal event.
    let events = drain(&rx);
    assert!(events
        .iter()
        .any(|e| matches!(e, AppEvent::DownloadFailed { rom_id: 7, .. })));

    // Retry is a fresh transfer and lands Ready.
    let path = dm.ensure_ready(&e).unwrap();
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
    assert_eq!(std::fs::read(&path).unwrap(), BYTES);
    assert!(dm.is_ready(&k));
}

#[test]
fn short_write_is_reported_as_a_size_mismatch_not_ready() {
    let dir = tempfile::tempdir().unwrap();
    let k = key(7, 70);
    // Catalogue declares the full size; source only produces half.
    let short = &BYTES[..BYTES.len() / 2];
    let e = entry(k.clone(), "Game.nes", BYTES.len() as u64, Some("v1"));
    let source = Arc::new(ShortSource {
        bytes: short.to_vec(),
        calls: AtomicUsize::new(0),
    });
    let (dm, _rx) = manager(source.clone(), dir.path(), std::slice::from_ref(&e));

    let err = dm.ensure_ready(&e).unwrap_err();
    match err {
        Error::SizeMismatch { expected, received } => {
            assert_eq!(expected, BYTES.len() as u64);
            assert_eq!(received, short.len() as u64);
        }
        other => panic!("expected SizeMismatch, got {other:?}"),
    }
    assert!(!dm.is_ready(&k));
    // No partial bytes were published.
    assert!(!dm.index().lock().bin_path(&k).exists());
}

#[test]
fn oversized_response_is_rejected_and_never_published() {
    let dir = tempfile::tempdir().unwrap();
    let k = key(7, 70);
    let e = entry(k.clone(), "Game.nes", 3, Some("v1"));
    let source = Arc::new(BytesSource {
        bytes: b"too large".to_vec(),
        calls: AtomicUsize::new(0),
        gate: None,
        entered: None,
    });
    let (dm, _) = manager(source, dir.path(), std::slice::from_ref(&e));

    let err = dm.ensure_ready(&e).unwrap_err();
    assert!(matches!(
        err,
        Error::SizeMismatch {
            expected: 3,
            received: 9
        }
    ));
    assert!(!dm.index().lock().bin_path(&k).exists());
    assert!(dm
        .index()
        .lock()
        .ready_if_current(&k, Some("v1"))
        .unwrap()
        .is_none());
}

#[test]
fn missing_ready_file_is_downloaded_again() {
    let dir = tempfile::tempdir().unwrap();
    let k = key(7, 70);
    let e = entry(k.clone(), "Game.nes", BYTES.len() as u64, Some("v1"));
    let source = Arc::new(BytesSource {
        bytes: BYTES.to_vec(),
        calls: AtomicUsize::new(0),
        gate: None,
        entered: None,
    });
    let (dm, _) = manager(source.clone(), dir.path(), std::slice::from_ref(&e));

    let path = dm.ensure_ready(&e).unwrap();
    std::fs::remove_file(path).unwrap();
    let recovered_path = dm.ensure_ready(&e).unwrap();

    assert_eq!(std::fs::read(recovered_path).unwrap(), BYTES);
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
}

#[test]
fn real_client_fetches_through_the_content_source_impl() {
    // End-to-end: RommClient -> ContentSource -> DownloadManager, with the
    // fixture counter proving the second ensure_ready hit the cache.
    let fx = FixtureServer::start();
    fx.on(
        "GET",
        "/api/roms/",
        ResponseSpec::Bytes {
            status: 200,
            bytes: BYTES.to_vec(),
            truncate_at: None,
            stall_after_bytes: None,
        },
    );
    let client = RommClient::new(fx.url()).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let k = key(7, 70);
    let e = entry(k.clone(), "Game.nes", BYTES.len() as u64, Some("v1"));
    let (dm, _rx) = manager(Arc::new(client), dir.path(), std::slice::from_ref(&e));

    let path = dm.ensure_ready(&e).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), BYTES);
    dm.ensure_ready(&e).unwrap();
    assert_eq!(
        fx.count("GET", "/api/roms/"),
        1,
        "second ensure_ready must not request content again"
    );
}

#[test]
fn download_events_follow_the_real_transfer() {
    let dir = tempfile::tempdir().unwrap();
    let k = key(7, 70);
    let e = entry(k.clone(), "Game.nes", BYTES.len() as u64, Some("v1"));
    let source = Arc::new(BytesSource {
        bytes: BYTES.to_vec(),
        calls: AtomicUsize::new(0),
        gate: None,
        entered: None,
    });
    let (dm, rx) = manager(source, dir.path(), std::slice::from_ref(&e));
    dm.ensure_ready(&e).unwrap();

    let events = drain(&rx);
    assert!(events.iter().any(|ev| matches!(
        ev,
        AppEvent::DownloadStarted {
            rom_id: 7,
            total: Some(_),
            ..
        }
    )));
    assert!(events.iter().any(|ev| matches!(
        ev,
        AppEvent::DownloadProgress {
            rom_id: 7,
            received,
            ..
        } if *received == BYTES.len() as u64
    )));
    assert!(events
        .iter()
        .any(|ev| matches!(ev, AppEvent::DownloadFinished { rom_id: 7, .. })));
    // A cache hit emits no download lifecycle events.
    dm.ensure_ready(&e).unwrap();
    assert!(drain(&rx).is_empty());
}
