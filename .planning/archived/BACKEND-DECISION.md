# Backend decision — Windows CFAPI

On 2026-10-05 the user selected custom Windows Cloud Files API integration,
with `fsk` reserved for Linux/macOS, then narrowed implementation to Windows.
This supersedes the WinFsp decision. Linux/macOS have no implementation in this change.

The constraint is no separately installed filesystem driver or ProjFS optional
feature. CFAPI uses Windows' built-in `cldflt.sys`; it still requires Windows 10
1709+ and a local NTFS volume. Native deployment suitability is unverified.

`rommfs-cfapi` uses existing `windows-sys` bindings without another wrapper crate.
The app retains its portable `RommFs`, download manager, catalogue and cache index.

- Register a sync root with FULL hydration and an eagerly populated metadata tree.
  Creating/listing/statting placeholders does not read ROM contents.
- FETCH_DATA downloads through the existing complete-file, single-flight cache,
  then transfers 1 MiB aligned chunks to NTFS. Progress heartbeats keep the native
  fetch timeout alive while the complete private download is being verified.
- NTFS stores a hydrated copy in addition to the private cache. Eviction first
  obtains exclusive access and dehydrates the owned placeholder, then removes the
  private copy and index row. Failures defer the whole eviction.
- Native external open notifications record warm access. Provider metadata opens
  are excluded to prevent reentry while the index is locked during eviction.
- Protected read/execute ACLs reject ordinary data writes and namespace changes.
  Owner WRITE_DAC permits CFAPI operations; this is not a security boundary
  against the owner intentionally changing permissions. Delete/rename callbacks
  also reject ordinary mutations while connected.
- A sibling manifest records server, paths, content/version identities and the
  original root ACL. Restart accepts only verified unmodified cloud placeholders;
  it rejects normal files, unknown entries, links and ownership mismatches.
- Startup rebuilds owned placeholders from the fresh catalogue. Private completed
  bytes survive; persistent hydrated copies are not trusted across catalogue changes.
- Shutdown disconnects, drains callbacks and removes only verified owned entries.
  Unregistration occurs only after the root is empty, because CFAPI unregistration
  traverses the root and can delete unhydrated placeholders. Busy/changed entries
  defer cleanup; registration/manifest are retained for recovery. A failed native
  disconnect aborts the process rather than freeing a still-live callback context.

Verification: portable regression tests pass. Windows Rust type/Clippy checks
use temporary C-dependency build stubs on this Linux host; they do not prove
Windows C compilation, linking, installation, mounting or emulator compatibility.
No temporary dependency patches are committed. Native tests and frontend smoke
checks are **not run** and must pass on clean Windows before calling this backend
validated. The native test includes external warm opens, byte correctness,
mutation rejection, active-file retention, dehydration/re-download, truncated
transfer/retry, clean stop/remount and preservation of unowned files.

Remaining acceptance work: native Windows build/runtime; clean-user setup without
WinFsp/ProjFS; slow downloads, memory-mapped reads, crash recovery and ACL restoration;
real frontend/emulator launch; actual disk allocation reclaimed from both copies.

Sources: [Cloud Files architecture](https://learn.microsoft.com/en-us/windows/win32/cfapi/build-a-cloud-file-sync-engine),
[placeholder creation](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfcreateplaceholders),
[transfer contracts](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/ns-cfapi-cf_operation_parameters),
[progress/timeouts](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfreportproviderprogress),
[disconnect lifetime](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfdisconnectsyncroot),
[unregistration side effects](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfunregistersyncroot).
