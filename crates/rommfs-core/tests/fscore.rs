//! Behavioral tests for the `RommFs` facade (PRD R2/R3/R4, §6): listing and
//! stat never download, the first content read triggers one transfer with
//! byte-exact results, version changes force a refetch, failures surface
//! without a ready entry, and evicted data downloads again while the tree
//! entry stays visible.

use rommfs_core::cache::{CacheIndex, Evictor, FakeClock, LiveState, NoopHydratedRemover};
use rommfs_core::catalog::{build_catalogue, Catalogue, RomKey};
use rommfs_core::download::{ContentSource, DownloadManager};
use rommfs_core::error::{Error, Result};
use rommfs_core::fscore::RommFs;
use rommfs_core::romm::{PlatformDto, RomDto, RomFileDto};
use rommfs_core::tree::{EntryKind, RommTree, ROOT_INODE};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

const BYTES: &[u8] = b"ROM-BYTES-\x00\x01\x02\x03-exact-content-0123456789";
const BYTES_V2: &[u8] = b"ROM-BYTES-V2-\x04\x05\x06-changed-bytes-987654321";
const DAY: u64 = 24 * 60 * 60;
const T0: u64 = 1_000_000;

/// Two platforms (one empty) so root enumeration has 2 children and
/// continuation-cookie paging is exercised with a page size of 1.
fn platforms() -> Vec<PlatformDto> {
    vec![
        PlatformDto {
            id: 1,
            slug: "nes".into(),
            fs_slug: "nes".into(),
            name: "Nintendo Entertainment System".into(),
            custom_name: None,
            rom_count: 1,
        },
        PlatformDto {
            id: 2,
            slug: "snes".into(),
            fs_slug: "snes".into(),
            name: "Super NES".into(),
            custom_name: None,
            rom_count: 0,
        },
    ]
}

fn rom(rom_id: i64, file_id: i64, name: &str, size: u64, sha1: &str) -> RomDto {
    RomDto {
        id: rom_id,
        platform_fs_slug: "nes".into(),
        platform_slug: "nes".into(),
        fs_name: name.into(),
        fs_size_bytes: size,
        has_simple_single_file: true,
        has_nested_single_file: false,
        has_multiple_files: false,
        missing_from_fs: false,
        is_physical: true,
        updated_at: "2026-10-01T00:00:00".into(),
        files: vec![RomFileDto {
            id: file_id,
            file_name: name.into(),
            file_size_bytes: size,
            last_modified: None,
            crc_hash: None,
            md5_hash: None,
            sha1_hash: Some(sha1.into()),
            is_top_level: true,
        }],
    }
}

fn catalogue(roms: Vec<RomDto>) -> Catalogue {
    build_catalogue("http://fixture", &platforms(), &roms, |_| {}).unwrap()
}

/// Counts fetches and serves swappable bytes so a refetch is provable by
/// both the counter and the content.
struct BytesSource {
    bytes: RwLock<Vec<u8>>,
    calls: AtomicUsize,
}

