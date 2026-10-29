# Architecture

This page describes the current workspace as implemented. It is a navigation
map, not a target design; use the code as the authority when behavior changes.

## Follow a user action

For a UI change, start in `crates/rommfs-app/src/window/pages/`. Each page reads
`UiState` and sends a `Command`; the window does not download games or sync saves.

For a behavior change, follow that command into the relevant module under
`crates/rommfs-app/src/controller/`. Workers report events, and
`controller/state.rs` turns them into the state the pages display. Shared
folder, save, and artwork summaries live there so different screens agree.
`problem.rs` translates technical failures into explanations; the original
message remains available in the UI's technical details.

For example, **Make games available** sends `Command::StartMount`. The mount
controller prepares the filesystem through core and the platform adapter, then
reports progress and completion. **Available** comes from the completion event.
Opening a game later triggers its download through the filesystem; displaying
the Overview does not read game contents.

## Crate boundaries

`rommfs-core` contains portable domain and I/O code: RomM HTTP access in
`romm/`, DTO-to-filesystem projection in `catalogue.rs`, path and layout rules,
cache/download behavior, save-sync rules and persistence, plus RetroBat setup
and metadata. Its filesystem facade is `filesystem.rs`; `tree.rs` provides stable
filesystem enumeration from a catalogue snapshot.

`rommfs-cfapi` separates portable root ownership and frontend metadata
operations (`root.rs`) from Windows CFAPI and ACL behavior (`windows.rs` and
`security.rs`). Native integration tests exercise a real mounted root on
Windows.

`rommfs-app` owns user-facing orchestration. `controller.rs` owns controller state and worker dispatch. Focused modules in
`controller/` handle state/events, commands, connection, catalogue, mounting,
RetroBat setup, and save-sync setup. `save_sync_agent/` owns the background reconciliation and
transfer actor. `window.rs` and `window/` provide the GPUI interface. The
session and desktop modules own app lifetime and Windows tray/startup behavior.

`rommfs-fixture` supplies a local HTTP server for deterministic tests. It
supports assertions about requests and response bodies without requiring a
live RomM instance. `route_hit_count` counts scripted route matches;
`request_count` counts captured wire requests.

## Data flow

1. The app authenticates through a `RommClient` and verifies the account when
   using remembered browser pairing.
2. It restores an account/server-scoped catalogue snapshot when available, or
   fetches the paginated platforms and ROM/file metadata. The first page
   requests the total; the remaining offsets are fetched four at a time and
   reassembled in offset order. A refresh is incremental when a snapshot for the
   same server/account exists and its last full refresh is under 24 h old:
   `/api/roms/identifiers` gives the authoritative visible id set and
   `updated_after` gives the changed ROMs, which are merged in place. A missing
   `/identifiers` route, a rejected `updated_after` or an unreconcilable delta
   falls back to a full walk. Refresh can happen in the background while the
   previous complete snapshot remains available.
3. `catalogue.rs` validates each original file identity and path, then projects
   supported original files into a generic or RetroBat tree. Multi-file games
   are represented as their individual original files when their paths are
   valid; unsupported or invalid records are reported.
4. The tree exposes placeholders. Listing is metadata-only. A content read
   resolves the `(server, ROM, file)` identity and downloads original bytes to
   the private cache as needed.
5. On Windows, `rommfs-cfapi` presents that filesystem through Cloud Files API.
   Root ownership checks and cleanup preserve unowned or modified content.
6. Save synchronization is a separate opt-in path. Controller configuration
   establishes its identity and roots; the save-sync agent observes files,
   reconciles local and remote revisions, and preserves conflicts for review.

## State and ownership rules

- A catalogue is immutable for a mounted tree. Refresh produces a new snapshot;
  the UI can report that a remount is needed for structural changes.
- Catalogue refresh has two modes. A full walk disables the unused
  char-index/filter-value/rom-id-index sidecars and asks for the total only on
  the first page; an incremental refresh merges `updated_after` against the
  snapshot and `/api/roms/identifiers`. The server's `updated_at` delta may not
  cover a rescan that only changed a ROM's file rows, so a full refresh runs at
  least every 24 h (`CatalogueSnapshot.full_refreshed_at`) and whenever the
  delta cannot be reconciled. `refreshed_at` is the refresh *start* time, so the
  next delta's five-minute skew covers the whole window.
- Real-time updates are a bounded incremental poll, not a socket client: RomM's
  socket.io connect handler resolves identity from the web session cookie only
  (the `auth` payload is ignored), so a bearer-only session cannot subscribe.
  The connected session arms a five-minute catalogue poll; it is cleared on
  sign-out and drained by the same cancellation as a manual refresh, and a
  stale result cannot publish into a later connection. The mounted tree stays a
  snapshot; the poll only stages the next one. Live play data rides the same
  path: a poll that lands while mounted re-runs the metadata pass, which already
  pulls favourites and play sessions.
