//! `RommTree`: the read-only inode tree the platform adapter serves.
//! Pure metadata — listing, lookup, and stat here must never perform IO or
//! download content (PRD R2). Inode identity is stable for an unchanged
//! catalogue across enumerations and remounts.

use crate::catalog::{Catalogue, NodeKind, VersionKey};
use std::collections::HashMap;

pub const ROOT_INODE: u64 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Directory,
    File,
}

#[derive(Clone, Copy, Debug)]
pub struct NodeMeta {
    pub inode: u64,
    pub parent: u64,
    pub size: u64,
    pub kind: EntryKind,
    /// Generation/version for the projected placeholder (changes when the
    /// underlying content version changes so stale hydration is noticed).
    pub generation: u64,
}

/// A directory entry yielded by `read_dir`.
#[derive(Clone, Debug)]
pub struct DirEntry {
    pub name: String,
    pub inode: u64,
    pub kind: EntryKind,
    /// Opaque continuation cookie for small-buffer paging.
    pub cookie: u64,
}

/// The immutable tree built from a `Catalogue`.
pub struct RommTree {
    catalogue: Catalogue,
    /// lowercase platform dir name -> index into `catalogue.platforms`.
    platform_by_lower: HashMap<String, usize>,
    /// platform index -> rom indices into `catalogue.entries`, in order.
    platform_files: Vec<Vec<usize>>,
}

impl RommTree {
    pub fn new(catalogue: Catalogue) -> Self {
        let platform_by_lower: HashMap<String, usize> = catalogue
            .platforms
            .iter()
            .enumerate()
            .map(|(i, p)| (p.to_lowercase(), i))
            .collect();
        let mut platform_files = vec![Vec::new(); catalogue.platforms.len()];
        for (i, e) in catalogue.entries.iter().enumerate() {
            if let Some(&p) = platform_by_lower.get(&e.platform_dir.to_lowercase()) {
                platform_files[p].push(i);
            }
        }
        Self {
            catalogue,
            platform_by_lower,
            platform_files,
        }
    }

    pub fn catalogue(&self) -> &Catalogue {
        &self.catalogue
    }

    /// `lookup(parent_inode, name) -> inode` or `None`.
    /// Name matching is case-insensitive (Windows semantics).
    pub fn lookup(&self, parent: u64, name: &str) -> Option<NodeMeta> {
        match self.catalogue.inode_kind(parent)? {
            NodeKind::Root => {
                let &idx = self.platform_by_lower.get(&name.to_lowercase())?;
                let inode = self.catalogue.platform_inode(idx)?;
                Some(NodeMeta {
                    inode,
                    parent,
                    size: 0,
                    kind: EntryKind::Directory,
                    generation: 0,
                })
            }
            NodeKind::PlatformDir { index } => {
                let lower = name.to_lowercase();
                let rom_index = self
                    .platform_files
                    .get(index)?
                    .iter()
                    .copied()
                    .find(|&i| self.catalogue.entries[i].file_name.to_lowercase() == lower)?;
                let entry = &self.catalogue.entries[rom_index];
                let inode = self.catalogue.rom_inode(rom_index)?;
                Some(NodeMeta {
                    inode,
                    parent,
                    size: entry.size,
                    kind: EntryKind::File,
                    generation: generation_of(entry.version.as_ref()),
                })
            }
            NodeKind::Rom { .. } => None,
        }
    }

    /// `metadata(inode)`.
    pub fn metadata(&self, inode: u64) -> Option<NodeMeta> {
        match self.catalogue.inode_kind(inode)? {
            NodeKind::Root => Some(NodeMeta {
                inode,
                parent: 0,
                size: 0,
                kind: EntryKind::Directory,
                generation: 0,
            }),
            NodeKind::PlatformDir { .. } => Some(NodeMeta {
                inode,
                parent: ROOT_INODE,
                size: 0,
                kind: EntryKind::Directory,
                generation: 0,
            }),
            NodeKind::Rom { index } => {
                let entry = &self.catalogue.entries[index];
                let parent = self
                    .platform_by_lower
                    .get(&entry.platform_dir.to_lowercase())
                    .and_then(|&p| self.catalogue.platform_inode(p))
                    .unwrap_or(ROOT_INODE);
                Some(NodeMeta {
                    inode,
                    parent,
                    size: entry.size,
                    kind: EntryKind::File,
                    generation: generation_of(entry.version.as_ref()),
                })
            }
        }
    }

    /// Page entries of `inode` starting at opaque `cookie` (0 = start),
    /// at most `max` entries; returns `(entries, next_cookie, eof)`.
    /// Small buffers must not lose or duplicate entries.
    pub fn read_dir(
        &self,
        inode: u64,
        cookie: u64,
        max: usize,
    ) -> Option<(Vec<DirEntry>, u64, bool)> {
        let children: Vec<DirEntry> = match self.catalogue.inode_kind(inode)? {
            NodeKind::Root => self
                .catalogue
                .platforms
                .iter()
                .enumerate()
                .map(|(i, p)| DirEntry {
                    name: p.clone(),
                    inode: 2 + i as u64,
                    kind: EntryKind::Directory,
                    cookie: 0,
                })
                .collect(),
            NodeKind::PlatformDir { index } => self
                .platform_files
                .get(index)?
                .iter()
                .map(|&ri| {
                    let e = &self.catalogue.entries[ri];
                    DirEntry {
                        name: e.file_name.clone(),
                        inode: self.catalogue.rom_inode(ri).unwrap_or(0),
                        kind: EntryKind::File,
                        cookie: 0,
                    }
                })
                .collect(),
            NodeKind::Rom { .. } => return None,
        };

        let start = cookie as usize;
        if start >= children.len() {
            return Some((Vec::new(), cookie, true));
        }
        let end = start.saturating_add(max).min(children.len());
        let mut page: Vec<DirEntry> = children[start..end].to_vec();
        for (j, e) in page.iter_mut().enumerate() {
            e.cookie = (start + j + 1) as u64;
        }
        let eof = end >= children.len();
        Some((page, end as u64, eof))
    }

    /// Map a relative path like `nes/Example Game.nes` to an inode.
    /// Used by the Windows adapter to resolve callback paths.
    /// Components match case-insensitively.
    pub fn inode_for_path(&self, components: &[&str]) -> Option<u64> {
        let mut current = ROOT_INODE;
        for c in components {
            current = self.lookup(current, c)?.inode;
        }
        Some(current)
    }

    /// The catalogue ROM index for a file inode (download wiring).
    pub fn rom_index_of(&self, inode: u64) -> Option<usize> {
        match self.catalogue.inode_kind(inode)? {
            NodeKind::Rom { index } => Some(index),
            _ => None,
        }
    }

    /// The relative path `platform_dir/file_name` for a file inode.
    pub fn path_of(&self, inode: u64) -> Option<String> {
        match self.catalogue.inode_kind(inode)? {
            NodeKind::Rom { index } => {
                let e = &self.catalogue.entries[index];
                Some(format!("{}/{}", e.platform_dir, e.file_name))
            }
            _ => None,
        }
    }
}

/// Content-version fingerprint folded into a placeholder generation:
/// changes when the catalogue's version metadata changes, `0` when RomM
/// supplies none.
fn generation_of(version: Option<&VersionKey>) -> u64 {
    match version {
        Some(VersionKey(s)) => {
            let mut h: u64 = 0xcbf29ce484222325;
            for &b in s.as_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            h
        }
        None => 0,
    }
}
