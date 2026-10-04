//! Behavioral tests for the eviction policy (PRD R4, §6): an injected clock
//! drives the 14-day inactivity threshold; active use and failed cleanup are
//! never force-deleted; both the hydrated copy and the private `.bin` go.

use rommfs_core::cache::{CacheIndex, Evictor, FakeClock, HydratedRemover, LiveState};
use rommfs_core::catalog::RomKey;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const DAY: u64 = 24 * 60 * 60;
const THRESHOLD: u64 = 14 * DAY;
const T0: u64 = 1_000_000;

fn key(rom_id: i64, file_id: i64) -> RomKey {
    RomKey {
        server_id: "http://fixture".into(),
        rom_id,
        file_id,
    }
}

/// Records the relative paths it was asked to remove; can be told to fail
/// so the deferral path is observable.
struct RecordingRemover {
    calls: Mutex<Vec<String>>,
    fail: AtomicBool,
}

impl RecordingRemover {
    fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            fail: AtomicBool::new(false),
        }
    }
    fn removed(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl HydratedRemover for RecordingRemover {
    fn remove_hydrated(&self, rel_path: &str) -> std::io::Result<()> {
        self.calls.lock().unwrap().push(rel_path.to_string());
        if self.fail.load(Ordering::SeqCst) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "busy",
            ));
        }
        Ok(())
    }
}

struct Rig {
    _dir: tempfile::TempDir,
    index: CacheIndex,
    live: Arc<LiveState>,
    clock: Arc<FakeClock>,
    remover: Arc<RecordingRemover>,
    evictor: Evictor,
    key: RomKey,
    bin: std::path::PathBuf,
}

/// A ready entry last used at `T0`, with a real `.bin` on disk.
fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut index = CacheIndex::open(dir.path()).unwrap();
    let key = key(7, 70);
    let bin = index.bin_path(&key);
    std::fs::write(&bin, b"cached-rom-bytes").unwrap();
    index
        .mark_ready(&key, 16, Some("v1"), "nes/Game.nes")
        .unwrap();
    index.touch(&key, T0).unwrap();

    let live = Arc::new(LiveState::default());
    let clock = Arc::new(FakeClock::new(T0));
    let remover = Arc::new(RecordingRemover::new());
    let evictor = Evictor::new(THRESHOLD, Arc::clone(&live), remover.clone());
    Rig {
        _dir: dir,
        index,
        live,
        clock,
        remover,
        evictor,
        key,
        bin,
    }
}

fn sweep(rig: &mut Rig) -> rommfs_core::cache::EvictionOutcome {
    let key = rig.key.clone();
    rig.evictor
        .sweep(
            &mut rig.index,
            rig.clock.as_ref(),
            move |k| {
                if *k == key {
                    Some("nes/Game.nes".to_string())
                } else {
                    None
                }
            },
            |p| std::fs::remove_file(p),
        )
        .unwrap()
}

#[test]
fn idle_entry_is_evicted_and_both_copies_removed() {
    let mut rig = rig();
    rig.clock.advance(THRESHOLD + DAY);

    let outcome = sweep(&mut rig);
    assert_eq!(outcome.evicted, vec![rig.key.clone()]);
    assert!(outcome.retained_active.is_empty());
    assert!(outcome.deferred_failed.is_empty());

    // The private copy AND the platform-managed hydrated copy are gone.
    assert!(!rig.bin.exists(), "private .bin must be deleted");
    assert_eq!(rig.remover.removed(), vec!["nes/Game.nes".to_string()]);
    // The durable row is gone — nothing left for a later sweep.
    assert!(rig
        .index
        .ready_if_current(&rig.key, None)
        .unwrap()
        .is_none());
    let second = sweep(&mut rig);
    assert!(second.evicted.is_empty());
    assert_eq!(rig.remover.removed().len(), 1);
}

#[test]
fn recently_used_entry_is_kept() {
    let mut rig = rig();
    rig.clock.advance(DAY); // 1 day idle, threshold is 14

    let outcome = sweep(&mut rig);
    assert!(outcome.evicted.is_empty());
    assert!(rig.bin.exists());
    assert!(rig.remover.removed().is_empty());
    assert!(rig
        .index
        .ready_if_current(&rig.key, None)
        .unwrap()
        .is_some());
}

#[test]
fn an_held_active_guard_blocks_eviction_until_dropped() {
    let mut rig = rig();
    let guard = rig.live.acquire(&rig.key);
    rig.clock.advance(THRESHOLD + DAY);

    let outcome = sweep(&mut rig);
    assert!(outcome.evicted.is_empty());
    assert_eq!(outcome.retained_active, vec![rig.key.clone()]);
    assert!(rig.bin.exists(), "in-use data must never be evicted");
    assert!(rig.remover.removed().is_empty());
    assert!(rig
        .index
        .ready_if_current(&rig.key, None)
        .unwrap()
        .is_some());

    // Once released, the next sweep evicts.
    drop(guard);
    let outcome = sweep(&mut rig);
    assert_eq!(outcome.evicted, vec![rig.key.clone()]);
    assert!(!rig.bin.exists());
}

#[test]
fn failed_cleanup_defers_the_whole_entry() {
    let mut rig = rig();
    rig.clock.advance(THRESHOLD + DAY);
    rig.remover.fail.store(true, Ordering::SeqCst);

    let outcome = sweep(&mut rig);
    assert!(outcome.evicted.is_empty());
    assert_eq!(outcome.deferred_failed, vec![rig.key.clone()]);
    // Hydrated removal failed -> the private copy and the row stay too.
    assert!(rig.bin.exists());
    assert!(rig
        .index
        .ready_if_current(&rig.key, None)
        .unwrap()
        .is_some());

    // Next sweep retries the deferral and completes it.
    rig.remover.fail.store(false, Ordering::SeqCst);
    let outcome = sweep(&mut rig);
    assert_eq!(outcome.evicted, vec![rig.key.clone()]);
    assert!(!rig.bin.exists());
}

#[test]
fn a_missing_bin_is_already_evicted_not_a_failure() {
    let mut rig = rig();
    rig.clock.advance(THRESHOLD + DAY);
    std::fs::remove_file(&rig.bin).unwrap(); // e.g. manual cleanup

    let outcome = sweep(&mut rig);
    assert_eq!(outcome.evicted, vec![rig.key.clone()]);
    assert!(outcome.deferred_failed.is_empty());
}