- Catalogue snapshots are scoped by normalized server and authenticated account.
  Tokens remain in memory; only paired secrets use Windows Credential Manager.
- ROM content cache and save-sync state are separate persistence areas.
- Save conflict choices retire superseded upload snapshots in the journal while
  keeping their private recovery files. Pending transfers are cancelled and drained
  before applying a choice; their retry queue and late results are discarded.
  Other unfinished uploads resume from the journal. Keep PC authorizes the selected snapshot
  against the reviewed history; an obsolete earlier baseline cannot override
  that choice. The acknowledgement follows verified upload. Reconciliation and
  upload history checks share verified content by account, scope, revision ID,
  and unchanged metadata; new or changed metadata triggers a fresh download.
  The worker retains verified metadata and hashes for the current scope. After
  16 resident payloads, additional payloads spill to temporary files, avoiding
  repeated downloads of unchanged history. Those files live until the revision
  disappears, the scope changes, or the worker stops.
- Login startup resumes consented save sync only after the asynchronous mount
  completes successfully; failed mounting keeps sync stopped.
- Save presentation uses current per-identity work emitted by the actor, not
  historical transfer stages. A settled unchanged watcher hint publishes its
  drained queue. Retained remote review versions remain attached to their save
  identity and are excluded from queued-download counts. Persisted consent,
  checking, current transfers and last successful scan time are separate facts.
  See [UI remediation](ux-remediation.md) for the affected journeys and limits.
- Metadata setup inspection validates the managed root through the CFAPI owner
  before comparing its setup record. Ordinary save/setup paths still reject
  reparse points. Artwork publication and favourites/history failures have
  separate presentation state.
- Mount ownership is recorded beside the root. Ordinary Stop disconnects CFAPI
  and drains callbacks while retaining registration and projected files.
  Restart compares path, original-file identity, version, and size, reusing
  unchanged entries. Explicit root/account replacement uses owned cleanup and
  does not recursively clear user data. Preserved frontend files keep their
  owned parent-directory entries in the cleanup manifest.
- Worker threads/tasks are owned by the app/controller lifecycle and are joined
  or drained on stop, logout, or quit. Cancelling a background job moves its
  handle to a draining list the worker loop reaps with `is_finished()`, so an
  in-flight HTTP request cannot stall the loop for its whole timeout; the
  handles are joined when the connection changes or the app shuts down.
  Event/session IDs prevent stale work from changing a later connection.
- Catalogue refresh, artwork caching, and gamelist generation/publication each
  run on their own thread and are polled by the worker. Gamelist publication
  reads and merges the RetroBat-owned gamelists, so it never blocks the loop;
  the mounted tree stays a snapshot and a completed refresh is applied on the
  next mount. A finished refresh result that waits more than five seconds to be
  polled is logged as a watchdog warning.
