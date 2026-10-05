# Backend decision — WinFsp

The user requested replacing the Windows optional-feature backend on
2026-10-05. `rommfs-winfsp` uses `winfsp` 0.13.1 with WinFsp 2.1 bindings;
there is one Windows mount implementation and no backend selector.

- `FileSystemContext` resolves paths into the existing `RommFs` inode tree.
  Security queries, opens, metadata and directory enumeration never read ROM
  contents. `DirBuffer` sorts entries and implements Windows continuation.
  Opens return the canonical catalogue spelling in WinFsp's normalized-name
  buffer, including when the caller supplies another capitalization.
- `read` alone calls `RommFs::read_at`, preserving lazy single-flight download
  and private-cache reuse. There is no persistent copy in the mounted tree.
- Each open owns an `ActiveGuard`, released on close. Windows caching is
  flushed and purged on cleanup; eviction deletes only the private cache.
- The volume is read-only, with a read/execute security descriptor. Open
  requests for writes, deletes or ACL changes are also rejected. Unimplemented
  mutation callbacks cannot change core data.
- The host owns dispatcher lifetime and removes its junction on drop. RomMFS
  restores the empty directory after unmount. Ownership is stored beside the
  root, since WinFsp creates its mount directory and requires an empty path.
- Nonempty and reparse-point roots are refused, even when marked as owned.
  Legacy roots are preserved; users must select a fresh empty directory.
- Final executables and native adapter tests delay-load the WinFsp DLL;
  initialization finds the installed runtime and reports its absence.

`winfsp_native.rs` covers ordinary OS listing, stat, read, mutations, concurrent
open guards, case-insensitive opens and canonical spelling, eviction/re-download,
stop and remount. `live_romm.rs` retains the explicitly runnable live-server
byte comparison.

Verification on 2026-10-05: portable tests and Clippy pass. The Windows
adapter, native test sources and headless app pass MSVC-target Rust type and
Clippy checks using the published pregenerated WinFsp bindings. For this
Linux-only source check, temporary dependency build scripts bypassed native
C compilation and registry lookup; no patches are committed. This does not
verify a Windows link, driver, mount or live server. The migration implementation
is complete. At the user's request, native Windows build, mount and frontend
verification are deferred to a later Windows session and do not gate this change.

Sources: [Rust bindings](https://github.com/SnowflakePowered/winfsp-rs),
[WinFsp mount implementation](https://github.com/winfsp/winfsp/blob/master/src/dll/mount.c),
[WinFsp API](https://winfsp.dev/doc/WinFsp-API-winfsp.h/).
The Rust bindings are GPL-3.0; distribution must account for that license.
