# RomMFS improvement implementation plan

Implement the recommendations as small, independently useful changes to the
existing download, cache, controller, save actor, and RetroBat integration.
Start with ROM integrity, save-history traffic, and conflict resolution. Add
emulator coverage after those fixes, then the larger convenience features.

This is an implementation plan, not a record of completed changes. It is based
on the current working tree at `4b95a63`, reviewed on 2026-10-17, and these inputs:

- [Project improvement review](../codex-session-01a13e24-09d8-7313-8c04-4759cbb365c7.md),
  numbered recommendations 1–10 below referenced as A1–A10.
- [RomMBat investigation session](../codex-session-01a14486-f8c7-7180-9aec-606b5809a0fb.md).
- [RomMBat assessment](../docs/research/rommbat-assessment-2026-10-16.md),
  numbered recommendations 1–8 below referenced as B1–B8. The investigation
  session and assessment describe the same work.

## Scope and delivery order

Every recommendation has a delivery item. Resume is one implementation shared
by both reviews. Native state integration and the menu shortcut retain their
lower priority; they have concrete scopes below rather than open design tasks.

| Item | Deliverable | Source | Order |
| --- | --- | --- | --- |
| 1 | Verify ROM downloads and reject damaged cache entries | A1 | First batch |
| 2 | Avoid repeated save-history body downloads | A2 | First batch |
| 3 | Resolve ordinary-save conflicts in Save review | A3 | First batch |
| 4 | Cancel stalled requests and individual CFAPI fetches | A6 | Reliability |
| 5 | Retry remembered connections after temporary outages | A5 | Reliability; after 4 |
| 6 | Bound storage and expose safe cleanup | A4 | Reliability |
| 7 | Recognize measured emulator save layouts | B1, B8 | Coverage |
| 8 | Feed local EmulationStation events into existing workers | B2 | Coverage; after 7 |
| 9 | Resolve BIOS requirements to verified destinations | B4 | Coverage |
| 10 | Synchronize one explicitly grouped save layout | B3 | After 7 and 8 |
| 11 | Resume verified original-file downloads | A8, B7 | After 1, 4, and 6 |
| 12 | Prepare selected games and support explicit offline access | A9 | After 1, 4, 5, and 6 |
| 13 | Automate checks and produce a Windows release archive | A7, A10 | Checks from first batch; test the candidate archive |
| 14 | Show compatibility information and per-emulator evidence | B8 | Alongside reliability and coverage |
| 15 | Open RomMFS from the RetroBat menu | B6 | After lifecycle helper; include launcher in packaging |
| 16 | Add conservative native RomM state interoperability | B5 | Last; reuse conflict handling |

These are delivery increments, not a single large rewrite. External capability
checks belong inside the relevant increment. Unsupported range responses use
full downloads; unknown layouts remain unmatched; missing optional API routes
disable only their feature. Windows acceptance belongs to the feature it
validates and does not prevent unrelated portable implementation from proceeding.

## Rules for implementation

- Extend the current owner of each invariant. Keep one download coordinator,
  one ordinary-save journal, and the existing actor/transfer-worker arrangement.
  Keep memory cards on their existing identity, consent, and API path.
- Use the existing hash, ZIP, SQLite, HTTP, XML, and Windows dependencies.
  Add small focused modules where needed; avoid a plugin registry, generic
  workflow engine, new async runtime, or broad controller refactor.
- Preserve metadata-only enumeration, original-file hydration, mounted
  snapshots, server/account scoping, worker draining, and stale-event rejection.
  New download commands are explicit user actions; browsing still downloads nothing.
- Preserve unowned, changed, busy, and redirected files. Discovery never
  enlarges approved save roots. Optional features start disabled and do not
  infer consent from installation or discovery.
- Use existing Settings, Save review, and activity surfaces. Domain decisions
  stay in the controller/actor. Buttons and keyboard actions use the same
  commands, stable IDs, visible progress/errors, and existing GPUI components.
- Add backward-compatible persistence fields/tables in the owning store.
  Existing databases open without resetting settings, queued saves, or backups.
  Write recovery intent before changing user save files.