- The frontend metadata pass is visible: it reports a stage ("Downloading
  artwork", "Writing N gamelists", "Artwork & gamelists up to date") and artwork
  counts as `FrontendMetadataProgress`. These counts describe artwork checks,
  including cache hits, rather than the whole update. Once they finish, the UI
  uses an indeterminate indicator while game information is prepared and
  published. A read-only process check of the selected RetroBat installation
  defers frontend writes while its frontend runs. A single timer checks again
  every two seconds; explicit Retry checks immediately. Waiting and finished
  states have no running progress bar. Partial publication reports issues.
  Cancelling/stopping a pass does not advance the last-success time.
  Artwork uses eight bounded workers
  (`CONCURRENCY` in `retrobat/artwork.rs`, matching this note), and progress is
  throttled to about 4 Hz with the completed count only moving forward (the
  reporter takes the maximum under its own lock, so out-of-order worker
  completions cannot report a lower count). Every status and progress event
  carries the pass's monotonic job id; `FrontendMetadataInvalidated` marks a
  cancelled pass, and the UI accepts progress only for the currently active
  pass, so a stale artwork callback or publication status cannot revive or
  finish a newer pass. The log line for each status is still written even when
  the UI drops the event. Artwork downloads use the same `RommClient` as the
  catalogue; `isahc`'s `Request::send` runs on a process-global shared client, so
  the connection pool is reused across the artwork workers and the catalogue
  fetch. A cached artwork file is named by its detected extension
  (`.rommfs-artwork/<key>.<ext>`), so a cache hit is validated by size only and
  never re-reads (or re-raster-checks) the image bytes; the exact file path is
  still reparse-checked, not just its parent.
- RetroBat integration coexists with the user's own ROMs. Setup adds only a
  `roms/<system>/RomM` link per system and records what it owns in
  `.rommfs-setup.json`; it never moves, replaces or deletes existing files.
  An earlier-schema record is surfaced to the UI as a migration prompt rather
  than a mount error: Overview offers an update that reruns setup, and Start is
  disabled while setup or migration is pending. Rerunning setup converts the
  old links and restores any folders an earlier build moved aside. Successful
  setup persists the selected games folder before publishing health. Skipped
  systems remain visible as issues; working system links can still start.
  Gamelists are merged in place: RomMFS rewrites only its `./RomM/…` entries.
  Publication checks current link targets; missing or redirected links are
  excluded. Unreadable systems retain their identity state. Gamelist writes
  are attempted independently; any write failure leaves the identity index
  staged so a retry cannot recapture old XML against new filename mappings.
- Favourites and play-history sync (`romm/play_data.rs`, `retrobat/play_data.rs`)
  is opt-in and scoped to the verified server and account (`play_data_consent`).
  The metadata pass pulls the favourites collection and the account's play
  sessions once, reconciles each `./RomM/…` game against a per-identity baseline
  stored in the metadata index, and publishes the merged values; a new local
  launch is pushed as a play session. The first reconciliation adopts the union
  of both sides instead of adding two histories. A failed push keeps the local
  delta for the next pass. A debounced gamelist watch re-runs metadata after
  RetroBat writes play data, on the metadata thread and after the same scoped
  frontend process check.
- "Hide RomM games you already have" is on by default and runs in the metadata
  pass (`retrobat/owned.rs`). It reads the user's own files in `roms/<system>/`
  read-only, matching by name-without-extension and exact size, then confirming
  under a 512 MB cap with whichever of CRC32, MD5 or SHA-1 RomM provides; above
  the cap name and size are enough. The `RomM` folder is skipped by name even
  when it is a real directory, and a linked system root or a linked `roms` root
  is never followed. A file the user can't read never matches, so nothing is
  inferred from an unreadable path. Local hashes are cached in the per-server
  cache by path, size and nanosecond modification time. A system that can't be
  read is reported and left visible. Ownership of the gamelist `hidden` flag is
  recorded per identity in the metadata index (`auto_hidden`), so removing a
  local copy, or turning the switch off, unhides only what RomMFS hid — a
  `hidden` the user set, and play-data history, are never touched. Play-data
  sync carries `hidden` verbatim and never pushes it to RomM.
- Save-sync consent and journal state are separate from mount configuration.
  Paths, scopes, and identities are validated before reads or writes.
  An explicit disable during initial catalogue loading is retained for that
  connection and persisted when its save scope becomes available, before
  workers can start from older remembered consent.
- Memory-card sync (`save_sync/memory_cards.rs`, `save_sync/memory_card_journal.rs`,
  `save_sync_agent/memory_cards.rs`, `romm/memory_cards.rs`) is a separate sync
  kind next to ordinary saves. A card's identity is (server, account, emulator,
  card name); its durable baseline is the accepted RomM version id plus the hash
  of the local card (`MemoryCardJournal`, in the save-sync journal DB). Discovery
  reads only the two RetroBat `memcards` folders (`psx/duckstation/memcards`,
  `ps2/pcsx2/memcards`); a card is a `*.mcd`/`*.ps2` file or, for PCSX2, a
  folder. Consent is off by default and scoped per discovered card folder
  (`memory_card_consent`), like a save root. The card reconciliation thread is
  owned by `SaveSyncAgent` (its start/stop/drop lifecycle), so it also runs when
  save sync itself is off; it keeps its own polling cadence because a 256 MB
  transfer must not block the save actor. It reuses `SaveSyncScope`/
  `SaveSyncIdentity`, the consent store, and the emulator guard. The guard
  enumerates the executables inside each installed DuckStation/PCSX2 folder and
  opens them exclusively (rather than guessing names); an installed folder with
  no executable, or any open image, means nothing syncs — the reason is
  reported, never a silent pass. It also waits for the quiet period before an
  upload. A consented folder is enumerated even when empty, so a card that
  exists only on RomM (synced from another machine) is restored; the archive
  layout is inferred (a single top-level file with the emulator's own extension
  restores as a file, anything else as a folder for PCSX2) and the
  server-supplied name is validated first. Uploads are bounded, safe zips
  (≤256 MB). An unreadable card, a reparse point, or a folder card containing a
  reparse point is rejected, never treated as absent or silently truncated.
  Before a download replaces a card the worker rechecks cancellation and the
  local baseline, and refuses if the destination exists at all (even
  unreadable), so a concurrent local edit becomes a conflict, never an
  overwrite. A conflict keeps the local card and writes the remote beside it as
  a version-specific `.rommfs-conflict-v<id>` sibling, so every reported remote
  version is actually preserved; the card is listed in Save review. Archive
  entry paths are validated per component with the same Windows-name validator
  used for save targets. The `memcards` exclusion for ordinary per-game save
  mapping remains in force.

For user-facing restrictions, see the [user guide](user-guide.md). For the
source and confidence level of RomM route assumptions, see the [API contract](contracts/romm-api.md).
