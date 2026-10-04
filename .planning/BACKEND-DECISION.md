# Backend decision — `fsk` 0.0.9 retained

Evaluated against the PRD §2 checklist by reading the shipped Windows adapter
(`fsk-0.0.9/src/windows.rs`, registry source, 2026-10-04) — not Linux rustdoc.

## Feasibility findings (Windows / ProjFS)

1. **Project dir+file, list/stat without content reads.** ProjFS placeholders;
   enumeration goes through `StartDirectoryEnumeration`/`GetDirectoryEnumeration`
   → our `read_directory`; stat via `GetPlaceholderInfo` → our `metadata`/`lookup`.
   Content flows only via `GetFileData` → our `read`. PASS.
2. **Cached reopen + access/lifetime info.** `PRJ_NOTIFY_FILE_OPENED` and
   `PRJ_NOTIFY_FILE_HANDLE_CLOSED_NO_MODIFICATION` reach `Filesystem::raw_event`
   when `raw_event_mask` includes `Operation::Notification`. `RawEvent.caller
   .process_id` carries `TriggeringProcessId`; the file path is readable inside
   the synchronous callback via the borrowed `PRJ_CALLBACK_DATA->FilePathName`.
   Warm reads served from Windows' cache bypass `read` but still emit open/close
   notifications → last-use tracking works. PASS.
3. **Evict hydrated data, keep virtual entry.** `PrjDeleteFile` requires the
   `PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT`, which fsk does not expose publicly but
   delivers inside `NativeRequest::Windows::namespace_context` during any raw
   event. We capture the handle value while mounted (guarded by mount lifetime —
   no borrowed callback pointers retained) and call `PrjDeleteFile` /
   `PrjUpdateFileIfNeeded` as eviction requires, checking
   `PRJ_UPDATE_FAILURE_CAUSES` and never forcing dirty/full/unmanaged files.
   Must be validated by the native test. PASS (pending native verification).
4. **Read-only + stop/restart.** Mutation vetoes via `RawOutcome::Reject` on
   `PRJ_NOTIFY_PRE_DELETE | PRE_RENAME | PRE_SET_HARDLINK |
   PRE_CONVERT_TO_FULL | FILE_OVERWRITTEN`. `MountSession::drop` →
   `PrjStopVirtualizing`; remount restarts cleanly. PASS.

## Caveats discovered (recorded, accepted)

- `MountOptions.read_only` and `filesystem_name` are **ignored** on Windows —
  read-only must be enforced through the notification vetoes above.
- `PRJ_NOTIFY_NEW_FILE_CREATED` is a post-only notification; new *local* files
  created inside the root cannot be vetoed (ProjFS merges them into the view).
  Scope per PRD is protecting *projected ROM entries* — documented limitation.
- `NetworkFilesystem::open_file`/`close_file` are never invoked on Windows
  (`mount_network` == `mount`); the local-file lease only applies to
  macOS/Linux. We implement `Filesystem` + raw events only.
- Directory continuation: fsk collects all entries via repeated
  `read_directory` calls and buffers per enumeration GUID; our cookie paging
  just needs to be correct — small OS buffers are handled inside fsk.
- `PrjMarkDirectoryAsPlaceholder` + `create_dir_all` on the root happens in
  `mount()`. Mounting only into empty/recognized roots is OUR check (PRD R5§5).
- **Found in E2E testing:** `PrjStopVirtualizing` leaves the root's ProjFS
  reparse tag + hydrated files behind (by design). fsk re-runs
  `PrjMarkDirectoryAsPlaceholder` unconditionally, which fails on a tagged
  root with `ERROR_FILE_SYSTEM_VIRTUALIZATION_BUSY` — wedging remounts.
  Namespace teardown is also asynchronous, briefly surfacing
  `ERROR_FILE_SYSTEM_VIRTUALIZATION_UNAVAILABLE` (369) at the same path.
  `mount_with_handle` recreates an *owned* (marker-bearing) stale root and
  retries transient failures on a 10s deadline — same-session and
  cross-run remounts verified by the native test.

**Conclusion:** required behavior achievable without a backend fork → retain
`fsk = 0.0.9` (pinned via committed `Cargo.lock`). `unifuse` fallback not used.