- Implement lessons independently. Source any small emulator/firmware tables
  from the installed configuration or primary emulator documentation, recording
  provenance. Do not copy RomMBat implementation or bundled tables.

## 1 Verify ROM downloads and reject damaged cache entries

**Owner:** [download.rs](../crates/rommfs-core/src/download.rs),
[cache/index.rs](../crates/rommfs-core/src/cache/index.rs), and the existing
`VersionKey` produced by `catalogue.rs`.

`fetch_and_publish` currently checks the reported byte count before rename;
`ready_if_current` checks file existence/type and version, but not length.

- Interpret the existing checksum version forms (`sha1`, `md5`, `crc`) with
  the installed hashing crates. Hash the written bytes before publishing Ready;
  use bounded reads or a streaming writer, whichever keeps this path clearest.
  A timestamp/size version remains a size-only check. Malformed advertised
  checksums must produce a diagnostic rather than silently count as verified.
- Compare actual staged length as well as the expected length. On mismatch,
  remove the failed private artifact, fail every waiter, and allow a later retry.
- On cache lookup, compare actual length with the recorded size; invalidate a
  damaged row so the normal download path repairs it. Preserve version checks.
  Do not rehash every warm read or introduce a background scrubber.

**Proof:** Extend `tests/download.rs` with same-length wrong-content rejection
and a successful retry, covering the three checksum forms in one table. Extend
`tests/cache_index.rs` or the existing download repair test with a truncated
Ready file. Retain concurrent-reader and no-download-on-enumeration coverage.

## 2 Avoid repeated save-history body downloads

**Owner:** [incoming_transfer.rs](../crates/rommfs-app/src/save_sync_agent/incoming_transfer.rs),
`save_sync_agent/incoming.rs`, `actor.rs`, and `save_sync/journal.rs`.

`reconcile_attempt` downloads every compatible revision and retains only 16
bodies. Increasing that limit would leave restart traffic and unbounded history
growth unresolved.

- Persist verified revision fingerprints in the existing journal, scoped by
  server, account, installation/save root, game, slot, and remote ID. Include
  the metadata used for invalidation and the hash actually computed from bytes.
  Remote IDs and filenames alone are not content verification.
- Select required bodies from inventory metadata: the newest candidate,
  revisions needed to settle uncertain uploads, and specific review/export
  requests. Keep historical metadata without retaining every historical body.
  Preserve existing ambiguous-history and unsupported-record handling.
- Let the actor own journal reads/writes and pass plain records to the transfer
  worker. Represent a verified fingerprint separately from available body data;
  publication/export must fetch missing bytes and verify them before use.
- Invalidate fingerprints when metadata changes, identity/scope changes, or a
  revision disappears from a successful complete inventory. Failed inventory
  requests must not erase accepted baselines. Keep any body cache bounded.

**Proof:** Expand the existing repeated-reconciliation test to multiple games
and more than 16 revisions, then reopen the journal/restart the worker. Assert
unchanged polls do not repeat content requests; changing one revision fetches
that revision; requesting an uncached historical copy fetches and verifies it.
Reuse existing history-tie, uncertain-upload, and scope-isolation tests.

## 3 Resolve ordinary-save conflicts in Save review

**Owner:** [controller/commands.rs](../crates/rommfs-app/src/controller/commands.rs),
`save_sync_agent/actor.rs`, `incoming.rs`, `files.rs`,
`save_sync/journal/incoming.rs`, and `window/pages/save_review.rs`.

The journal already stages incoming copies and records publication intent.
`publish_changed_save` already checks the old content, keeps a backup, and
protects against a concurrent writer. Extend that path.

- Add two commands for an identified ordinary-save conflict: **Keep local**
  and **Use remote**. Carry the session, incoming revision, and observed local
  generation/hash so a stale review cannot resolve a different conflict.
- Revalidate scope, consent, mapping, local bytes, and current remote inventory.
  Changed evidence refreshes the review and preserves both copies.
- Serialize resolution with transfers for that file. Persist the chosen action
  and expected revisions before work starts; resume that intent after restart.
  Suspend obsolete queued uploads so they cannot undo the choice, retaining
  their recovery artifacts until the resolution is accepted.