impl BytesSource {
    fn new(bytes: &[u8]) -> Self {
        Self {
            bytes: RwLock::new(bytes.to_vec()),
            calls: AtomicUsize::new(0),
        }
    }
    fn count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl ContentSource for BytesSource {
    fn fetch(
        &self,
        _key: &RomKey,
        writer: &mut dyn std::io::Write,
        _progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<u64> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let bytes = self.bytes.read().unwrap().clone();
        writer.write_all(&bytes).map_err(Error::Io)?;
        Ok(bytes.len() as u64)
    }
}

/// Always fails with the stored error.
struct BrokenSource {
    calls: AtomicUsize,
}

impl BrokenSource {
    fn count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl ContentSource for BrokenSource {
    fn fetch(
        &self,
        _key: &RomKey,
        _writer: &mut dyn std::io::Write,
        _progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<u64> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(Error::Truncated {
            expected: BYTES.len() as u64,
            received: 3,
        })
    }
}

struct Rig {
    dir: tempfile::TempDir,
    fs: RommFs,
    live: Arc<LiveState>,
    clock: Arc<FakeClock>,
    inode: u64,
}

impl Rig {
    /// The catalogue key behind the fixture inode — identity is derived
    /// from server + rom + file ids, not the visible name.
    fn key(&self) -> RomKey {
        RomKey {
            server_id: "http://fixture".into(),
            rom_id: 7,
            file_id: 70,
        }
    }
}

/// Build the whole core stack over one catalogue + source, in one cache dir.
fn build_fs(
    dir: &Path,
    cat: Catalogue,
    source: Arc<dyn ContentSource>,
    live: Arc<LiveState>,
    clock: Arc<FakeClock>,
) -> RommFs {
    let index = CacheIndex::open(dir).unwrap();
    let expected: HashMap<RomKey, u64> = cat
        .entries
        .iter()
        .map(|e| (e.key.clone(), e.size))
        .collect();
    let versions: HashMap<RomKey, Option<String>> = cat
        .entries
        .iter()
        .map(|e| (e.key.clone(), e.version.as_ref().map(|v| v.0.clone())))
        .collect();
    let (sink, _rx) = rommfs_core::events::channel();
    let downloads = Arc::new(DownloadManager::new(
        index,
        Arc::clone(&live),
        source,
        sink,
        expected,
        versions,
    ));
    let evictor = Evictor::new(14 * DAY, Arc::clone(&live), Arc::new(NoopHydratedRemover));
    let tree = RommTree::new(cat);
    RommFs::new(tree, downloads, evictor, clock)
}

fn rig(source: Arc<dyn ContentSource>, sha1: &str) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let cat = catalogue(vec![rom(7, 70, "Game.nes", BYTES.len() as u64, sha1)]);
    let live = Arc::new(LiveState::default());
    let clock = Arc::new(FakeClock::new(T0));
    let fs = build_fs(dir.path(), cat, source, Arc::clone(&live), clock.clone());
    let inode = fs.inode_for_path(&["nes", "Game.nes"]).unwrap();
    Rig {
        dir,
        fs,
        live,
        clock,
        inode,
    }
}

#[test]
fn listing_lookup_and_stat_perform_zero_fetches() {
    let source = Arc::new(BytesSource::new(BYTES));
    let rig = rig(source.clone(), "v1");

    // Root enumeration with a page size of 1 — continuation cookies must
    // not lose or duplicate entries (PRD R2).
    let mut names = Vec::new();
    let (mut page, mut cookie, mut eof) = rig.fs.read_dir(ROOT_INODE, 0, 1).unwrap();
    names.extend(page.drain(..).map(|e| e.name));
    while !eof {
        let (next, next_cookie, done) = rig.fs.read_dir(ROOT_INODE, cookie, 1).unwrap();
        cookie = next_cookie;
        eof = done;
        names.extend(next.into_iter().map(|e| e.name));
    }
    names.sort();
    assert_eq!(names, ["nes", "snes"]);

    let nes = page_inode(&rig, "nes");
    let (files, _, _) = rig.fs.read_dir(nes, 0, 64).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "Game.nes");
    assert_eq!(files[0].kind, EntryKind::File);
    assert_eq!(files[0].inode, rig.inode);

    let meta = rig.fs.lookup(nes, "game.nes").unwrap(); // case-insensitive
    assert_eq!(meta.size, BYTES.len() as u64);
    let meta2 = rig.fs.metadata(rig.inode).unwrap();
    assert_eq!(meta2.size, BYTES.len() as u64);
    assert_eq!(rig.fs.path_of(rig.inode).as_deref(), Some("nes/Game.nes"));

    assert_eq!(source.count(), 0, "metadata must never request ROM content");
}

fn page_inode(rig: &Rig, name: &str) -> u64 {
    rig.fs.lookup(ROOT_INODE, name).unwrap().inode
}

#[test]
fn first_read_downloads_once_then_cache_serves() {
    let source = Arc::new(BytesSource::new(BYTES));
    let rig = rig(source.clone(), "v1");

    let mut buf = vec![0u8; BYTES.len()];
    let n = rig.fs.read_at(rig.inode, 0, &mut buf).unwrap();
    assert_eq!(n, BYTES.len());
    assert_eq!(buf, BYTES);
    assert_eq!(source.count(), 1);

    // Second read comes from the cache — no new transfer.
    let mut buf2 = vec![0u8; BYTES.len()];
    assert_eq!(
        rig.fs.read_at(rig.inode, 0, &mut buf2).unwrap(),
        BYTES.len()
    );
    assert_eq!(buf2, BYTES);
    assert_eq!(source.count(), 1);

    // Non-sequential read at an offset.
    let mut mid = [0u8; 8];
    let n = rig.fs.read_at(rig.inode, 10, &mut mid).unwrap();
    assert_eq!(n, 8);
    assert_eq!(mid, BYTES[10..18]);
    assert_eq!(source.count(), 1);

    // Short read at EOF: clamped to file size, still no transfer.
    let mut tail = [0u8; 16];
    let n = rig
        .fs
        .read_at(rig.inode, BYTES.len() as u64 - 4, &mut tail)
        .unwrap();
    assert_eq!(n, 4);
    assert_eq!(tail[..4], BYTES[BYTES.len() - 4..]);
    assert_eq!(source.count(), 1);
}

