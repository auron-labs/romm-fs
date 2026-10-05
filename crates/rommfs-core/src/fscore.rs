//! The filesystem-facing facade: what the platform adapter AND the portable
//! "real filesystem-facing implementation" tests drive (PRD §6). Composes
//! `RommTree` + `DownloadManager` + `Evictor`; performs the policy that turns
//! a content read into a cached-file read.

use crate::cache::clock::Clock;
use crate::cache::policy::{ActiveGuard, EvictionOutcome};
use crate::cache::Evictor;
use crate::catalog::RomKey;
use crate::download::DownloadManager;
use crate::error::{Error, Result};
use crate::tree::{NodeMeta, RommTree};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;

/// Everything the adapter needs from core, in one object.
pub struct RommFs {
    tree: RommTree,
    downloads: Arc<DownloadManager>,
    evictor: Evictor,
    clock: Arc<dyn Clock>,
    /// key -> `platform_dir/file_name` for hydrated-copy removal on eviction.
    key_paths: HashMap<RomKey, String>,
}

impl RommFs {
    pub fn new(
        tree: RommTree,
        downloads: Arc<DownloadManager>,
        evictor: Evictor,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let key_paths = tree
            .catalogue()
            .entries
            .iter()
            .map(|e| (e.key.clone(), format!("{}/{}", e.platform_dir, e.file_name)))
            .collect();
        Self {
            tree,
            downloads,
            evictor,
            clock,
            key_paths,
        }
    }

    // --- metadata: never downloads ---

    pub fn catalogue(&self) -> &crate::catalog::Catalogue {
        self.tree.catalogue()
    }

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
        let entry = self.entry_for(inode)?;
        let size = entry.size;
        // A zero-length or at/past-EOF read never triggers a transfer (R3).
        if out.is_empty() || offset >= size {
            return Ok(0);
        }

        // Guard BEFORE the download/lookup so a concurrent sweep cannot
        // evict the entry between readiness check and file open — the
        // acquire/evict critical sections serialize under LiveState's lock.
        let _guard = self.downloads.live().acquire(&entry.key);
        let path = self.downloads.ensure_ready(entry)?;

        let mut file = std::fs::File::open(&path).map_err(Error::Io)?;
        file.seek(SeekFrom::Start(offset)).map_err(Error::Io)?;
        let limit = out.len().min((size - offset) as usize);
        let mut n = 0usize;
        while n < limit {
            match file.read(&mut out[n..limit]) {
                Ok(0) => break, // EOF: return the short read (R3)
                Ok(m) => n += m,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(Error::Io(e)),
            }
        }

        // Real access: last-use is stamped on data reads, not downloads.
        self.downloads
            .index()
            .lock()
            .touch(&entry.key, self.clock.unix_secs())?;
        Ok(n)
    }

    /// Record an open for access tracking. Warm OS reads may bypass `read_at`.
    /// Returns the active-use guard; caller drops it at close.
    pub fn note_open(&self, inode: u64) -> Option<ActiveGuard> {
        let entry = self.entry_for(inode).ok()?;
        // An open is real access activity even when the read itself is
        // later served from an OS-side cache.
        let _ = self
            .downloads
            .index()
            .lock()
            .touch(&entry.key, self.clock.unix_secs());
        Some(self.downloads.live().acquire(&entry.key))
    }

    /// Run one eviction sweep. The hydrated remover handles the
    /// platform-managed copy before removing private cached bytes.
    pub fn evict_stale(&self) -> Result<EvictionOutcome> {
        let key_paths = &self.key_paths;
        self.evictor.sweep(
            &mut self.downloads.index().lock(),
            self.clock.as_ref(),
            |key| key_paths.get(key).cloned(),
            |path| std::fs::remove_file(path),
        )
    }

    fn entry_for(&self, inode: u64) -> Result<&crate::catalog::RomEntry> {
        let index = self
            .tree
            .rom_index_of(inode)
            .ok_or_else(|| Error::Unsupported(format!("inode {inode} is not a ROM file")))?;
        Ok(&self.tree.catalogue().entries[index])
    }
}