- Keep local preserves the remote copy and queues the chosen local snapshot as
  a new revision through the existing upload worker. Clear the conflict only
  after verified upload/acknowledgement; a network failure remains pending.
- Use remote preserves the local file using the existing journaled backup and
  guarded replacement path. Accept the new baseline transactionally after
  publication. Interrupted operations remain recoverable and visible on restart.
- Keep Export as a non-resolving action. Show the selected file/revision and
  replacement consequence in the existing review details. Prevent duplicate
  submission and keep keyboard activation/focus working.

**Proof:** Actor/journal tests for both choices, successful restart persistence,
failed upload/publication, and local/remote/session changes during resolution.
Use the existing publication barriers and backup tests. One real UI walkthrough
proves each command reaches the selected conflict and reports its result.

## 4 Cancel stalled requests and individual CFAPI fetches

**Owner:** [romm/client.rs](../crates/rommfs-core/src/romm/client.rs),
`romm/saves.rs::wait_for_save`, `download.rs`, `filesystem.rs`, and
[cfapi/windows.rs](../crates/rommfs-cfapi/src/windows.rs).

- Generalize the existing cancellable HTTP wait only enough for ROM and
  metadata requests. Drop the request future/body on cancellation so waiting
  for headers or bytes terminates promptly. Keep the current thread model.
- Register `CANCEL_FETCH_DATA`; track each live request by its CFAPI request
  identity. A cancelled reader stops waiting and receives an aborted result.
- Give the shared download its own lifetime: cancel its HTTP operation when
  the last interested reader leaves, or immediately on mount/session shutdown.
  Cancelling one reader must not cancel bytes another reader still needs.
  Move the transfer into an owned per-flight worker if needed so cancellation
  can release the first reader too; do not detach it or add a general job pool.
- Drain requests and callbacks before freeing their context. Retain the
  existing no-partial-publication rule and typed cancellation outcome.

**Proof:** Fixture barriers for stalled headers and bodies, two readers with
one cancelled, last-reader cancellation, and shutdown with a pending request.
Assert bounded completion and request/publication outcomes without arbitrary
sleeps. Add one native CFAPI cancelled-read scenario to the Windows procedure.

## 5 Retry remembered connections after temporary outages

**Owner:** [controller/connection.rs](../crates/rommfs-app/src/controller/connection.rs)
and the existing controller worker deadline loop.

- Add one retry deadline and attempt counter to the worker. Use a fixed capped
  schedule, for example 1, 2, 5, 10, then 30 seconds between attempts while the
  remembered connection remains desired. Keep retry timing out of user settings.
- Retry transport failures and transient server failures. Authentication
  rejection becomes Sign in required; permission and malformed-contract errors
  retain their own actionable state. Keep remembered preferences on outages.
- Reauthenticate and verify the expected account before resuming work. Resume
  only the mount that was active/requested, or already authorized login startup,
  and only unchanged save consent. A configured folder alone is not auto-mount consent.
- Cancel retries on logout, account/server changes, explicit stop of the
  relevant requested operation, and shutdown. Carry the connection epoch through
  attempts and use item 4 to interrupt in-flight waits.

**Proof:** A deterministic deadline-driven controller test fails twice then
connects and resumes the authorized scope. Add authentication rejection and
logout/account-switch during retry cases. Reuse the existing remembered-state
and stale-event tests; no real-time backoff test suite.

## 6 Bound storage and expose safe cleanup

**Owner:** [cache/policy.rs](../crates/rommfs-core/src/cache/policy.rs),
`cache/index.rs`, `download.rs`, the CFAPI ownership adapter, connection
preferences, and the existing Settings page.

- Add one optional persisted ROM storage limit; an unset limit preserves the
  current age policy. Keep a fixed free-space reserve and check space before
  large transfers, even when no limit is configured.
- Extend the current eviction selection to reclaim oldest unused entries until
  the target is met. Reuse the same guarded removal for age, capacity, and an
  explicit **Free unused space** command. Busy/modified/unowned files stay put.
- Account for private cache bytes and hydrated mount bytes. Query allocation
  through the Windows adapter; show an estimate explicitly when exact allocation
  is unavailable. Calculate required space per volume: both copies on one drive
  need combined room; different drives need separate checks. Include active
  transfer reservations so concurrent downloads cannot all claim the same space.
