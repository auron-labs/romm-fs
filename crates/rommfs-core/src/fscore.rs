//! The filesystem-facing facade: what the platform adapter AND the portable
//! "real filesystem-facing implementation" tests drive (PRD §6). Composes
//! `RommTree` + `DownloadManager` + `Evictor`; performs the policy that turns
//! a content read into a cached-file read.

use crate::cache::Evictor;
use crate::cache::clock::Clock;
use crate::cache::policy::{ActiveGuard, EvictionOutcome};
use crate::download::DownloadManager;
use crate::error::Result;
use crate::tree::{NodeMeta, RommTree};
use std::sync::Arc;

/// Everything the adapter needs from core, in one object.
pub struct RommFs {
    tree: RommTree,
    downloads: Arc<DownloadManager>,
    evictor: Evictor,
    clock: Arc<dyn Clock>,
}

impl RommFs {
    pub fn new(
        tree: RommTree,
        downloads: Arc<DownloadManager>,
        evictor: Evictor,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self { tree, downloads, evictor, clock }
    }

    // --- metadata: never downloads ---

    pub fn lookup(&self, parent: u64, name: &str) -> Option<NodeMeta> {
        self.tree.lookup(parent, name)
    }
    pub fn metadata(&self, inode: u64) -> Option<NodeMeta> {
        self.tree.metadata(inode)
    }
    pub fn read_dir(
        &self,
        inode: u64,
        cookie: u64,
        max: usize,
    ) -> Option<(Vec<crate::tree::DirEntry>, u64, bool)> {
        self.tree.read_dir(inode, cookie, max)
    }
    pub fn inode_for_path(&self, components: &[&str]) -> Option<u64> {
        self.tree.inode_for_path(components)
    }
    pub fn path_of(&self, inode: u64) -> Option<String> {
        self.tree.path_of(inode)
    }

    // --- content path ---

    /// `read_at(inode, offset, out) -> bytes_filled`.
    /// On first data access: one download (single-flight), then serve bytes
    /// from the private cache file. Correct short reads at EOF; `out` empty
    /// or offset >= size needs no download (PRD R3).
    pub fn read_at(&self, inode: u64, offset: u64, out: &mut [u8]) -> Result<usize> {
        let _ = (inode, offset, out);
        todo!("resolve inode->RomKey; guard via live.acquire; ensure_ready; read_at from cached file; index.touch")
    }

    /// Record an open for access tracking (Windows notification path calls
    /// this — warm reads may bypass `read_at` but opens still count).
    /// Returns the active-use guard; caller drops it at close.
    pub fn note_open(&self, inode: u64) -> Option<ActiveGuard> {
        let _ = inode;
        todo!()
    }

    /// Run one eviction sweep. The hydrated remover handles the
    /// platform-managed copy (PrjDeleteFile on Windows).
    pub fn evict_stale(&self) -> Result<EvictionOutcome> {
        todo!("evictor.sweep(index, clock, rel_path_of=path_of(key), remove_file=fs::remove_file)")
    }
}
