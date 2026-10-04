//! `RommTree`: the read-only inode tree the platform adapter serves.
//! Pure metadata — listing, lookup, and stat here must never perform IO or
//! download content (PRD R2). Inode identity is stable for an unchanged
//! catalogue across enumerations and remounts.

use crate::catalog::Catalogue;

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
}

impl RommTree {
    pub fn new(catalogue: Catalogue) -> Self {
        Self { catalogue }
    }

    pub fn catalogue(&self) -> &Catalogue {
        &self.catalogue
    }

    /// `lookup(parent_inode, name) -> inode` or `None`.
    pub fn lookup(&self, parent: u64, name: &str) -> Option<NodeMeta> {
        let _ = (parent, name);
        todo!()
    }

    /// `metadata(inode)`.
    pub fn metadata(&self, inode: u64) -> Option<NodeMeta> {
        let _ = inode;
        todo!()
    }

    /// Page entries of `inode` starting at opaque `cookie` (0 = start),
    /// at most `max` entries; returns `(entries, next_cookie, eof)`.
    /// Small buffers must not lose or duplicate entries.
    pub fn read_dir(&self, inode: u64, cookie: u64, max: usize) -> Option<(Vec<DirEntry>, u64, bool)> {
        let _ = (inode, cookie, max);
        todo!()
    }

    /// Map a relative path like `nes/Example Game.nes` to an inode.
    /// Used by the Windows adapter to resolve callback paths.
    pub fn inode_for_path(&self, components: &[&str]) -> Option<u64> {
        let _ = components;
        todo!()
    }

    /// The catalogue ROM index for a file inode (download wiring).
    pub fn rom_index_of(&self, inode: u64) -> Option<usize> {
        let _ = inode;
        todo!()
    }

    /// The relative path `platform_dir/file_name` for a file inode.
    pub fn path_of(&self, inode: u64) -> Option<String> {
        let _ = inode;
        todo!()
    }
}
