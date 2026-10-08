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

/// Immutable arbitrary-depth metadata tree. Construction performs no I/O.
pub struct RommTree {
    catalogue: Catalogue,
    children: HashMap<u64, Vec<DirEntry>>,
    metadata: HashMap<u64, NodeMeta>,
    paths: HashMap<u64, String>,
}
impl RommTree {
    pub fn new(catalogue: Catalogue) -> Self {
        let directory_inodes: HashMap<_, _> = catalogue
            .directories()
            .iter()
            .enumerate()
            .map(|(i, d)| (d.to_lowercase(), 2 + i as u64))
            .collect();
        let mut tree = Self {
            catalogue,
            children: HashMap::new(),
            metadata: HashMap::new(),
            paths: HashMap::new(),
        };
        tree.metadata.insert(
            ROOT_INODE,
            NodeMeta {
                inode: ROOT_INODE,
                parent: 0,
                size: 0,
                kind: EntryKind::Directory,
                generation: 0,
            },
        );
        tree.paths.insert(ROOT_INODE, String::new());
        let nodes: Vec<_> = tree
            .catalogue
            .directories()
            .iter()
            .enumerate()
            .map(|(i, path)| (2 + i as u64, path.clone(), 0, EntryKind::Directory, 0))
            .chain(tree.catalogue.entries.iter().enumerate().map(|(i, e)| {
                (
                    tree.catalogue.rom_inode(i).expect("catalogue entry inode"),
                    format!("{}/{}", e.platform_dir, e.file_name),
                    e.size,
                    EntryKind::File,
                    generation_of(e.version.as_ref()),
                )
            }))
            .collect();
        for (inode, path, size, kind, generation) in nodes {
            let (parent_path, name) = path.rsplit_once('/').unwrap_or(("", &path));
            let parent = directory_inodes
                .get(&parent_path.to_lowercase())
                .copied()
                .unwrap_or(ROOT_INODE);
            tree.metadata.insert(
                inode,
                NodeMeta {
                    inode,
                    parent,
                    size,
                    kind,
                    generation,
                },
            );
            tree.children.entry(parent).or_default().push(DirEntry {
                name: name.into(),
                inode,
                kind,
                cookie: 0,
            });
            if kind == EntryKind::Directory {
                tree.children.entry(inode).or_default();
            }
            tree.paths.insert(inode, path);
        }
        tree.children.entry(ROOT_INODE).or_default();
        for children in tree.children.values_mut() {
            children.sort_by(|a, b| a.name.cmp(&b.name));
            for (i, entry) in children.iter_mut().enumerate() {
                entry.cookie = i as u64 + 1;
            }
        }
        tree
    }
    pub fn catalogue(&self) -> &Catalogue {
        &self.catalogue
    }
    pub fn lookup(&self, parent: u64, name: &str) -> Option<NodeMeta> {
        let lower = name.to_lowercase();
        let child = self
            .children
            .get(&parent)?
            .iter()
            .find(|c| c.name.to_lowercase() == lower)?;
        self.metadata(child.inode)
    }
    pub fn metadata(&self, inode: u64) -> Option<NodeMeta> {
        self.metadata.get(&inode).copied()
    }
    pub fn read_dir(
        &self,
        inode: u64,
        cookie: u64,
        max: usize,
    ) -> Option<(Vec<DirEntry>, u64, bool)> {
        let children = self.children.get(&inode)?;
        let start = usize::try_from(cookie).ok()?;
        if start >= children.len() {
            return Some((Vec::new(), cookie, true));
        }
        let end = start.saturating_add(max).min(children.len());
        Some((
            children[start..end].to_vec(),
            end as u64,
            end == children.len(),
        ))
    }
    pub fn inode_for_path(&self, components: &[&str]) -> Option<u64> {
        let mut current = ROOT_INODE;
        for component in components {
            current = self.lookup(current, component)?.inode;
        }
        Some(current)
    }
    pub fn rom_index_of(&self, inode: u64) -> Option<usize> {
        match self.catalogue.inode_kind(inode)? {
            NodeKind::Rom { index } => Some(index),
            _ => None,
        }
    }
    pub fn path_of(&self, inode: u64) -> Option<String> {
        self.paths.get(&inode).cloned()
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