- If safe cleanup cannot make enough room, fail that download clearly. Report
  usage, reclaimed bytes, and retained/busy files in existing UI status. Future
  pinned games from item 12 are excluded from routine cleanup.

**Proof:** Extend eviction tests with an over-budget case that frees oldest
eligible entries while retaining active files and failed removals. Test admission
with same/different volumes and simultaneous transfers using supplied capacity
values. Native acceptance checks real free-space reporting and owned dehydration.

## 7 Recognize measured emulator save layouts

**Owner:** [SaveFileRules](../crates/rommfs-core/src/save_sync/ordinary_files.rs),
existing catalogue metadata, and `tests/save_file_rules.rs`.

- Start with a short literal rule table for nested RetroArch/core directories
  inside the selected save root, beginning with the reported
  `snes/libretro.snes9x/<game>.*` shape. Restrict each row to its measured
  directory and save filename conventions; do not match any arbitrary ancestor.
- Add exact game-ID matching using the existing `title_id`/`save_target` metadata
  for a measured layout. Allow an exact emulator-title match only when the
  system, layout, and title produce one owner. Do not add fuzzy attribution or
  infer ownership from an ID-like substring.
- Preserve explicit associations as the override. Conflicting automatic
  evidence or multiple owners remains unmatched. Keep the association evidence
  as a small rule/source field usable in diagnostics; no separate rules engine.
- Retain the existing reversible file slot and tracked mapping across ROM
  renames. A renamed ROM without stable ownership evidence needs an explicit
  association. Preserve shared-card exclusions and empty/opaque saves.
- Record each enabled rule's actual RetroBat/emulator/core version, path,
  ownership evidence, round-trip result, and no-op resync in a current validation
  record. Candidate rules without native evidence stay explicitly unvalidated.

**Proof:** One table-driven mapping test covers the measured nested paths,
same-stem games, stable-ID rename, misleading ID, and unknown files. Existing
tests already cover explicit mappings, companions, empty files, and unsafe paths;
extend those only where behavior changes. Perform one real save round trip per
enabled emulator/core row using a legally available playable game.

## 8 Feed local EmulationStation events into existing workers

**Owner:** RetroBat setup/ownership code, a small native hook binary in the
existing app package, and `save_sync_agent/actor.rs`.

- Install optional owned hooks for `start`, `game-start`, `game-end`, and `quit`.
  The helper validates its inputs, writes one local record by temporary-file
  rename, and exits. It does no network work and starts no second sync engine.
  Preserve other scripts and modified hook files during setup/removal.
- Consume records in the resident app. Scope them to the installation and
  activation generation; deduplicate consumed event IDs durably. Retire an
  event only after recording the actor's resulting work so crash replay is safe.
- Use available launch arguments and a small reader for the installed launch
  log to resolve ROM/system/emulator/core. Track log identity/offset for rotation;
  ignore menu launches. Unknown launch identity means defer incoming writes
  for the affected scope, not guess a game.
- Mark active games and affected shared cards. Defer all incoming publication,
  including explicit resolution, while the emulator may retain saves in memory.
  Game-end schedules an observation after the current quiet period. Preserve
  watchers/fallback scans for missing hooks and direct emulator launches.
- Lost game-end/crash events leave publication deferred until local process
  inactivity is established; elapsed time alone is insufficient. Logout/session
  changes invalidate queued work. Hook write failures leave existing sync usable
  and produce a local diagnostic. Keep play counts on the current gamelist
  reconciliation path so these hints cannot double-count sessions.

**Proof:** Test record replay and crash-before-ack, duplicates/out-of-order or
missing events, stale scope, and failed writes. Test an active game temporarily
closing its file: no incoming replacement occurs until inactivity is confirmed.
Native acceptance covers paths with spaces, log rotation, menu launch exclusion,
script coexistence/removal, and launch/exit with the server unavailable.

## 9 Resolve BIOS requirements to verified destinations

**Owner:** [retrobat/auxiliary.rs](../crates/rommfs-core/src/retrobat/auxiliary.rs),
`romm/firmware.rs`, and `tests/auxiliary.rs`.