#[test]
fn zero_length_and_eof_reads_do_not_download() {
    let source = Arc::new(BytesSource::new(BYTES));
    let rig = rig(source.clone(), "v1");
    let size = BYTES.len() as u64;

    assert_eq!(rig.fs.read_at(rig.inode, 0, &mut []).unwrap(), 0);
    assert_eq!(rig.fs.read_at(rig.inode, size, &mut [0u8; 8]).unwrap(), 0);
    assert_eq!(
        rig.fs.read_at(rig.inode, size + 99, &mut [0u8; 8]).unwrap(),
        0
    );
    assert_eq!(source.count(), 0);
}

#[test]
fn catalogue_version_bump_refetches_and_serves_new_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let source = Arc::new(BytesSource::new(BYTES));
    let live = Arc::new(LiveState::default());
    let clock = Arc::new(FakeClock::new(T0));

    // Mount session 1: catalogue reports version sha1:v1.
    let cat1 = catalogue(vec![rom(7, 70, "Game.nes", BYTES.len() as u64, "v1")]);
    let fs1 = build_fs(
        dir.path(),
        cat1,
        source.clone(),
        Arc::clone(&live),
        clock.clone(),
    );
    let inode1 = fs1.inode_for_path(&["nes", "Game.nes"]).unwrap();
    let mut buf = vec![0u8; BYTES.len()];
    fs1.read_at(inode1, 0, &mut buf).unwrap();
    assert_eq!(buf, BYTES);
    assert_eq!(source.count(), 1);
    drop(fs1);

    // Session 2 (stop/start reload): same ROM, new content version + bytes.
    *source.bytes.write().unwrap() = BYTES_V2.to_vec();
    let cat2 = catalogue(vec![rom(7, 70, "Game.nes", BYTES_V2.len() as u64, "v2")]);
    let fs2 = build_fs(dir.path(), cat2, source.clone(), live, clock);
    let inode2 = fs2.inode_for_path(&["nes", "Game.nes"]).unwrap();
    let mut buf2 = vec![0u8; BYTES_V2.len()];
    fs2.read_at(inode2, 0, &mut buf2).unwrap();
    assert_eq!(buf2, BYTES_V2);
    assert_eq!(
        source.count(),
        2,
        "a known version change must invalidate the cached bytes"
    );
}

#[test]
fn truncated_transfer_surfaces_an_error_and_no_ready_entry() {
    let source = Arc::new(BrokenSource {
        calls: AtomicUsize::new(0),
    });
    let rig = rig(source.clone(), "v1");

    let mut buf = [0u8; 16];
    let err = rig.fs.read_at(rig.inode, 0, &mut buf).unwrap_err();
    assert!(matches!(err, Error::Truncated { .. }), "got {err:?}");
    assert_eq!(source.count(), 1);
    // The failed entry never becomes servable — verified against the
    // durable index (a fresh handle, as a restart would see it).
    let check = CacheIndex::open(rig.dir.path()).unwrap();
    assert!(check
        .ready_if_current(&rig.key(), Some("sha1:v1"))
        .unwrap()
        .is_none());
}

#[test]
fn evicted_entry_stays_listed_and_redownloads_on_read() {
    let source = Arc::new(BytesSource::new(BYTES));
    let rig = rig(source.clone(), "v1");

    let mut buf = vec![0u8; BYTES.len()];
    rig.fs.read_at(rig.inode, 0, &mut buf).unwrap();
    assert_eq!(source.count(), 1);

    // 15 idle days: the sweep evicts; the ROM is still listed.
    rig.clock.advance(15 * DAY);
    let outcome = rig.fs.evict_stale().unwrap();
    assert_eq!(outcome.evicted.len(), 1);
    let nes = page_inode(&rig, "nes");
    let (files, _, _) = rig.fs.read_dir(nes, 0, 64).unwrap();
    assert_eq!(files.len(), 1, "eviction must not hide the ROM entry");

    // Next read fetches again and returns correct bytes.
    let mut buf2 = vec![0u8; BYTES.len()];
    let n = rig.fs.read_at(rig.inode, 0, &mut buf2).unwrap();
    assert_eq!(n, BYTES.len());
    assert_eq!(buf2, BYTES);
    assert_eq!(source.count(), 2);
}

#[test]
fn an_open_file_survives_a_sweep_until_close() {
    let source = Arc::new(BytesSource::new(BYTES));
    let rig = rig(source.clone(), "v1");

    let mut buf = vec![0u8; BYTES.len()];
    rig.fs.read_at(rig.inode, 0, &mut buf).unwrap();

    let guard = rig.fs.note_open(rig.inode).expect("ROM inode opens");
    assert!(rig.live.active_keys().contains(&rig.key()));
    rig.clock.advance(15 * DAY);
    let outcome = rig.fs.evict_stale().unwrap();
    assert!(outcome.evicted.is_empty());
    assert_eq!(outcome.retained_active.len(), 1);

    drop(guard);
    assert!(rig.live.active_keys().is_empty());
    let outcome = rig.fs.evict_stale().unwrap();
    assert_eq!(outcome.evicted.len(), 1);
}
