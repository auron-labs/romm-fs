# Architecture

This page describes the current workspace as implemented. It is a navigation
map, not a target design; use the code as the authority when behavior changes.

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
   fetches the paginated platforms and ROM/file metadata. Refresh can happen in
   the background while the previous complete snapshot remains available.
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
- Catalogue snapshots are scoped by normalized server and authenticated account.
  Tokens remain in memory; only paired secrets use Windows Credential Manager.
- ROM content cache and save-sync state are separate persistence areas.
- Mount ownership is recorded beside the root. Cleanup validates ownership and
  does not recursively clear user data.
- Worker threads/tasks are owned by the app/controller lifecycle and are joined
  or drained on stop, logout, or quit. Event/session IDs prevent stale work from
  changing a later connection.
- RetroBat integration coexists with the user's own ROMs. Setup adds only a
  `roms/<system>/RomM` link per system and records what it owns in
  `.rommfs-setup.json`; it never moves, replaces or deletes existing files.
  Gamelists are merged in place: RomMFS rewrites only its `./RomM/…` entries.
  Publication checks current link targets; missing or redirected links are
  excluded. Unreadable systems retain their identity state. Gamelist writes
  are attempted independently; any write failure leaves the identity index
  staged so a retry cannot recapture old XML against new filename mappings.
- Save-sync consent and journal state are separate from mount configuration.
  Paths, scopes, and identities are validated before reads or writes.

For user-facing restrictions, see the [user guide](user-guide.md). For the
source and confidence level of RomM route assumptions, see the [API contract](contracts/romm-api.md).