- Add a small requirement table before materialization: emulator/system,
  relative destination, accepted checksum, and provenance. Start with documented
  standalone PS1 BIOS requirements; support nested destinations in the same table.
- `FirmwareDto` currently has no hash fields. Add optional checksum fields only
  where the server contract supplies them; otherwise hash candidate downloads in
  private staging. Candidate names narrow work but never establish correctness.
  Verify downloaded bytes even when inventory advertises an accepted checksum.
- Resolve missing, ambiguous, already-present, and blocked destinations
  individually. Equal verified content can satisfy several requirements; differing
  equally ranked candidates remain ambiguous. Preserve the existing materializer,
  ownership index, user-file protections, and targeted Switch behavior.
- A matching unowned destination may satisfy a requirement without becoming
  RomMFS-owned. Wrong or modified user content is preserved. Acquire firmware
  only from the user's server. Keep unknown systems on existing placement behavior
  without claiming requirement verification; leave archive firmware out of this increment.

**Proof:** Extend auxiliary tests for a renamed source, same filename/wrong
hash, nested destination, duplicate accepted content, and a modified destination.
One native emulator launch verifies each initially supported requirement family.

## 10 Synchronize one explicitly grouped save layout

**Owner:** `save_sync/ordinary_files.rs`, `file_identity.rs`, the existing
journal and transfer worker, plus a focused grouped-save module.

- Start with one PPSSPP per-game directory under the configured PSP save root.
  Use exact game-ID ownership; exclude shared containers. Add a distinct versioned
  unit slot/profile so a unit cannot collide with ordinary files or memory cards.
- Snapshot that directory's exact membership as one revision using the installed
  ZIP and hashing libraries. Hash sorted relative member names and bytes, not ZIP
  timestamps. Verify membership/content stayed stable during capture.
- Upload/download through the ordinary-save revision API. Restore into a private
  staging directory with bounded extraction, canonical paths, no links/traversal,
  and verified member hashes. Reuse suitable archive checks from memory cards;
  do not merge their lifecycle or consent models.
- Publish only while inactive and with unchanged local evidence. Journal the
  old/new manifests, stage, backup, and publication progress. Prefer renaming the
  dedicated per-game directory with a retained backup over a general member-move
  transaction. Recover every interrupted rename state on restart without
  overwriting a concurrently created file/directory.
- A removed remote member disappears only from this explicitly approved unit;
  unrelated games remain untouched. Enable grouping explicitly and suspend
  member-level uploads for that unit, preserving prior remote revisions and
  queued artifacts until the first unit baseline is accepted.

**Proof:** A round-trip fixture checks content and membership with an unrelated
neighbor directory. A parameterized fault test interrupts each durable publication
boundary and reopens the journal, including cancellation and a concurrent local
change. Reject corrupt/traversing archives. Finish with one native PPSSPP round trip.

## 11 Resume verified original-file downloads

**Owner:** `romm/client.rs`, `download.rs`, and `cache/index.rs`.

- Add resumable state to the existing cache store, separate from Ready: original
  file identity/version, expected size, validator, and durable partial length.
  Startup retains only consistent, owned resumable artifacts. Account/session
  authorization is rechecked before issuing any request; partials are never served.
- Request ranges on `/api/roms/{file_id}/files/content/{file_name}` only with a
  suitable validator, preferring a strong ETag. Send `If-Range`; accept a `206`
  only when its validator, start offset, and total match the stored transfer.
- A full `200` truncates/restarts instead of appending. Missing validators,
  changed identity, malformed `206`, or `416` discard resume state and make at
  most one clean full retry for that attempt. Unsupported servers keep working.
- Rehash the retained prefix and complete file through item 1 before publication.
  Retain valid partials after interruption; remove corrupt/stale ones. Include
  partial storage in item 6 cleanup and keep single-flight/cancellation semantics.
- Capture live request/response evidence on the actual original-file route in
  the same increment. Advertise resume only for observed compatible responses;
  absence of support is the implemented full-download fallback.

**Proof:** A parameterized HTTP fixture covers interrupted/restarted transfer,
valid `206`, changed validator, full `200`, malformed range, and `416`. Assert
final bytes, requested offsets, and no partial publication. Reuse concurrent
reader tests; add restart recovery of a persisted partial.

## 12 Prepare selected games and support explicit offline access

**Owner:** Existing controller commands, catalogue snapshots, download/cache
stores, mount construction, and Settings/library selection UI.

Deliver this in two increments so preparation is useful before offline mounting.

- Add **Keep available** for an explicitly selected game. Enumerate every
  required original file in its current launchable catalogue layout, invoke the
  existing download manager, and persist the pinned selection by server/account
  and content identity. Ready means all required files are verified. A partial
  failure stays incomplete; removing the pin returns files to normal eviction.
- Expose selection through a small existing-style game list/picker; show progress
  and incomplete status. Keep downloads out of render/enumeration. Do not add a
  second library-management system or an independent download scheduler.
- Add **Allow offline access on this device**, off by default and granted only
  after online account verification. On a later transport outage, mount that
  account's saved snapshot with a cache-only source exposing complete pinned
  games. Missing/corrupt bytes fail locally without network fallback.
- This opt-in permits local access until explicit logout, account/server change,
  disabling offline access, or a known authentication revocation. Remote revocation
  cannot be detected while disconnected; state this consequence at opt-in.
  Authentication rejection never silently falls back to offline mode.
- Keep offline catalogue and pin authorization account-scoped even where private
  ROM bytes are shared by server/file key. Do not relax the authenticated mount
  path for ordinary sessions. Reuse mount-root ownership and snapshot checks.
- While offline, save uploads/downloads wait. Preserve local saves and existing
  journal artifacts; scan/reconcile only after the same account is reverified.
  No offline save-transfer engine is needed.

**Proof:** Test a multi-file game with one missing file, a pinned eviction
exclusion, restart with server down, and a zero-request successful offline read.
Test corrupt cache, account switch/logout, and explicit authentication rejection.
Native acceptance launches a prepared game after a disconnected restart.

## 13 Automate checks and produce a Windows release archive

**Owner:** New small CI workflows and packaging script, existing `mise.toml`,
[Windows acceptance guide](../docs/testing/windows.md), and test wizard.

- Add portable format/test/lint and host UI compilation jobs using the repository
  commands below. Add a native Windows workspace test job. Preserve test counts,
  failures, and skips in artifacts; a hosted server runner is not Windows 10/11
  client acceptance.
- Reuse the already-added `test-wizard.cmd` and PowerShell wizard for interactive
  acceptance/evidence collection. Add only new scenarios required by these items;
  do not build another harness or treat generated zero-filled ROMs as gameplay tests.
- Build a versioned Windows x64 ZIP containing the executable, owned helper(s),
  required runtime assets, license notices, concise setup instructions, and
  SHA-256 checksums. Use a single packaging script from CI and local builds.
  Start with ZIP distribution; omit an installer and self-updater.
- Exercise the exact archive on a clean Windows 10/11 client without Rust.
  Verify first launch, pairing, listing without downloads, hydration, and upgrade
  against copied existing settings/journals. Re-register owned hook/menu launcher
  paths when the app moves; never reset user data during upgrade.
- Save client acceptance evidence against the artifact version/commit. CI can
  produce a candidate archive while native acceptance is outstanding; mark a
  release tested only after that evidence exists. Preserve rollback copies of
  user databases before any migration that an older release cannot read.

**Proof:** Actual CI execution, packaging output inspection, one clean-client
smoke run, and one upgrade run. Run existing wizard tests when changing the wizard.
No tests that merely assert workflow YAML or documentation strings.

## 14 Show compatibility information and per-emulator evidence

**Owner:** `romm/client.rs`, connection/controller events, existing Settings or
activity details, `docs/contracts/romm-api.md`, and new dated validation records.

- Best-effort read the observed API version once per connection using the known
  `/openapi.json` source. Missing/unreadable diagnostics do not block a usable
  connection. Record optional capabilities from actual endpoint outcomes; do
  not probe write endpoints or infer compatibility from a version string alone.
- Distinguish transport failure, sign-in rejection, insufficient scope, absent
  endpoint, and unreadable payload. Report only the affected feature as unavailable.
  Reuse existing error variants and event/status presentation.
- Keep a compact validation table per system/emulator/core with versions,
  concrete paths, ownership, save/restore, firmware where relevant, and no-op
  resync outcomes. Add execution evidence when collected; preserve historical
  validation records and separate fixture evidence from native results.

**Proof:** Add fixture cases for optional diagnostics unavailable, unknown
version with working required routes, and an unavailable optional capability.
Use native evidence from items 7–10 rather than duplicating emulator test runs.

## 15 Open RomMFS from the RetroBat menu

**Owner:** RetroBat setup/metadata merge, the small native helper from item 8,
and [desktop.rs](../crates/rommfs-app/src/desktop.rs).

- Add an optional owned `system/es_menu/rommfs.menu` entry and merged gamelist
  entry. Put an owned launcher at the location required by the installed menu
  contract; record the main executable's absolute path instead of relocating it.
- Extend the helper with a launch mode if necessary. Start the normal executable
  so existing single-instance handling opens the current window. Pass arguments
  directly to process creation, preserving spaces and avoiding shell expansion.
- Repeated setup updates only unchanged owned artifacts. Removal preserves
  unrelated menu entries, user-edited metadata, and modified launchers. The first
  version opens the desktop UI; controller navigation is outside this shortcut.

**Proof:** Setup twice/removal with unrelated and modified entries, plus a native
menu launch with spaces in paths and an already-running hidden instance.

## 16 Add conservative native RomM state interoperability

**Owner:** A focused state-template/parser module, a small `romm/states.rs`
client, and the existing journal, transfer worker, and review surface.

- Make native state integration an explicit opt-in. Read the installed
  `es_savestates.cfg` for one initial supported emulator/core. Match complete
  templates, exact slot digits, and documented screenshot companions; `.dsv`
  battery saves must not match `.ds<digit>` states.
- Persist distinct state identity including game, emulator, core, relative path,
  and slot. Encode a collision-free remote filename within the observed API
  limits; retain emulator/core version information for restore compatibility.
- Use unique append-only uploads with automatic cleanup/overwrite disabled
  where the state contract supports it. Re-list and verify before accepting a
  baseline. If the endpoint cannot preserve existing versions, keep local state
  snapshots and report upload unavailable rather than overwrite another revision.
- Stage incoming states and screenshots for deliberate review/restore. Use
  inactivity checks, backups, and the guarded publication path; never promise
  ordinary-save conflict guarantees from an endpoint without those guarantees.
  Concurrent remote changes keep the review pending.
- Exclude opted-in state paths from ordinary-file uploads to avoid duplicate
  transfer routes. Preserve old ordinary-save revisions; no automatic migration
  or deletion. Unsupported servers/configurations keep ordinary-file behavior.

**Proof:** One parser table includes battery/state separation and exact slot
widths. Fixture tests cover two cores with identical local filenames, screenshots,
concurrent remote change, and an endpoint that cannot append safely. Complete one
native state round trip for the supported emulator/core.

## Verification and completion

For each increment, run the affected existing test binary/module serially and
add only the behavioral cases listed above. Use fixture request counts for
network claims, barriers for races/cancellation, and actual bytes plus reopened
journals for persistence claims. Do not count a filter matching zero tests as
success. Avoid source-shape assertions, mock call choreography, broad UI
snapshots, synthetic benchmarks, and tests of prose/constants.

At the end of each completed batch, run the repository gates once:

```sh
cargo fmt --all --check
cargo test --locked -p rommfs-core -p rommfs-fixture -p rommfs-cfapi -p rommfs-app --no-default-features -- --test-threads=1
cargo clippy --locked -p rommfs-core -p rommfs-fixture -p rommfs-cfapi -p rommfs-app --no-default-features --all-targets -- -D warnings
cargo check --locked -p rommfs-app --all-targets
```

On Windows, also run the affected native tests and the workspace gate:

```sh
cargo test --locked --workspace -- --test-threads=1
```

Update canonical architecture/API/user documentation only for behavior actually
delivered. Add a new dated validation record for new execution evidence. Keep
the staged source reports and Windows wizard work intact, and leave archived
plans unchanged. Each handoff records the user-visible result, focused proof,
and any native scenario still unexecuted; a successful Linux run never stands
in for native CFAPI or emulator acceptance.
