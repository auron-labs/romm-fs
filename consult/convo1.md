# Session: romm\_fs

<session id="9d37bbd8-bbbc-4344-ab8a-4a7faabe6c42" project="romm_fs" agent="Claude" started_at="2026-10-27T09:03:54.352Z" ended_at="2026-10-27T09:07:35.569Z" message_count="32">
<message role="user" ordinal="0" timestamp="2026-10-27T09:03:54.674Z">
I need this project finished in two days. How do we get there? I can&#39;t come up with the reason why, but I just dislike using this project. It&#39;s confusing, it&#39;s slow, the interface has conflicting state. How do we get this finished? Don&#39;t make any changes.
</message>
<message role="assistant" ordinal="1" timestamp="2026-10-27T09:04:01.119Z" has_thinking="true" has_tool_use="true">
<thinking><![CDATA[
Since the user wants a two-day plan without actual changes, I need to explore the codebase read-only to ground my recommendations — checking the repo structure, docs, README, and UI/controller state logic. I should also recall relevant constraints from memory, like never touching user ROMs and maintaining RomM feature parity.
]]></thinking>
<tool_call id="toolu_01WwPnyGA35aANZDmQjE1Vde" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"cat README.md | head -120 && ls docs docs/* .scratch 2>/dev/null && git log --oneline | wc -l && git log --oneline -30","description":"Read README, docs listing, and git history"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ cat README.md | head -120 && ls docs docs/* .scratch 2>/dev/null && git log --oneline | wc -l && git log --oneline -30
]]></tool_body>
<tool_result><![CDATA[
# RomMFS

RomMFS makes your RomM games available as a Windows folder. Connect your
server, choose a folder, and use your games in RetroBat or another emulator.
Game files download when opened and are cached for later use.

- **RomM** stores your library on the server.
- **RomMFS** connects that library to this PC and keeps the games folder available.
- **RetroBat or your emulator** browses and plays the games.

Artwork updates and optional save sync run alongside the games folder. Save
sync is off until you enable it. The project is under active development.

## Quick start

Requirements and detailed setup are in the [user guide](docs/user-guide.md).
For a local Windows build, install Rust 1.89 or later with the MSVC toolchain
and Windows SDK, then run:

```powershell
cargo run --locked -p rommfs-app
```

1. Enter your RomM server address and choose **Sign in with browser…**.
   Approve the connection in RomM.
2. Choose an empty folder on a local NTFS drive for your games.
3. For RetroBat, follow **Settings → RetroBat** to connect your installation.
   See the [RetroBat setup guide](docs/user-guide.md#set-up-retrobat).
4. On **Overview**, choose **Make games available**. When the status becomes
   **Available**, open RetroBat or point your emulator at the games folder.

Keep RomMFS running while you play. The first read downloads a game file;
later reads use cached bytes. Game files in this folder are read-only.
See [everyday use](docs/user-guide.md#everyday-use) for saves, updates, and
what the status messages mean.

## Workspace

| Crate | Responsibility |
|---|---|
| `rommfs-core` | RomM client, catalogue, cache, save logic, and RetroBat setup/metadata. |
| `rommfs-fixture` | In-process HTTP fixture used by portable tests. |
| `rommfs-cfapi` | Windows Cloud Files adapter and portable mount-root ownership checks. |
| `rommfs-app` | GPUI desktop UI, controller, session, and save-sync worker. |

## Project docs

- [User guide](docs/user-guide.md): requirements, Generic folders, RetroBat, save sync, and limitations.
- [Development guide](docs/development.md): build and check commands, crate/task map, and where tests live.
- [Architecture](docs/architecture.md): current crate boundaries, data flow, and invariants.
- [RomM API contract](docs/contracts/romm-api.md): verified and source-backed API behavior, with evidence boundaries.
- [Windows testing](docs/testing/windows.md): native acceptance procedure and report format.
- [Dated validation record](docs/validation/2026-10-12.md): implementation status and recorded Linux/Windows evidence.

## Testing wizard

Contributors can run `test-wizard.cmd` on Windows for guided checks and local
evidence collection. See [Windows testing](docs/testing/windows.md) for the
procedure, resume/skip controls, and screenshot privacy information.
docs/architecture.md
docs/development.md
docs/user-guide.md
docs/ux-remediation.md

.scratch:
design
human-testing-2026-10-26

docs:
architecture.md
contracts
development.md
research
testing
user-guide.md
ux-remediation.md
validation

docs/contracts:
romm-api.md

docs/research:
rommbat-assessment-2026-10-16.md

docs/testing:
save-sync-helper.md
windows.md

docs/validation:
2026-10-12.md
127
9492a7e feat(ui): clarify app status and setup guidance
6f46a7c Merge pull request #6 from auron-labs/linux-fuse-redesign
c3b6a01 test: update stale gamelist-published log assertion
733a9d8 feat: apply new dark shell design across app pages
f62096f feat: add read-only FUSE mount backend for Linux testing
fea973b chore: cleanup
40e59cf chore(agents): add workflow skills and config
360e13a feat: improve windows
52513e8 feat: improve save sync review and shutdown handling
55b5c10 chore: human-testing
84068e6 chore: add wizard
4b95a63 feat: finish human-test fixes and memory-card sync
230b9a1 feat: sync favourites and play history with RomM
a822d74 feat(app): reorganize Settings into tabs and trim page copy
5a7be56 feat: fix ui issues with scrolling
a94965c docs: describe RetroBat coexistence and metadata recovery
44b478a feat: merge RetroBat gamelists with recoverable identity state
0ab7054 feat: link RomM games alongside existing RetroBat libraries
0c775ba feat: improve metadata sync
afbd1ec fix: allow frontend metadata publication without an identity index
007f896 feat: add windows uac elevation
4c19cab Merge branch 'fix/retrobat-clean-install-ux'
67bea4a refactor: clarify module and fixture API names
508641c refactor: clarify module and fixture API names
7b80f62 docs(testing): extract Windows evidence helper
5831969 refactor(app): organize window pages and controls
745dcd9 test(app): split save sync and controller suites
9013f84 refactor(app): split controller into focused modules
61ca844 docs: organize project guides and agent navigation
b61b0f5 refactor(core): separate legacy GB save profile
]]></tool_result>
<tool_result tool_call_id="toolu_01WwPnyGA35aANZDmQjE1Vde" source="tool_result" timestamp="2026-10-27T09:04:02.038Z"><![CDATA[
# RomMFS

RomMFS makes your RomM games available as a Windows folder. Connect your
server, choose a folder, and use your games in RetroBat or another emulator.
Game files download when opened and are cached for later use.

- **RomM** stores your library on the server.
- **RomMFS** connects that library to this PC and keeps the games folder available.
- **RetroBat or your emulator** browses and plays the games.

Artwork updates and optional save sync run alongside the games folder. Save
sync is off until you enable it. The project is under active development.

## Quick start

Requirements and detailed setup are in the [user guide](docs/user-guide.md).
For a local Windows build, install Rust 1.89 or later with the MSVC toolchain
and Windows SDK, then run:

```powershell
cargo run --locked -p rommfs-app
```

1. Enter your RomM server address and choose **Sign in with browser…**.
   Approve the connection in RomM.
2. Choose an empty folder on a local NTFS drive for your games.
3. For RetroBat, follow **Settings → RetroBat** to connect your installation.
   See the [RetroBat setup guide](docs/user-guide.md#set-up-retrobat).
4. On **Overview**, choose **Make games available**. When the status becomes
   **Available**, open RetroBat or point your emulator at the games folder.

Keep RomMFS running while you play. The first read downloads a game file;
later reads use cached bytes. Game files in this folder are read-only.
See [everyday use](docs/user-guide.md#everyday-use) for saves, updates, and
what the status messages mean.

## Workspace

| Crate | Responsibility |
|---|---|
| `rommfs-core` | RomM client, catalogue, cache, save logic, and RetroBat setup/metadata. |
| `rommfs-fixture` | In-process HTTP fixture used by portable tests. |
| `rommfs-cfapi` | Windows Cloud Files adapter and portable mount-root ownership checks. |
| `rommfs-app` | GPUI desktop UI, controller, session, and save-sync worker. |

## Project docs

- [User guide](docs/user-guide.md): requirements, Generic folders, RetroBat, save sync, and limitations.
- [Development guide](docs/development.md): build and check commands, crate/task map, and where tests live.
- [Architecture](docs/architecture.md): current crate boundaries, data flow, and invariants.
- [RomM API contract](docs/contracts/romm-api.md): verified and source-backed API behavior, with evidence boundaries.
- [Windows testing](docs/testing/windows.md): native acceptance procedure and report format.
- [Dated validation record](docs/validation/2026-10-12.md): implementation status and recorded Linux/Windows evidence.

## Testing wizard

Contributors can run `test-wizard.cmd` on Windows for guided checks and local
evidence collection. See [Windows testing](docs/testing/windows.md) for the
procedure, resume/skip controls, and screenshot privacy information.
docs/architecture.md
docs/development.md
docs/user-guide.md
docs/ux-remediation.md

.scratch:
design
human-testing-2026-10-26

docs:
architecture.md
contracts
development.md
research
testing
user-guide.md
ux-remediation.md
validation

docs/contracts:
romm-api.md

docs/research:
rommbat-assessment-2026-10-16.md

docs/testing:
save-sync-helper.md
windows.md

docs/validation:
2026-10-12.md
127
9492a7e feat(ui): clarify app status and setup guidance
6f46a7c Merge pull request #6 from auron-labs/linux-fuse-redesign
c3b6a01 test: update stale gamelist-published log assertion
733a9d8 feat: apply new dark shell design across app pages
f62096f feat: add read-only FUSE mount backend for Linux testing
fea973b chore: cleanup
40e59cf chore(agents): add workflow skills and config
360e13a feat: improve windows
52513e8 feat: improve save sync review and shutdown handling
55b5c10 chore: human-testing
84068e6 chore: add wizard
4b95a63 feat: finish human-test fixes and memory-card sync
230b9a1 feat: sync favourites and play history with RomM
a822d74 feat(app): reorganize Settings into tabs and trim page copy
5a7be56 feat: fix ui issues with scrolling
a94965c docs: describe RetroBat coexistence and metadata recovery
44b478a feat: merge RetroBat gamelists with recoverable identity state
0ab7054 feat: link RomM games alongside existing RetroBat libraries
0c775ba feat: improve metadata sync
afbd1ec fix: allow frontend metadata publication without an identity index
007f896 feat: add windows uac elevation
4c19cab Merge branch 'fix/retrobat-clean-install-ux'
67bea4a refactor: clarify module and fixture API names
508641c refactor: clarify module and fixture API names
7b80f62 docs(testing): extract Windows evidence helper
5831969 refactor(app): organize window pages and controls
745dcd9 test(app): split save sync and controller suites
9013f84 refactor(app): split controller into focused modules
61ca844 docs: organize project guides and agent navigation
b61b0f5 refactor(core): separate legacy GB save profile
]]></tool_result>
<tool_call id="toolu_01MGHfYzjju3M4dwFBKuVhhn" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"find crates -name '*.rs' | xargs wc -l | sort -n | tail -50","description":"List largest Rust source files by line count"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ find crates -name '*.rs' | xargs wc -l | sort -n | tail -50
]]></tool_body>
<tool_result><![CDATA[
    455 crates/rommfs-core/tests/retrobat_metadata.rs
    457 crates/rommfs-core/tests/download.rs
    464 crates/rommfs-core/tests/retrobat_setup.rs
    467 crates/rommfs-app/src/save_sync_agent/incoming.rs
    475 crates/rommfs-core/src/catalogue.rs
    482 crates/rommfs-core/tests/memory_cards.rs
    490 crates/rommfs-core/src/retrobat/auxiliary.rs
    493 crates/rommfs-core/src/save_sync/installation.rs
    500 crates/rommfs-app/src/save_sync_agent/tests/ordinary.rs
    507 crates/rommfs-core/src/retrobat/setup.rs
    507 crates/rommfs-core/src/save_sync/preview.rs
    522 crates/rommfs-core/src/romm/saves.rs
    534 crates/rommfs-core/src/save_sync/settings.rs
    536 crates/rommfs-core/src/romm/client.rs
    537 crates/rommfs-core/src/retrobat/installer.rs
    540 crates/rommfs-app/tests/controller/session_and_downloads.rs
    549 crates/rommfs-app/src/controller/tests/save_sync.rs
    573 crates/rommfs-cfapi/tests/cfapi_native.rs
    584 crates/rommfs-core/src/retrobat/play_data.rs
    589 crates/rommfs-core/src/save_sync/journal/tests.rs
    593 crates/rommfs-app/src/controller/save_sync_setup.rs
    594 crates/rommfs-app/src/window/components.rs
    597 crates/rommfs-app/src/save_sync_agent/memory_cards.rs
    623 crates/rommfs-core/src/save_sync/journal/mapping.rs
    630 crates/rommfs-app/src/controller/tests/catalogue.rs
    636 crates/rommfs-core/tests/romm_client.rs
    662 crates/rommfs-core/src/save_sync/journal/incoming.rs
    672 crates/rommfs-app/src/controller/tests/connection.rs
    695 crates/rommfs-core/src/events.rs
    723 crates/rommfs-core/src/save_sync/scheduler.rs
    732 crates/rommfs-core/src/retrobat/metadata.rs
    764 crates/rommfs-core/src/save_sync/legacy_gb_profile.rs
    776 crates/rommfs-app/src/controller/save_files.rs
    801 crates/rommfs-app/src/save_sync_agent/tests/uploads.rs
    804 crates/rommfs-app/src/save_sync_agent/files.rs
    833 crates/rommfs-core/src/save_sync/memory_cards.rs
    836 crates/rommfs-app/src/controller/retrobat_setup.rs
    938 crates/rommfs-core/src/save_sync/journal/snapshot.rs
   1044 crates/rommfs-app/src/window/pages/save_review.rs
   1052 crates/rommfs-app/src/save_sync_agent/transfer.rs
   1085 crates/rommfs-cfapi/src/root.rs
   1101 crates/rommfs-app/src/window.rs
   1113 crates/rommfs-app/src/window/pages/settings.rs
   1114 crates/rommfs-app/src/window/pages/activity.rs
   1217 crates/rommfs-cfapi/src/windows.rs
   1252 crates/rommfs-fixture/src/lib.rs
   1447 crates/rommfs-app/src/controller/state.rs
   1509 crates/rommfs-app/src/controller.rs
   1612 crates/rommfs-app/src/save_sync_agent/actor.rs
  55669 total
]]></tool_result>
<tool_result tool_call_id="toolu_01MGHfYzjju3M4dwFBKuVhhn" source="tool_result" timestamp="2026-10-27T09:04:01.963Z"><![CDATA[
    455 crates/rommfs-core/tests/retrobat_metadata.rs
    457 crates/rommfs-core/tests/download.rs
    464 crates/rommfs-core/tests/retrobat_setup.rs
    467 crates/rommfs-app/src/save_sync_agent/incoming.rs
    475 crates/rommfs-core/src/catalogue.rs
    482 crates/rommfs-core/tests/memory_cards.rs
    490 crates/rommfs-core/src/retrobat/auxiliary.rs
    493 crates/rommfs-core/src/save_sync/installation.rs
    500 crates/rommfs-app/src/save_sync_agent/tests/ordinary.rs
    507 crates/rommfs-core/src/retrobat/setup.rs
    507 crates/rommfs-core/src/save_sync/preview.rs
    522 crates/rommfs-core/src/romm/saves.rs
    534 crates/rommfs-core/src/save_sync/settings.rs
    536 crates/rommfs-core/src/romm/client.rs
    537 crates/rommfs-core/src/retrobat/installer.rs
    540 crates/rommfs-app/tests/controller/session_and_downloads.rs
    549 crates/rommfs-app/src/controller/tests/save_sync.rs
    573 crates/rommfs-cfapi/tests/cfapi_native.rs
    584 crates/rommfs-core/src/retrobat/play_data.rs
    589 crates/rommfs-core/src/save_sync/journal/tests.rs
    593 crates/rommfs-app/src/controller/save_sync_setup.rs
    594 crates/rommfs-app/src/window/components.rs
    597 crates/rommfs-app/src/save_sync_agent/memory_cards.rs
    623 crates/rommfs-core/src/save_sync/journal/mapping.rs
    630 crates/rommfs-app/src/controller/tests/catalogue.rs
    636 crates/rommfs-core/tests/romm_client.rs
    662 crates/rommfs-core/src/save_sync/journal/incoming.rs
    672 crates/rommfs-app/src/controller/tests/connection.rs
    695 crates/rommfs-core/src/events.rs
    723 crates/rommfs-core/src/save_sync/scheduler.rs
    732 crates/rommfs-core/src/retrobat/metadata.rs
    764 crates/rommfs-core/src/save_sync/legacy_gb_profile.rs
    776 crates/rommfs-app/src/controller/save_files.rs
    801 crates/rommfs-app/src/save_sync_agent/tests/uploads.rs
    804 crates/rommfs-app/src/save_sync_agent/files.rs
    833 crates/rommfs-core/src/save_sync/memory_cards.rs
    836 crates/rommfs-app/src/controller/retrobat_setup.rs
    938 crates/rommfs-core/src/save_sync/journal/snapshot.rs
   1044 crates/rommfs-app/src/window/pages/save_review.rs
   1052 crates/rommfs-app/src/save_sync_agent/transfer.rs
   1085 crates/rommfs-cfapi/src/root.rs
   1101 crates/rommfs-app/src/window.rs
   1113 crates/rommfs-app/src/window/pages/settings.rs
   1114 crates/rommfs-app/src/window/pages/activity.rs
   1217 crates/rommfs-cfapi/src/windows.rs
   1252 crates/rommfs-fixture/src/lib.rs
   1447 crates/rommfs-app/src/controller/state.rs
   1509 crates/rommfs-app/src/controller.rs
   1612 crates/rommfs-app/src/save_sync_agent/actor.rs
  55669 total
]]></tool_result>
</message>
<message role="assistant" ordinal="4" timestamp="2026-10-27T09:04:05.692Z" has_thinking="true" has_tool_use="true">
<thinking><![CDATA[
No doc needed here—I should keep digging. Let me check the ux-remediation docs, the human-testing notes from October, the validation record, and the controller state to get a fuller picture.
]]></thinking>
<tool_call id="toolu_01CwhTte1nRYTfjWvuvknC2J" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"cat docs/ux-remediation.md; ls -la .scratch/design .scratch/human-testing-2026-10-26","description":"Read UX remediation doc and list scratch notes"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ cat docs/ux-remediation.md; ls -la .scratch/design .scratch/human-testing-2026-10-26
]]></tool_body>
<tool_result><![CDATA[
# UI and UX remediation

## Product polish — 26 October 2026

This pass improves the existing working application without requiring another
human test round before making UX changes.

- Overview now uses readable, full-width sections for games, RetroBat, saves,
  and artwork. Primary actions wrap at smaller window sizes; library counts
  and optional details no longer compete with the main task.
- Artwork checking, publication, waiting for file access, failure, and success
  share one event-derived summary across Overview and Activity. Completing
  image checks no longer looks like completing the whole update. Waiting has
  an explanation and no animated progress bar.
- Errors use the shared plain-language explanations with full diagnostics
  behind Technical details. Folder settings opens the actual settings page;
  setup and locked settings explain the next available action.
- The window restores focus to the visible tab order when a page change
  removes the focused control. Native Tab and Enter navigation now survives
  transitions from setup to Overview and Settings.
- The README explains the roles of RomM, RomMFS, and RetroBat. The user guide
  starts with everyday tasks; the architecture guide explains how to follow a
  user action through the code.

Verification: **385 portable tests passed**, including new coverage for
artwork publication/waiting/failure transitions and the unverified-save
explanation. Formatting, host UI compilation, portable Clippy, and UI-enabled
Clippy passed. Rendered the native GPUI screens in an isolated Linux/Xvfb
source copy with seeded state at 1160×920 and 720×480. Checked disclosures,
folder-settings navigation, and keyboard activation with assertions, and
inspected setup, Overview, Saves, Activity, Settings, and artwork progress,
waiting, and failure states.

The preview used isolated settings and an inert event drain. These checks
cover presentation and navigation; they are not a new Windows runtime run.
They do not establish the cause of file locks, deleted RetroBat links, or the
reported Windows latency. Mount ownership, lazy ROM reads, save consent, and
worker behavior retain their existing contracts.

## Implementation progress — 17 October 2026

Current step: implementation complete; native Windows acceptance remains outstanding. This record describes the working tree
based on `55b5c104953a16dd9e3234bdd8588a8ff450eb9e`; it is not native Windows
acceptance. All save scenarios use temporary copies and the in-process fixture.
Live consent and live server data have not been changed.

### Implementation and ownership

- `crates/rommfs-app/src/controller/state.rs` owns event-derived folder,
  consent, scan and save summaries. `window.rs` drains events on the existing
  UI timer and notifies the visible page; `session.rs` retains application
  ownership independently of the window.
- `window/actions.rs` and `window/folder_controls.rs` dispatch Start, Stop,
  library checking and applying library changes. Start prepares placeholders;
  Stop disconnects the provider and cleans verified owned entries while
  retaining private cache and ordinary frontend metadata. Applying library
  changes runs that existing stop/start sequence. Catalogue checks stage a
  new snapshot separately from the active mount.
- `save_sync_agent/actor.rs` emits authoritative queue snapshots. A local
  watcher hint can finish without a new snapshot; that drain now emits its
  own queue update. Current per-save work comes from scheduler, dispatched
  revisions, reconciliation and journal baselines. Retained review versions
  are not queued downloads. Completed stages share revision identity in UI
  history and do not create work on restart.
- `controller/save_sync_setup.rs` owns persisted consent and save checks.
  Check for changes rediscovers/reconciles using the existing consent; it can
  transfer saves when consent is On and cannot enable consent when Off.
  Session IDs reject old worker results. An explicit pending consent change
  cannot be undone by the previous session's discovery response.
- `window/pages/overview.rs`, `save_review.rs`, `activity.rs` and `settings.rs`
  compose the four destinations. Supported save identities stay separate;
  remote review records nest beneath their owning identity. Skipped files
  have a separate disclosure; recorded replacement backups use journal
  evidence for their label, rather than filename-based ownership claims.
- `desktop.rs` opens the configured absolute folder through the native shell
  with a separate path argument and reports failure inline. It never falls
  back to the home or working directory.
- `Controller::begin_shutdown` requests cleanup once and transfers its join
  handle. Explicit tray Quit shows the closing state and joins off the UI
  thread before exiting. Window close still hides to the tray when a tray
  exists. The app-quit fallback also waits for cleanup off-thread.
- `controller/retrobat_setup.rs` runs metadata publication. Core setup-record
  inspection previously reapplied the save-path reparse guard to the active
  CFAPI root. Metadata now uses `linked_systems_with_root_check` with the
  existing CFAPI ownership/native-root check. Ordinary setup and all save
  paths retain strict validation. The publisher still stages identity state,
  writes atomically, and preserves existing files on failure.
- Favourites/history fetch failures are surfaced separately from artwork
  publication and consent. A 403 is described as denied access; the UI does
  not infer an expired token or change credentials/permissions.
- ROM download reporting reuses existing `DownloadStarted`, `DownloadProgress`
  and terminal events. Clear completed downloads only removes successful UI
  rows. Diagnostic logs, journals, backups and remote revisions are untouched.
  The technical log is collapsed by default and stops following when a user
  scrolls away from the bottom. This reduces default render work; it is not
  evidence of the cause of the reported Windows hover lag.

### Verification and limits

- Baseline host UI compilation passed; the historic undefined `gamelist`,
  `LocalFree` and `NoopHydratedRemover` errors did not reproduce.
- Added behavior coverage for revision-stage identity, newer pending work,
  consent during checks, stale scan results, zero-file successful scans,
  review while Off, Stop/closing transitions, structural skips, unchanged
  watcher-hint draining, and backend-specific metadata root validation.
- Final portable suite: **383 passed, zero failures**, using the repository's
  serial `cargo test --locked -p rommfs-core -p rommfs-fixture -p rommfs-cfapi
  -p rommfs-app --no-default-features -- --test-threads=1` command. Localhost
  fixture sockets required execution outside the filesystem/network sandbox.
- `cargo fmt --all --check`, native-host UI `cargo check --locked -p rommfs-app
  --all-targets`, portable Clippy for all four crates with `-D warnings`, and
  UI-enabled app Clippy with `-D warnings` passed. Checks used the existing
  lockfile and installed dependencies, with build output isolated under `/tmp`.
- Rendered production pages in a temporary Linux/Xvfb source copy with seeded
  state and isolated settings. Inspected Overview, Saves, Activity and Settings,
  preparing/stopping/off/closing states, and one affected save with six remote
  review versions. Inspected 1162×952 and 720×480 logical windows and simulated
  X11 scaling at 125%/150%. Pointer expansion followed by Enter collapse kept
  visible row focus. Local captures are in `.scratch/archived/ux-remediation-preview/`.
  The preview's event drain was inert, so these screenshots do not validate
  real worker completion, native shell behavior, or Windows accessibility.
- Source inspection confirmed that legacy `save_sync_profile_version` comes
  from the installation's `system/version.info`; the generic-file profile
  instead uses an `ordinary files` marker. The ambiguous "checked" header was
  removed, and Settings no longer presents that save-profile value as the
  selected RetroBat installation's version.
- Catalogue omission totals mix whole-game and individual-file omissions.
  Library details now states those units and the actual missing-data,
  destination, name and path categories; it does not call them missing games
  or harmless assets.
- Native Windows CFAPI reparse tags, actual Explorer launch, close/tray/Quit
  behavior, 125%/150% scaling, and the reported hover latency require the
  Windows procedure in [testing/windows.md](testing/windows.md). Linux
  previews and portable tests cannot establish those facts.
- No new remote-adoption or force-sync path was added. Whether arbitrary
  browser-uploaded or renamed saves should be accepted remains a separate
  product decision; the existing protective rejection remains in force.

## Implementation brief

The original requested brief is retained below for the remaining native checks.

### RomM-FS UX remediation plan

## Objective

Improve the existing Windows application so a user can quickly tell whether their games are available, whether supported saves are up to date, what is happening, and what action they need to take. Keep the existing dark appearance and four main destinations. Prioritise truthful status, clear controls, and understandable recovery before cosmetic polish.

This is an implementation brief for a coding agent. Work through the implementation steps in order and complete one coherent change at a time. The intended result is an improved working application, with evidence from the affected journeys.

### Scope and evidence

- Keep the existing Rust application, UI toolkit, Windows CFAPI backend, lazy ROM access, and RetroBat integration. Alternative filesystem backends and other operating systems are outside this change.
- Reuse existing capabilities. Do not add a game browser, launcher replacement, cache manager, new save formats, a new sync engine, or a general framework for status/events. Small shared presentation helpers and changes to incorrect event handling are appropriate.
- Base this work on the supplied `report.md` dated 17 October 2026 and the 18 attached screenshots. The report references 24 screenshots; six are not in this attachment set. Settings and initial setup are not pictured, so recommendations for those surfaces require inspection of the current application.
- Screenshots establish displayed states. The report also records user-observed failures and partial functional testing. It does not establish the cause of lag, prove cache reuse, or demonstrate a failure of the normal two-sided conflict algorithm.
- Preserve local saves, backups, remote revisions, consent settings, and existing path-validation protections. UX work must not silently adopt an unverified remote save or make a failed operation look successful.
- Use an isolated test configuration, fixtures, and test copies for reproduction that could transfer or replace saves. The report leaves final live consent unknown; its server/path examples do not authorise live syncing. For any separately authorised live testing, inspect current consent and account for transfers that startup or restart may trigger. Leave live settings unchanged when preparing isolated scenarios.

## Findings to address

Evidence references such as S01 identify filenames in the evidence index at the end.

| Priority | Observed problem                                                                                                                                             | Required outcome                                                                                                                                                       | Evidence                                              |
| -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------- |
| P0       | Save headers say `Syncing saves…` or `2 outgoing` while completed `verified` events are visible. The state also persists after restart.                      | Current work comes from authoritative operation state. Completed history cannot create pending work. Preserve genuine queued work.                                     | S07–S09, S14–S15; report finding 7                    |
| P0       | The Start button disappears while preparing. Stop reportedly appears ineffective until changing pages; Quit reportedly leaves an unresponsive taskbar entry. | Stable controls acknowledge the operation immediately, prevent duplicate requests, and update on completion without navigation. Shutdown continues to perform cleanup. | S01–S03; report findings 4–5                          |
| P0       | A rejected remote revision produces one affected save plus six `Incoming` history rows and `6 incoming · 1 need review`.                                     | Show one affected save, an accurate protective-pause explanation, and the remote records under its details. Do not imply six downloads are queued.                     | S16–S18                                               |
| P1       | `Linked` describes association while a user needs to know whether their progress reached RomM. Hashes dominate healthy rows.                                 | Game title and current save status lead each row. Mapping and diagnostic information are secondary.                                                                    | S10–S12, S15                                          |
| P1       | `3 unmatched` refers to rows shown as excluded memory-card locations. App-created backups receive the same explanation as unsupported memory cards.          | Count eligible unlinked saves separately from skipped files. Explain each actual exclusion correctly.                                                                  | S10–S12, S17                                          |
| P1       | `No save scan yet` remains visible after files have been discovered and synced.                                                                              | Show the correct scan/check time, or acknowledge that a historic timestamp is unavailable.                                                                             | S10–S12, S15                                          |
| P1       | The folder is `ready` on Overview, `running` in Activity, and `mounting` while preparing. `Update ready` and `Restart folder` omit the purpose and effect.   | Use the vocabulary and action contracts below across all screens.                                                                                                      | S02–S06, S13, S16                                     |
| P1       | Overview displays a raw reparse-point exception beside green availability indicators.                                                                        | Explain the affected capability and impact. Keep games availability, game-information updates, and server permissions distinct.                                        | S04–S06; report findings 1–2                          |
| P1       | Open games folder reportedly opens the home directory.                                                                                                       | Open exactly the configured managed folder, with a visible error if that cannot be done.                                                                               | Report finding 3; not reproduced by these screenshots |
| P2       | Large logs, UUIDs, repeated stage rows, unexplained badges, and a full-width Review saves button compete with primary tasks.                                 | Clear hierarchy, concise summaries, and progressive disclosure of technical information.                                                                               | S01, S07–S12, S17–S18                                 |

## Product model and navigation

Use one sentence to explain the product near the games-folder controls or in existing setup help:

> Your RomM games appear in this folder. Game data downloads when an emulator or another program reads a game file.

The folder being available does not mean every game has been downloaded or that offline play is guaranteed. Reading ROM contents can trigger CFAPI hydration; the application must not read ROM contents merely to render status, calculate a UI summary, or inspect its own catalogue. Microsoft documents this distinction in its [CFAPI overview](https://learn.microsoft.com/en-us/windows/win32/cfApi/build-a-cloud-file-sync-engine).

Retain four destinations, renaming the current **Save sync** destination to **Saves**:

| Destination | Question it answers                                       | Content                                                                                                      |
| ----------- | --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| Overview    | Are my games available, and does anything need attention? | Games folder and primary controls; concise save summary; RetroBat configuration and game-information status. |
| Saves       | What is happening to my progress?                         | Automatic-sync setting, current supported saves, review actions, and skipped-file details.                   |
| Activity    | What is happening now, and what recently finished?        | Unresolved issues, current operations, recent outcomes, and expandable technical logs.                       |
| Settings    | How is this installation configured?                      | Existing connection, folder, RetroBat, and application settings, grouped by purpose.                         |

Keep ROM availability, catalogue freshness, RetroBat setup, artwork/game-information publication, save synchronisation, and RomM connectivity as separate facts. A reachable server does not imply every endpoint is permitted. A successful mount does not imply artwork publication succeeded. Each problem belongs to its affected feature.

## Vocabulary and control contracts

Use these visible labels consistently. A short label is acceptable when its object is clear from the containing section; the accessible name must remain specific.

| Current wording or concept      | Use instead                                                       | Meaning                                                                                      |
| ------------------------------- | ----------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| Mount, start games folder       | **Make games available**                                          | Start the existing games-folder operation.                                                   |
| Mounting, starting games folder | **Preparing games…**                                              | Work is actually in progress; never imply that every ROM is downloading.                     |
| Games folder ready/running      | **Available**                                                     | The games folder is active under the existing backend contract.                              |
| Unmount, stop games folder      | **Stop** in the Games folder section                              | Accessible name: `Stop games folder`. Explain the verified effect on game access.            |
| Refresh library                 | **Check for library changes**                                     | Fetch/check catalogue changes using the existing operation.                                  |
| Update ready                    | **Library changes are ready to apply**                            | A checked catalogue has changes not yet reflected in the active folder.                      |
| Restart folder                  | **Apply library changes**                                         | Apply those changes through the existing restart process, if that is its verified effect.    |
| Save sync Refresh               | **Check for changes**                                             | The existing save discovery/reconciliation operation; confirm its effects in source first.   |
| Disable/Enable save sync        | **Automatic save sync** with an explicit On/Off control           | Persistent user consent, separate from checking and transfer activity.                       |
| Linked, mapped                  | **Linked to [game]** in association details                       | Identifies the game; does not certify successful syncing.                                    |
| Captured                        | **Preparing upload…** only while that stage is current            | Otherwise keep the technical stage in operation details.                                     |
| Outgoing/incoming               | **Waiting to upload/download** or **Uploading/Downloading**       | Choose the actual queue or execution state, not just a direction.                            |
| Ignored/excluded                | **Skipped files** as the group                                    | Each item has its real reason: unsupported format, auxiliary file, local backup, etc.        |
| Link a file by path / Manual    | **Link a save file…**                                             | An action outside the table, never a fake file row.                                          |
| Frontend metadata               | **Artwork and game information**                                  | The RetroBat-facing artwork, descriptions, and game lists handled by the existing publisher. |
| More / Details                  | **Technical details**, **Show full message**, or **Save details** | Name the content that will appear.                                                           |

Do not rename a command to imply narrower or broader behaviour than it actually has. If source inspection finds that a label above does not match the current operation, correct the contract and use one accurate label everywhere. Do not implement a new backend capability to satisfy a speculative label.

## Screen specifications

### Overview

Keep the main content in three sections, ordered as follows.

**1. Games folder**

Place the status next to the heading. Show the configured path as selectable text, the one-sentence lazy-download explanation, and a stable action row. Keep the useful `game count · platform count · last library check` summary below it. Do not hard-code the screenshot counts.

| Actual state              | Primary control                                         | Other behaviour                                                                                                                                                              |
| ------------------------- | ------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Required setup incomplete | **Finish setup**                                        | Open the specific existing settings section that needs attention.                                                                                                            |
| Stopped                   | **Make games available**                                | Explain that starting makes the games appear in the configured folder. Keep **Check for library changes** available when the connection and existing operation allow it.     |
| Preparing                 | Disabled **Preparing games…** in the same position      | Show actual stage/progress; prevent duplicate starts. Keep unsupported concurrent actions disabled.                                                                          |
| Available                 | **Open games folder**                                   | Show secondary **Stop** and **Check for library changes**.                                                                                                                   |
| Stopping                  | Keep the action row stable; disable unavailable actions | The Stop control becomes disabled **Stopping…**. Update when actual completion arrives.                                                                                      |
| Operation failed          | Restore the relevant usable control                     | Show the failed action alongside the actual remaining folder state. Keep independent catalogue checks available when supported. Offer the relevant retry or settings action. |

Show measured progress when the backend provides meaningful completed/total values. Otherwise use an indeterminate indicator with a stage label. Distinguish preparing game entries from downloading game data.

A failed Stop must not imply the folder stopped. A failed catalogue check must not replace an Available folder with a mount-failure state. After an update/restart failure, show the actual resulting folder state; if it is uncertain, say it is being checked rather than guessing.

Move raw file totals and the `files not shown` badge into **Library details**. Explain actual categories and units there. An omitted artwork file is not a missing game; a ROM missing on the server may be an actionable problem. Do not describe all omitted files as unsupported games, duplicates, or harmless assets.

When changes need a restart, show **Library changes are ready to apply** next to **Apply library changes**. Explain the verified interruption before the action: for example, `Applying changes restarts the games folder and temporarily interrupts file access. Finish playing first.` Reuse existing safeguards. Do not add game-process detection or a new update system.

**2. Saves**

Use a compact summary and normal-sized secondary action. Examples, driven by real data:

- `2 tracked saves up to date` → **View saves**.
- `Uploading 1 save` → **View activity**.
- `1 save needs review` → **Review save**, opening the affected item or its filtered list.
- `Automatic save sync is off` → **View saves**, where the user can enable it deliberately.

Unsupported locations must not appear as unfinished linking work. Use a separately discoverable skipped-file count. A summary count and the destination it opens must describe the same records.

**3. RetroBat**

Group installation/configuration and **Artwork and game information** together. Use **Configured** for a validated setup unless the implementation can establish a stronger status. Keep the settings action secondary.

For the observed metadata-only failure, use:

> **Couldn't update artwork and game information**
> Your games are available, but artwork and game information may be missing or out of date.

Only include the games-available sentence when the folder is actually available. Provide **Retry update** for the existing publisher, with a specific accessible name, and **Technical details** containing the original exception. If retry re-runs a wider operation, communicate that actual scope.

Show a 403 favourites/history problem separately: `RomM denied access to favourites and play history.` Do not claim the cause is an expired token or missing scope until verified. Offer the existing connection-settings/details route, not an invented permission-granting action.

### Saves

**Header.** Show **Automatic save sync** with its real On/Off state, one current summary, the latest relevant successful check, and **Check for changes**. Remove the list of absolute save paths from the header. Place server and RetroBat configuration facts in their settings/details context.

The screenshot text `checked 8.3.0-stable-win64` has no clear subject. Inspect its origin: call it a detected RetroBat version only if the installation was inspected; otherwise label the actual compatibility reference. Do not turn a tested-version constant into a claim about the user's installation.

**Current saves.** Use one row per actual supported save identity, not one row per remote revision or one row per game title. Multiple save files/slots for the same game must remain distinguishable. Preserve the existing identity and matching rules.

| Main row field        | What to display                                                                                                         |
| --------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| Game                  | Catalogue title without a ROM extension; platform as secondary text. Use filename fallback when a title is unavailable. |
| Save file             | A distinguishable filename or existing slot label; full path in details.                                                |
| Status                | Current sync state from the state table below.                                                                          |
| Last result or action | Useful outcome/time, or **Review** for an actionable issue.                                                             |

Place actionable saves first while keeping ordering stable during updates. Retain search and a status filter. Preserve search text, selection, row expansion, scroll position, and keyboard focus during background refreshes.

**Details.** Expand a row to show local path, known association, current issue, and last successful result. Show `Local and RomM copies match` only when current verification supports it. Put relevant remote history under that save; keep full hashes, ROM/file IDs, revision IDs, and original errors inside **Technical details**, selectable and copyable.

**Unlinked and skipped files.** An eligible file that needs a game association is actionable. An unsupported memory-card directory is not. Show eligible unlinked files with the existing linking workflow. Place skipped files behind **Skipped files (N)** with accurate reason labels. Known application-generated backups should read `Local backup — not synced`; establish ownership from the existing backup rules, not a loose filename guess. Preserve all such files.

Move **Link a save file…** into a secondary action near the list. Do not encourage linking arbitrary emulator configuration or unsupported shared memory cards. Show pagination only when there is more than one page.

**Empty states.** Distinguish `No supported saves found` from `No results match this filter`. If some files were skipped, provide the corresponding details link. For a genuine initial state, use the existing discovery action and concise guidance about where supported saves are found; do not introduce a second setup wizard.

Turning automatic sync off does not hide discovered saves, associations, last successful results, or unresolved review issues. Show **Automatic save sync is off** in the header and label retained results as last known. Use an empty-state layout only when the list is actually empty. **Check for changes** must respect disabled consent: it must not start a save transfer or apply a remote version without separately authorised existing behaviour.

### Save review and protective pauses

For the final screenshot scenario, show one affected Pokémon save and this message:

> **This save needs review**
> RomM returned a save version that RomM-FS could not verify. This unverified version was not applied.

Include the second sentence only when the backend confirms that disposition for the rejected version. A generic Needs review state is not sufficient to assert the outcome of every remote revision or earlier operation.

Add `Automatic sync is paused for this save` only if the backend confirms that scope. If the whole service is paused, say so. List the ambiguous remote records as **Remote versions to review** under this save; do not count them as incoming transfers unless downloads have actually been queued.

The main action is **Review details**, opening the explanation and relevant records. Expose any existing supported recovery action only when its preconditions are met. If no supported automatic recovery exists, say that plainly in the details and retain access to diagnostics. Do not add **Force sync**, **Use newest**, or a remote-adoption bypass.

Keep genuine supported conflicts distinct from unverified remote input. Only a backend-confirmed conflict can use the existing conflict-resolution workflow. If that workflow can replace data, present explicit local/RomM choices, known timestamps and consequences using its existing backup and validation rules. Do not select a winner from timestamps alone or add a new resolution algorithm.

Whether arbitrary browser-uploaded/renamed saves should be accepted is a separate product decision. Record it without blocking the rest of this plan.

### Activity

Use four clearly separated sections:

1. **Needs attention:** unresolved issues, named by the affected feature/save, with an impact sentence and appropriate existing action. No old warnings that have already resolved.
2. **In progress:** real current operations only. Keep one row per operation and update that row through its stages.
3. **Recent activity:** a bounded list of user-meaningful outcomes, with older results collapsed or available through the existing history mechanism.
4. **Technical log:** collapsed by default; retain search, severity filters, follow, copy, and open-log-folder functionality.

For example, a single transfer progresses from `Pokémon Red — Preparing upload…` to `Uploading to RomM…` and ends as `Pokémon Red — Uploaded to RomM · 2 minutes ago` in Recent activity. Captured/uploading/verified events remain in details/logs. Distinct save revisions remain distinct operations; do not merge unrelated work just because the game title matches.

A running games folder is persistent status, not an indefinitely pending operation. Represent its successful start as a completed event. Empty In progress should say **No operations in progress**, without suppressing a separate unresolved issue.

Inspect existing ROM-download reporting and reuse it here where available, showing the game and actual progress. Keep downloads triggered by game access distinct from catalogue checks and save transfers. This does not require a new download-management system.

Inspect the current **Clear** command. If it only clears completed UI history, label it **Clear completed activity**. If its effect differs, name that effect and keep its current boundaries. It must not acquire journal, save, backup, or server-deletion side effects.

Use a stable scrolling region. Following new logs must not drag a user away from a message they are inspecting. Make full-message expansion work inline or in an existing details surface, without rows of detached `More` buttons.

### Settings and existing setup

Inspect the existing form before editing it. Group current fields under **RomM connection**, **Games folder**, **RetroBat**, and **Application**, where applicable. Relocate existing advanced controls under an Advanced disclosure; do not add speculative settings.

Use persistent labels, concise purpose text, and the existing folder picker. Keep one clear Apply/Save model per section; avoid mixing immediate changes and staged values without indication. Test connection and form validation must acknowledge progress, retain entered values on failure, and place errors beside the responsible field.

Preserve the tested distinction between closing to the tray and explicit Quit. Explain that distinction in the existing application settings/help location. During explicit Quit, show **Closing RomM-FS…** while cleanup runs and prevent repeated quit requests. Do not make the application disappear before required cleanup or bypass cleanup to reduce apparent delay.

## State and feedback rules

Implement these rules through existing models and shared selectors/presentation helpers. Do not create a replacement event architecture.

| Authoritative fact                                                                           | Display                                                                                                           |
| -------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| Save association exists, but current sync result is unknown                                  | Association details plus **Not checked** or **Checking for changes…**, as appropriate; not a green success claim. |
| User explicitly switched automatic sync off                                                  | **Automatic save sync is off**; retain last known history. Checking must not turn it on.                          |
| Discovery/reconciliation is actually running                                                 | **Checking for changes…**; consent remains independently On/Off.                                                  |
| An eligible transfer is queued                                                               | **Waiting to upload** / **Waiting to download**.                                                                  |
| Transfer is actively executing                                                               | **Uploading to RomM…** / **Downloading from RomM…**.                                                              |
| Current save verified against the appropriate remote baseline, with no newer unresolved work | **Up to date**, scoped to that save and last successful check.                                                    |
| Connection unavailable and pending work cannot proceed                                       | **Waiting for RomM** with the known issue; do not claim active transfer or silently drop work.                    |
| Existing safety rule rejects a remote revision                                               | **Needs review**, with the specific protective explanation.                                                       |
| Actual supported two-sided conflict                                                          | **Conflicting changes**, linked to the existing supported workflow.                                               |
| Expected unsupported/auxiliary file                                                          | A skipped-file reason, outside the pending/error counts.                                                          |
| Existing records have no reliable historic scan timestamp                                    | **Last scan time unavailable**, not `No save scan yet`.                                                           |

For aggregate status, separate consent from work. Within enabled sync, surface blocked/actionable saves before normal progress; then active transfers, queued work, checking, and confirmed up-to-date status. Retain secondary counts so a problem in one save does not conceal work on another. Report affected saves and remote versions as different units.

If the backend permits an already-started transfer to finish or stop after automatic sync is switched off, show the Off setting alongside that operation's actual progress until it reaches its terminal state. Preserve existing cancellation/finish semantics and keep unresolved review issues visible.

Start/Stop/check actions must render their pending state before awaiting long work. Use the real completion/failure event to update the screen. Do not rely on switching pages, a later timer, or an optimistic assumption that the operation succeeded. Preserve genuine queued work across restart. Remove each completed revision/operation from pending counts on its own terminal completion. Keep newer unresolved work pending; an older completion must not overwrite its state. The save becomes Up to date only when current verification is valid and no unresolved work remains.

Refresh must never flash or persist **Disabled** unless consent actually changed. Ignore stale completion results from an older account/session or superseded operation using existing request identity where available. A late callback must not undo a user's disable action.

Keep the last successful scan/check time separate from the time an attempt started. A completed scan finding zero files still updates the scan result. Do not infer a scan timestamp from transfer history.

## Implementation steps

Use the repository's instructions and build workflow. The report used `mise x -- mbx` on Windows, but verify the current instructions rather than assuming the old command list is still correct. Keep this plan in the existing UX documentation, or use `docs/ux-remediation.md` if no suitable document exists. Add a compact progress section with current step, changes, verification, and remaining blockers.

### Step 1 Map the current implementation and reproduce the failures

- Identify the actual screen components, controllers, state sources, event subscriptions, action handlers, and relevant tests. Record their real paths in the progress section; do not guess them from crate names.
- Map Start, Stop, close, Quit, both Refresh actions, Restart folder, save consent, retry, Clear, and existing ROM-download reporting to their effects and completion signals.
- Reproduce the stale status and wrong-folder reports. Capture baseline screenshots for the states below. Record the exact build/commit and any local modifications used.
- Recheck current build failures. The supplied report mentions undefined `gamelist`, unresolved `LocalFree`, and undefined `NoopHydratedRemover`; those are historical observations, not instructions to patch a different checkout blindly.

**Done when:** actual action semantics and current state ownership are known, the affected build is identified, and existing validation blockers are distinguished from new defects. Limit investigation to decisions needed for these fixes.

### Step 2 Make state and lifecycle controls trustworthy

- Correct pending/completed save counts and derive consistent summaries for Overview, Saves, and Activity.
- Separate persisted sync consent from checking and transfer state. Fix stale completion handling where reproduced.
- Keep controls in place while busy, prevent duplicate operations, and update the current screen on completion.
- Fix Open games folder to use the configured absolute root, including paths with spaces. Show an actionable error instead of falling back to the working/home directory.
- Make Stop and Quit visibly acknowledge progress while preserving cleanup and tray behaviour.

**Done when:** repeated navigation is unnecessary to observe completion; all screens agree; completed history does not become queued work after restart; checks do not change consent.

### Step 3 Simplify Overview and standardise wording

- Implement the three-section Overview and the vocabulary/control contracts.
- Group RetroBat setup and game-information status, keeping the permission warning separate.
- Explain pending library changes and the interruption caused by applying them.
- Replace dense counts with useful summaries and discoverable details. Route summary actions to the actual affected records.
- Apply consistent field/action wording to existing Settings/setup without expanding their capabilities.

**Done when:** a user can identify games-folder state, next action, save state, and any affected optional feature without reading a technical log.

### Step 4 Rework the Saves list and review flow

- Implement one current row per existing save identity with readable titles, actual sync states, and useful result/action fields.
- Separate eligible unlinked saves from skipped files and correct verified backup reasons.
- Replace the fake Manual row with the secondary linking action.
- Correct scan/check labels and implement accurate empty/filter states.
- Group ambiguous remote history beneath the affected save and apply the protective-pause wording without changing its verification rules.

**Done when:** the supplied ambiguous-history case appears as one affected save with nested remote records; newly linked saves are not automatically labelled synced; expected exclusions are not presented as unfinished work.

### Step 5 Rework Activity around operations and outcomes

- Implement Needs attention, In progress, Recent activity, and collapsed Technical log.
- Group stage events by existing operation identity, keeping newer revisions and separate save files distinct.
- Replace raw default labels with game/action/outcome/time. Preserve full technical records in details.
- Fix ambiguous Clear/More controls and scrolling behaviour.

**Done when:** one completed upload yields a terminal user-visible outcome and no pending copy of its historic stages; logs remain available for diagnosis.

### Step 6 Resolve the isolated functional and responsiveness defects

Treat these as distinct fixes with their own evidence, not reasons to redesign the backend:

- **Metadata publication:** inspect the failing validation call, native reparse tag, managed-root identity, and ownership. The report's shared-save-guard explanation is a hypothesis. If confirmed, make the smallest correction specific to metadata publication into the app-owned root. Keep save-path validation strict, reject untrusted traversal, and retain existing metadata if publication fails. CFAPI placeholders use reparse points, which are not synonymous with symbolic links; this explains why the guard deserves inspection, not why it should be removed. See [Microsoft's reparse-point compatibility section](https://learn.microsoft.com/en-us/windows/win32/cfApi/build-a-cloud-file-sync-engine#compatibility-with-applications-that-use-reparse-points).
- **403 favourites/history:** confirm the failed endpoint and current permission response. Present the feature-specific failure; do not expand permissions, replace credentials, or attribute it to the metadata path error without evidence.
- **Hover/rendering lag:** measure the reproduced interaction and check whether UI-thread work, event volume, expensive formatting, or list rendering explains it. Apply the smallest measured fix using the existing toolkit. Do not assume logs cause it, add arbitrary polling, or introduce a performance framework. Keep full diagnostic logging available.

Add concise operation start/completion/failure context where needed to diagnose affected flows, using existing logging conventions. Do not spam a log on every render or create new telemetry infrastructure.

**Done when:** each reproduced defect is corrected and verified, or has a specific remaining blocker and accurate UI. A copy change does not count as fixing metadata publication. Continue unaffected work when a product decision or environment permission blocks one item.

### Step 7 Apply visual consistency and verify the edited journeys

- Reuse the current dark surfaces and blue accent, existing font, and existing widgets. Establish consistent spacing, heading sizes, button heights, field labels, and row alignment using existing tokens or a small local set.
- Keep status text near its object. Use blue/neutral treatment for active work, green for confirmed success, amber for review/warnings, and red for failures. Always include words/icons so colour is not the only signal.
- Use roughly 14–16 logical-pixel body text and readable secondary text; verify actual rendering and scaling rather than forcing fixed physical pixels. Meet normal-text contrast of at least 4.5:1 and large-text contrast of 3:1.
- Keep primary controls visible and secondary actions compact. Long paths/errors must wrap, truncate with an accessible full value, or expand without pushing controls off-screen.
- Check keyboard navigation, visible focus, activation, row disclosure, form labels, and accessible status names through the existing UI toolkit. Microsoft's [Windows accessibility guidance](https://learn.microsoft.com/en-au/windows/apps/develop/accessibility) covers these requirements; no toolkit migration is requested.
- Inspect the current screenshot-size window (1162 × 952), the application's supported minimum size, and Windows 125%/150% scaling. Verify scrolling and focus during live updates.

**Done when:** the verification checklist passes for the affected scope and the agent has inspected the rendered application, not merely compiled it.

## Focused verification checklist

Use existing tests and fixtures. Add automated coverage for meaningful state/consent regressions, not for static labels or implementation-shaped snapshots. Do not turn this into the application's entire unfinished acceptance programme.

- [ ] **State transitions:** slow start, successful stop, failed start/stop, duplicate clicks, and late callbacks produce accurate visible states. Completion updates without navigation. Repeat at least one sequence with background save activity.
- [ ] **Pending work:** verified completed revisions are absent from pending counts; genuine queued work remains after restart; a newer revision is not hidden by an older completion.
- [ ] **Consent:** check while disabled, disable while checking, and restart with persisted consent. Neither temporary UI state nor a late result enables syncing or presents a false disable transition.
- [ ] **Saves:** distinguish linked-but-unchecked, verified up to date, waiting, active, review, eligible unlinked, and expected skipped files. Check a completed empty scan and restored records without a timestamp.
- [ ] **Protective refusal:** use a controlled fixture for unverified remote history. Show one affected save and accurate nested versions. Confirm no remote adoption, local replacement, backup deletion, or journal reset was introduced.
- [ ] **Normal conflicts only if touched:** test with valid owned revisions from an existing fixture or supported second client. A renamed browser upload is not evidence for the normal two-sided conflict path.
- [ ] **Folder lifecycle on Windows:** open the configured root; enumerate; read a known small ROM; stop; remount and read again. Check the expected placeholder cleanup while retaining the existing policy for frontend files. Close-to-tray continues serving reads; explicit Quit performs cleanup.
- [ ] **Lazy access:** opening Overview/Saves/Activity or checking the catalogue does not read ROM contents. If those paths changed, use existing request/download instrumentation to verify no ROM hydration caused by the UI. Repeated hashes alone do not prove cache reuse or no-download enumeration.
- [ ] **RetroBat and ordinary saves:** list and launch an exercised game. If affected code handles saves, verify a supported ordinary save upload and status completion using a test copy/fixture or a specifically authorised live test. Preserve original progress and revisions.
- [ ] **Metadata:** if repaired, verify the artwork/game-information result independently. For failure fixtures, retain the correct partial-success state and existing metadata. Validate path protection for the touched publication path; no blanket reparse bypass.
- [ ] **Rendered UX:** compare before/after Overview stopped/preparing/available/partial failure, Saves healthy/review/skipped/empty, and Activity active/completed. Check long text, keyboard use, scaling, and live updates.

Report a blocked native test as blocked. Do not count compilation, helper-script tests, or screenshots as proof of native runtime behaviour. Offline queues, cancellation coverage, clean-host driver audits, and second-device acceptance already have gaps in the report; cover them only if the changes affect those behaviours or the repository requires them.

## Agent working and completion rules

- Make one implementation step at a time. Preserve unrelated work in a dirty checkout. Use small focused commits according to repository instructions; do not bundle unrelated repairs into this change.
- Update the progress section after each step with what changed, what ran, the observed result, and the next remaining issue. Record concrete blockers without repeatedly stopping the whole task for routine wording/layout choices.
- Reuse existing dependencies, controls, async mechanisms, and test tooling. Do not install a design system or switch UI frameworks to carry out this plan.
- Do not remove a capability just to make a screen simpler. Move advanced information into details while retaining the existing operation and diagnostic path.
- Treat all screenshot numbers and messages as examples of states, not static production data.
- Finish with a concise change summary, before/after screenshots of the affected states, the actual commands and build tested, and remaining defects or unverified cases. Mark the supplied two-sided conflict test inconclusive unless a valid new test establishes its result.

**Completion standard:** the user can understand the current state and take the appropriate next action on the relevant screen; the core games-folder, RetroBat, and supported save behaviour remains intact; and the remaining functional limitations are accurately represented.

## Evidence index

The filenames below identify the supplied attachments. They are not source-code paths. Read `human-testing-2026-10-17/report.md` for human observations and evidence limitations. Keep its original contents unchanged.

| Ref | Supplied screenshot                                        | Relevant state                                                   |
| --- | ---------------------------------------------------------- | ---------------------------------------------------------------- |
| S01 | `rommfs-app_qVG5bdhZKj.png`                                | Overview stopped; unmatched summary; raw counts.                 |
| S02 | `rommfs-app_PaxHrgvogh.png`                                | Overview preparing; missing start control.                       |
| S03 | `rommfs-app_aicyKCa4eG.png`                                | Activity preparing entries; technical log.                       |
| S04 | `rommfs-app_Gxfitb5tpA.png`                                | Activity mount success; metadata failure; permission warning.    |
| S05 | `rommfs-app_sSDoEcKQLD.png`                                | Available games; raw metadata error on Overview.                 |
| S06 | `rommfs-app_vOwSt0Uck7.png`                                | Remounted Overview with persistent metadata error.               |
| S07 | `codex-clipboard-122e06d0-99b7-4b02-ac9f-1c8486d7d147.png` | Captured/uploading/verified rows and outgoing count.             |
| S08 | `rommfs-app_b02GtaRVMy.png`                                | Activity stages, completed controls, and dense log layout.       |
| S09 | `codex-clipboard-dca3dc46-bd80-4dd4-9d98-6745697ff402.png` | Multiple verified revisions still listed under Operations.       |
| S10 | `rommfs-app_nye92MZPni.png`                                | Zero mapped saves; excluded locations counted as unmatched.      |
| S11 | `codex-clipboard-23259219-6856-4cb7-8f33-93c1f742c66d.png` | Two linked saves after refresh; false scan message.              |
| S12 | `codex-clipboard-50bdd8e9-e82c-49ed-a0cf-974b0e175e5d.png` | Expanded save details dominated by IDs/hashes.                   |
| S13 | `rommfs-app_Xcqr0BfarJ.png`                                | Update ready and Restart folder.                                 |
| S14 | `codex-clipboard-1cec1022-53e4-4744-af40-ff529ffc915c.png` | Restart Activity reports two outgoing.                           |
| S15 | `codex-clipboard-8b1649ad-eb5e-4109-bb6e-2cbda42db188.png` | Restart Saves retains mapping but reports syncing.               |
| S16 | `codex-clipboard-7a41b6b0-0ac8-4762-bb02-f468d2aab712.png` | Overview with save attention and metadata failure.               |
| S17 | `codex-clipboard-a058e9fb-7a5b-420b-9d86-4cac175b0e0b.png` | Protective refusal; ambiguous history; misleading backup reason. |
| S18 | `codex-clipboard-786a6fe5-1d97-4101-8b21-9818e939cd06.png` | Activity incoming/review counts and refusal log.                 |

External references establish platform behaviour and basic accessibility requirements. The proposed information architecture, wording, and layout are design decisions made for this application.
.scratch/design:
total 244
drwxr-xr-x 2 aaron aaron  4096 Oct 25 21:42 .
drwxrwxr-x 4 aaron aaron  4096 Oct 26 08:46 ..
-rw------- 1 aaron aaron     0 Oct 25 21:36 .pupdate.hook.lock
-rw-r--r-- 1 aaron aaron  2918 Oct 25 21:41 Buttons Section.png
-rw-r--r-- 1 aaron aaron  2254 Oct 25 21:41 Nav item Section.png
-rw-r--r-- 1 aaron aaron  1667 Oct 25 21:41 Pills and badges Section.png
-rw-r--r-- 1 aaron aaron  3932 Oct 25 21:41 Settings row Section.png
-rw-r--r-- 1 aaron aaron 14666 Oct 25 21:41 Settings · Application.png
-rw-r--r-- 1 aaron aaron 18023 Oct 25 21:41 Settings · Games folder.png
-rw-r--r-- 1 aaron aaron 18802 Oct 25 21:41 Settings · RetroBat.png
-rw-r--r-- 1 aaron aaron 13546 Oct 25 21:41 Settings · RomM connection.png
-rw-r--r-- 1 aaron aaron 22745 Oct 25 21:41 Settings · Saves.png
-rw-r--r-- 1 aaron aaron  1000 Oct 25 21:41 Toggle Section.png
-rw------- 1 aaron aaron 20632 Oct 25 21:41 activity.png
-rw------- 1 aaron aaron 30852 Oct 25 21:41 components.png
-rw------- 1 aaron aaron 26489 Oct 25 21:41 overview.png
-rw------- 1 aaron aaron 35564 Oct 25 21:41 saves.png

.scratch/human-testing-2026-10-26:
total 1040
drwxrwxr-x 2 aaron aaron   4096 Oct 26 08:46 .
drwxrwxr-x 4 aaron aaron   4096 Oct 26 08:46 ..
-rw-rw-r-- 1 aaron aaron  69785 Oct 26 08:46 10-26-26-192216.png
-rw-rw-r-- 1 aaron aaron  41277 Oct 26 08:46 10-26-26-192225.png
-rw-rw-r-- 1 aaron aaron  76771 Oct 26 08:46 10-26-26-192231.png
-rw-rw-r-- 1 aaron aaron  82116 Oct 26 08:46 10-26-26-192235.png
-rw-rw-r-- 1 aaron aaron  48755 Oct 26 08:46 10-26-26-192304.png
-rw-rw-r-- 1 aaron aaron  36693 Oct 26 08:46 10-26-26-192308.png
-rw-rw-r-- 1 aaron aaron  50319 Oct 26 08:46 10-26-26-192312.png
-rw-rw-r-- 1 aaron aaron  50430 Oct 26 08:46 10-26-26-192315.png
-rw-rw-r-- 1 aaron aaron  61861 Oct 26 08:46 10-26-26-192318.png
-rw-rw-r-- 1 aaron aaron  42313 Oct 26 08:46 10-26-26-192322.png
-rw-rw-r-- 1 aaron aaron  83971 Oct 26 08:46 10-26-26-192330.png
-rw-rw-r-- 1 aaron aaron  83867 Oct 26 08:46 10-26-26-192345.png
-rw-rw-r-- 1 aaron aaron  55527 Oct 26 08:46 10-26-26-192848.png
-rw-rw-r-- 1 aaron aaron  76429 Oct 26 08:46 10-26-26-192854.png
-rw-rw-r-- 1 aaron aaron 157914 Oct 26 08:46 10-26-26-193202.png
-rw-rw-r-- 1 aaron aaron    868 Oct 26 08:46 notes.md

]]></tool_result>
<tool_result tool_call_id="toolu_01CwhTte1nRYTfjWvuvknC2J" source="tool_result" timestamp="2026-10-27T09:04:06.553Z"><![CDATA[
# UI and UX remediation

## Product polish — 26 October 2026

This pass improves the existing working application without requiring another
human test round before making UX changes.

- Overview now uses readable, full-width sections for games, RetroBat, saves,
  and artwork. Primary actions wrap at smaller window sizes; library counts
  and optional details no longer compete with the main task.
- Artwork checking, publication, waiting for file access, failure, and success
  share one event-derived summary across Overview and Activity. Completing
  image checks no longer looks like completing the whole update. Waiting has
  an explanation and no animated progress bar.
- Errors use the shared plain-language explanations with full diagnostics
  behind Technical details. Folder settings opens the actual settings page;
  setup and locked settings explain the next available action.
- The window restores focus to the visible tab order when a page change
  removes the focused control. Native Tab and Enter navigation now survives
  transitions from setup to Overview and Settings.
- The README explains the roles of RomM, RomMFS, and RetroBat. The user guide
  starts with everyday tasks; the architecture guide explains how to follow a
  user action through the code.

Verification: **385 portable tests passed**, including new coverage for
artwork publication/waiting/failure transitions and the unverified-save
explanation. Formatting, host UI compilation, portable Clippy, and UI-enabled
Clippy passed. Rendered the native GPUI screens in an isolated Linux/Xvfb
source copy with seeded state at 1160×920 and 720×480. Checked disclosures,
folder-settings navigation, and keyboard activation with assertions, and
inspected setup, Overview, Saves, Activity, Settings, and artwork progress,
waiting, and failure states.

The preview used isolated settings and an inert event drain. These checks
cover presentation and navigation; they are not a new Windows runtime run.
They do not establish the cause of file locks, deleted RetroBat links, or the
reported Windows latency. Mount ownership, lazy ROM reads, save consent, and
worker behavior retain their existing contracts.

## Implementation progress — 17 October 2026

Current step: implementation complete; native Windows acceptance remains outstanding. This record describes the working tree
based on `55b5c104953a16dd9e3234bdd8588a8ff450eb9e`; it is not native Windows
acceptance. All save scenarios use temporary copies and the in-process fixture.
Live consent and live server data have not been changed.

### Implementation and ownership

- `crates/rommfs-app/src/controller/state.rs` owns event-derived folder,
  consent, scan and save summaries. `window.rs` drains events on the existing
  UI timer and notifies the visible page; `session.rs` retains application
  ownership independently of the window.
- `window/actions.rs` and `window/folder_controls.rs` dispatch Start, Stop,
  library checking and applying library changes. Start prepares placeholders;
  Stop disconnects the provider and cleans verified owned entries while
  retaining private cache and ordinary frontend metadata. Applying library
  changes runs that existing stop/start sequence. Catalogue checks stage a
  new snapshot separately from the active mount.
- `save_sync_agent/actor.rs` emits authoritative queue snapshots. A local
  watcher hint can finish without a new snapshot; that drain now emits its
  own queue update. Current per-save work comes from scheduler, dispatched
  revisions, reconciliation and journal baselines. Retained review versions
  are not queued downloads. Completed stages share revision identity in UI
  history and do not create work on restart.
- `controller/save_sync_setup.rs` owns persisted consent and save checks.
  Check for changes rediscovers/reconciles using the existing consent; it can
  transfer saves when consent is On and cannot enable consent when Off.
  Session IDs reject old worker results. An explicit pending consent change
  cannot be undone by the previous session's discovery response.
- `window/pages/overview.rs`, `save_review.rs`, `activity.rs` and `settings.rs`
  compose the four destinations. Supported save identities stay separate;
  remote review records nest beneath their owning identity. Skipped files
  have a separate disclosure; recorded replacement backups use journal
  evidence for their label, rather than filename-based ownership claims.
- `desktop.rs` opens the configured absolute folder through the native shell
  with a separate path argument and reports failure inline. It never falls
  back to the home or working directory.
- `Controller::begin_shutdown` requests cleanup once and transfers its join
  handle. Explicit tray Quit shows the closing state and joins off the UI
  thread before exiting. Window close still hides to the tray when a tray
  exists. The app-quit fallback also waits for cleanup off-thread.
- `controller/retrobat_setup.rs` runs metadata publication. Core setup-record
  inspection previously reapplied the save-path reparse guard to the active
  CFAPI root. Metadata now uses `linked_systems_with_root_check` with the
  existing CFAPI ownership/native-root check. Ordinary setup and all save
  paths retain strict validation. The publisher still stages identity state,
  writes atomically, and preserves existing files on failure.
- Favourites/history fetch failures are surfaced separately from artwork
  publication and consent. A 403 is described as denied access; the UI does
  not infer an expired token or change credentials/permissions.
- ROM download reporting reuses existing `DownloadStarted`, `DownloadProgress`
  and terminal events. Clear completed downloads only removes successful UI
  rows. Diagnostic logs, journals, backups and remote revisions are untouched.
  The technical log is collapsed by default and stops following when a user
  scrolls away from the bottom. This reduces default render work; it is not
  evidence of the cause of the reported Windows hover lag.

### Verification and limits

- Baseline host UI compilation passed; the historic undefined `gamelist`,
  `LocalFree` and `NoopHydratedRemover` errors did not reproduce.
- Added behavior coverage for revision-stage identity, newer pending work,
  consent during checks, stale scan results, zero-file successful scans,
  review while Off, Stop/closing transitions, structural skips, unchanged
  watcher-hint draining, and backend-specific metadata root validation.
- Final portable suite: **383 passed, zero failures**, using the repository's
  serial `cargo test --locked -p rommfs-core -p rommfs-fixture -p rommfs-cfapi
  -p rommfs-app --no-default-features -- --test-threads=1` command. Localhost
  fixture sockets required execution outside the filesystem/network sandbox.
- `cargo fmt --all --check`, native-host UI `cargo check --locked -p rommfs-app
  --all-targets`, portable Clippy for all four crates with `-D warnings`, and
  UI-enabled app Clippy with `-D warnings` passed. Checks used the existing
  lockfile and installed dependencies, with build output isolated under `/tmp`.
- Rendered production pages in a temporary Linux/Xvfb source copy with seeded
  state and isolated settings. Inspected Overview, Saves, Activity and Settings,
  preparing/stopping/off/closing states, and one affected save with six remote
  review versions. Inspected 1162×952 and 720×480 logical windows and simulated
  X11 scaling at 125%/150%. Pointer expansion followed by Enter collapse kept
  visible row focus. Local captures are in `.scratch/archived/ux-remediation-preview/`.
  The preview's event drain was inert, so these screenshots do not validate
  real worker completion, native shell behavior, or Windows accessibility.
- Source inspection confirmed that legacy `save_sync_profile_version` comes
  from the installation's `system/version.info`; the generic-file profile
  instead uses an `ordinary files` marker. The ambiguous "checked" header was
  removed, and Settings no longer presents that save-profile value as the
  selected RetroBat installation's version.
- Catalogue omission totals mix whole-game and individual-file omissions.
  Library details now states those units and the actual missing-data,
  destination, name and path categories; it does not call them missing games
  or harmless assets.
- Native Windows CFAPI reparse tags, actual Explorer launch, close/tray/Quit
  behavior, 125%/150% scaling, and the reported hover latency require the
  Windows procedure in [testing/windows.md](testing/windows.md). Linux
  previews and portable tests cannot establish those facts.
- No new remote-adoption or force-sync path was added. Whether arbitrary
  browser-uploaded or renamed saves should be accepted remains a separate
  product decision; the existing protective rejection remains in force.

## Implementation brief

The original requested brief is retained below for the remaining native checks.

### RomM-FS UX remediation plan

## Objective

Improve the existing Windows application so a user can quickly tell whether their games are available, whether supported saves are up to date, what is happening, and what action they need to take. Keep the existing dark appearance and four main destinations. Prioritise truthful status, clear controls, and understandable recovery before cosmetic polish.

This is an implementation brief for a coding agent. Work through the implementation steps in order and complete one coherent change at a time. The intended result is an improved working application, with evidence from the affected journeys.

### Scope and evidence

- Keep the existing Rust application, UI toolkit, Windows CFAPI backend, lazy ROM access, and RetroBat integration. Alternative filesystem backends and other operating systems are outside this change.
- Reuse existing capabilities. Do not add a game browser, launcher replacement, cache manager, new save formats, a new sync engine, or a general framework for status/events. Small shared presentation helpers and changes to incorrect event handling are appropriate.
- Base this work on the supplied `report.md` dated 17 October 2026 and the 18 attached screenshots. The report references 24 screenshots; six are not in this attachment set. Settings and initial setup are not pictured, so recommendations for those surfaces require inspection of the current application.
- Screenshots establish displayed states. The report also records user-observed failures and partial functional testing. It does not establish the cause of lag, prove cache reuse, or demonstrate a failure of the normal two-sided conflict algorithm.
- Preserve local saves, backups, remote revisions, consent settings, and existing path-validation protections. UX work must not silently adopt an unverified remote save or make a failed operation look successful.
- Use an isolated test configuration, fixtures, and test copies for reproduction that could transfer or replace saves. The report leaves final live consent unknown; its server/path examples do not authorise live syncing. For any separately authorised live testing, inspect current consent and account for transfers that startup or restart may trigger. Leave live settings unchanged when preparing isolated scenarios.

## Findings to address

Evidence references such as S01 identify filenames in the evidence index at the end.

| Priority | Observed problem                                                                                                                                             | Required outcome                                                                                                                                                       | Evidence                                              |
| -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------- |
| P0       | Save headers say `Syncing saves…` or `2 outgoing` while completed `verified` events are visible. The state also persists after restart.                      | Current work comes from authoritative operation state. Completed history cannot create pending work. Preserve genuine queued work.                                     | S07–S09, S14–S15; report finding 7                    |
| P0       | The Start button disappears while preparing. Stop reportedly appears ineffective until changing pages; Quit reportedly leaves an unresponsive taskbar entry. | Stable controls acknowledge the operation immediately, prevent duplicate requests, and update on completion without navigation. Shutdown continues to perform cleanup. | S01–S03; report findings 4–5                          |
| P0       | A rejected remote revision produces one affected save plus six `Incoming` history rows and `6 incoming · 1 need review`.                                     | Show one affected save, an accurate protective-pause explanation, and the remote records under its details. Do not imply six downloads are queued.                     | S16–S18                                               |
| P1       | `Linked` describes association while a user needs to know whether their progress reached RomM. Hashes dominate healthy rows.                                 | Game title and current save status lead each row. Mapping and diagnostic information are secondary.                                                                    | S10–S12, S15                                          |
| P1       | `3 unmatched` refers to rows shown as excluded memory-card locations. App-created backups receive the same explanation as unsupported memory cards.          | Count eligible unlinked saves separately from skipped files. Explain each actual exclusion correctly.                                                                  | S10–S12, S17                                          |
| P1       | `No save scan yet` remains visible after files have been discovered and synced.                                                                              | Show the correct scan/check time, or acknowledge that a historic timestamp is unavailable.                                                                             | S10–S12, S15                                          |
| P1       | The folder is `ready` on Overview, `running` in Activity, and `mounting` while preparing. `Update ready` and `Restart folder` omit the purpose and effect.   | Use the vocabulary and action contracts below across all screens.                                                                                                      | S02–S06, S13, S16                                     |
| P1       | Overview displays a raw reparse-point exception beside green availability indicators.                                                                        | Explain the affected capability and impact. Keep games availability, game-information updates, and server permissions distinct.                                        | S04–S06; report findings 1–2                          |
| P1       | Open games folder reportedly opens the home directory.                                                                                                       | Open exactly the configured managed folder, with a visible error if that cannot be done.                                                                               | Report finding 3; not reproduced by these screenshots |
| P2       | Large logs, UUIDs, repeated stage rows, unexplained badges, and a full-width Review saves button compete with primary tasks.                                 | Clear hierarchy, concise summaries, and progressive disclosure of technical information.                                                                               | S01, S07–S12, S17–S18                                 |

## Product model and navigation

Use one sentence to explain the product near the games-folder controls or in existing setup help:

> Your RomM games appear in this folder. Game data downloads when an emulator or another program reads a game file.

The folder being available does not mean every game has been downloaded or that offline play is guaranteed. Reading ROM contents can trigger CFAPI hydration; the application must not read ROM contents merely to render status, calculate a UI summary, or inspect its own catalogue. Microsoft documents this distinction in its [CFAPI overview](https://learn.microsoft.com/en-us/windows/win32/cfApi/build-a-cloud-file-sync-engine).

Retain four destinations, renaming the current **Save sync** destination to **Saves**:

| Destination | Question it answers                                       | Content                                                                                                      |
| ----------- | --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| Overview    | Are my games available, and does anything need attention? | Games folder and primary controls; concise save summary; RetroBat configuration and game-information status. |
| Saves       | What is happening to my progress?                         | Automatic-sync setting, current supported saves, review actions, and skipped-file details.                   |
| Activity    | What is happening now, and what recently finished?        | Unresolved issues, current operations, recent outcomes, and expandable technical logs.                       |
| Settings    | How is this installation configured?                      | Existing connection, folder, RetroBat, and application settings, grouped by purpose.                         |

Keep ROM availability, catalogue freshness, RetroBat setup, artwork/game-information publication, save synchronisation, and RomM connectivity as separate facts. A reachable server does not imply every endpoint is permitted. A successful mount does not imply artwork publication succeeded. Each problem belongs to its affected feature.

## Vocabulary and control contracts

Use these visible labels consistently. A short label is acceptable when its object is clear from the containing section; the accessible name must remain specific.

| Current wording or concept      | Use instead                                                       | Meaning                                                                                      |
| ------------------------------- | ----------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| Mount, start games folder       | **Make games available**                                          | Start the existing games-folder operation.                                                   |
| Mounting, starting games folder | **Preparing games…**                                              | Work is actually in progress; never imply that every ROM is downloading.                     |
| Games folder ready/running      | **Available**                                                     | The games folder is active under the existing backend contract.                              |
| Unmount, stop games folder      | **Stop** in the Games folder section                              | Accessible name: `Stop games folder`. Explain the verified effect on game access.            |
| Refresh library                 | **Check for library changes**                                     | Fetch/check catalogue changes using the existing operation.                                  |
| Update ready                    | **Library changes are ready to apply**                            | A checked catalogue has changes not yet reflected in the active folder.                      |
| Restart folder                  | **Apply library changes**                                         | Apply those changes through the existing restart process, if that is its verified effect.    |
| Save sync Refresh               | **Check for changes**                                             | The existing save discovery/reconciliation operation; confirm its effects in source first.   |
| Disable/Enable save sync        | **Automatic save sync** with an explicit On/Off control           | Persistent user consent, separate from checking and transfer activity.                       |
| Linked, mapped                  | **Linked to [game]** in association details                       | Identifies the game; does not certify successful syncing.                                    |
| Captured                        | **Preparing upload…** only while that stage is current            | Otherwise keep the technical stage in operation details.                                     |
| Outgoing/incoming               | **Waiting to upload/download** or **Uploading/Downloading**       | Choose the actual queue or execution state, not just a direction.                            |
| Ignored/excluded                | **Skipped files** as the group                                    | Each item has its real reason: unsupported format, auxiliary file, local backup, etc.        |
| Link a file by path / Manual    | **Link a save file…**                                             | An action outside the table, never a fake file row.                                          |
| Frontend metadata               | **Artwork and game information**                                  | The RetroBat-facing artwork, descriptions, and game lists handled by the existing publisher. |
| More / Details                  | **Technical details**, **Show full message**, or **Save details** | Name the content that will appear.                                                           |

Do not rename a command to imply narrower or broader behaviour than it actually has. If source inspection finds that a label above does not match the current operation, correct the contract and use one accurate label everywhere. Do not implement a new backend capability to satisfy a speculative label.

## Screen specifications

### Overview

Keep the main content in three sections, ordered as follows.

**1. Games folder**

Place the status next to the heading. Show the configured path as selectable text, the one-sentence lazy-download explanation, and a stable action row. Keep the useful `game count · platform count · last library check` summary below it. Do not hard-code the screenshot counts.

| Actual state              | Primary control                                         | Other behaviour                                                                                                                                                              |
| ------------------------- | ------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Required setup incomplete | **Finish setup**                                        | Open the specific existing settings section that needs attention.                                                                                                            |
| Stopped                   | **Make games available**                                | Explain that starting makes the games appear in the configured folder. Keep **Check for library changes** available when the connection and existing operation allow it.     |
| Preparing                 | Disabled **Preparing games…** in the same position      | Show actual stage/progress; prevent duplicate starts. Keep unsupported concurrent actions disabled.                                                                          |
| Available                 | **Open games folder**                                   | Show secondary **Stop** and **Check for library changes**.                                                                                                                   |
| Stopping                  | Keep the action row stable; disable unavailable actions | The Stop control becomes disabled **Stopping…**. Update when actual completion arrives.                                                                                      |
| Operation failed          | Restore the relevant usable control                     | Show the failed action alongside the actual remaining folder state. Keep independent catalogue checks available when supported. Offer the relevant retry or settings action. |

Show measured progress when the backend provides meaningful completed/total values. Otherwise use an indeterminate indicator with a stage label. Distinguish preparing game entries from downloading game data.

A failed Stop must not imply the folder stopped. A failed catalogue check must not replace an Available folder with a mount-failure state. After an update/restart failure, show the actual resulting folder state; if it is uncertain, say it is being checked rather than guessing.

Move raw file totals and the `files not shown` badge into **Library details**. Explain actual categories and units there. An omitted artwork file is not a missing game; a ROM missing on the server may be an actionable problem. Do not describe all omitted files as unsupported games, duplicates, or harmless assets.

When changes need a restart, show **Library changes are ready to apply** next to **Apply library changes**. Explain the verified interruption before the action: for example, `Applying changes restarts the games folder and temporarily interrupts file access. Finish playing first.` Reuse existing safeguards. Do not add game-process detection or a new update system.

**2. Saves**

Use a compact summary and normal-sized secondary action. Examples, driven by real data:

- `2 tracked saves up to date` → **View saves**.
- `Uploading 1 save` → **View activity**.
- `1 save needs review` → **Review save**, opening the affected item or its filtered list.
- `Automatic save sync is off` → **View saves**, where the user can enable it deliberately.

Unsupported locations must not appear as unfinished linking work. Use a separately discoverable skipped-file count. A summary count and the destination it opens must describe the same records.

**3. RetroBat**

Group installation/configuration and **Artwork and game information** together. Use **Configured** for a validated setup unless the implementation can establish a stronger status. Keep the settings action secondary.

For the observed metadata-only failure, use:

> **Couldn't update artwork and game information**
> Your games are available, but artwork and game information may be missing or out of date.

Only include the games-available sentence when the folder is actually available. Provide **Retry update** for the existing publisher, with a specific accessible name, and **Technical details** containing the original exception. If retry re-runs a wider operation, communicate that actual scope.

Show a 403 favourites/history problem separately: `RomM denied access to favourites and play history.` Do not claim the cause is an expired token or missing scope until verified. Offer the existing connection-settings/details route, not an invented permission-granting action.

### Saves

**Header.** Show **Automatic save sync** with its real On/Off state, one current summary, the latest relevant successful check, and **Check for changes**. Remove the list of absolute save paths from the header. Place server and RetroBat configuration facts in their settings/details context.

The screenshot text `checked 8.3.0-stable-win64` has no clear subject. Inspect its origin: call it a detected RetroBat version only if the installation was inspected; otherwise label the actual compatibility reference. Do not turn a tested-version constant into a claim about the user's installation.

**Current saves.** Use one row per actual supported save identity, not one row per remote revision or one row per game title. Multiple save files/slots for the same game must remain distinguishable. Preserve the existing identity and matching rules.

| Main row field        | What to display                                                                                                         |
| --------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| Game                  | Catalogue title without a ROM extension; platform as secondary text. Use filename fallback when a title is unavailable. |
| Save file             | A distinguishable filename or existing slot label; full path in details.                                                |
| Status                | Current sync state from the state table below.                                                                          |
| Last result or action | Useful outcome/time, or **Review** for an actionable issue.                                                             |

Place actionable saves first while keeping ordering stable during updates. Retain search and a status filter. Preserve search text, selection, row expansion, scroll position, and keyboard focus during background refreshes.

**Details.** Expand a row to show local path, known association, current issue, and last successful result. Show `Local and RomM copies match` only when current verification supports it. Put relevant remote history under that save; keep full hashes, ROM/file IDs, revision IDs, and original errors inside **Technical details**, selectable and copyable.

**Unlinked and skipped files.** An eligible file that needs a game association is actionable. An unsupported memory-card directory is not. Show eligible unlinked files with the existing linking workflow. Place skipped files behind **Skipped files (N)** with accurate reason labels. Known application-generated backups should read `Local backup — not synced`; establish ownership from the existing backup rules, not a loose filename guess. Preserve all such files.

Move **Link a save file…** into a secondary action near the list. Do not encourage linking arbitrary emulator configuration or unsupported shared memory cards. Show pagination only when there is more than one page.

**Empty states.** Distinguish `No supported saves found` from `No results match this filter`. If some files were skipped, provide the corresponding details link. For a genuine initial state, use the existing discovery action and concise guidance about where supported saves are found; do not introduce a second setup wizard.

Turning automatic sync off does not hide discovered saves, associations, last successful results, or unresolved review issues. Show **Automatic save sync is off** in the header and label retained results as last known. Use an empty-state layout only when the list is actually empty. **Check for changes** must respect disabled consent: it must not start a save transfer or apply a remote version without separately authorised existing behaviour.

### Save review and protective pauses

For the final screenshot scenario, show one affected Pokémon save and this message:

> **This save needs review**
> RomM returned a save version that RomM-FS could not verify. This unverified version was not applied.

Include the second sentence only when the backend confirms that disposition for the rejected version. A generic Needs review state is not sufficient to assert the outcome of every remote revision or earlier operation.

Add `Automatic sync is paused for this save` only if the backend confirms that scope. If the whole service is paused, say so. List the ambiguous remote records as **Remote versions to review** under this save; do not count them as incoming transfers unless downloads have actually been queued.

The main action is **Review details**, opening the explanation and relevant records. Expose any existing supported recovery action only when its preconditions are met. If no supported automatic recovery exists, say that plainly in the details and retain access to diagnostics. Do not add **Force sync**, **Use newest**, or a remote-adoption bypass.

Keep genuine supported conflicts distinct from unverified remote input. Only a backend-confirmed conflict can use the existing conflict-resolution workflow. If that workflow can replace data, present explicit local/RomM choices, known timestamps and consequences using its existing backup and validation rules. Do not select a winner from timestamps alone or add a new resolution algorithm.

Whether arbitrary browser-uploaded/renamed saves should be accepted is a separate product decision. Record it without blocking the rest of this plan.

### Activity

Use four clearly separated sections:

1. **Needs attention:** unresolved issues, named by the affected feature/save, with an impact sentence and appropriate existing action. No old warnings that have already resolved.
2. **In progress:** real current operations only. Keep one row per operation and update that row through its stages.
3. **Recent activity:** a bounded list of user-meaningful outcomes, with older results collapsed or available through the existing history mechanism.
4. **Technical log:** collapsed by default; retain search, severity filters, follow, copy, and open-log-folder functionality.

For example, a single transfer progresses from `Pokémon Red — Preparing upload…` to `Uploading to RomM…` and ends as `Pokémon Red — Uploaded to RomM · 2 minutes ago` in Recent activity. Captured/uploading/verified events remain in details/logs. Distinct save revisions remain distinct operations; do not merge unrelated work just because the game title matches.

A running games folder is persistent status, not an indefinitely pending operation. Represent its successful start as a completed event. Empty In progress should say **No operations in progress**, without suppressing a separate unresolved issue.

Inspect existing ROM-download reporting and reuse it here where available, showing the game and actual progress. Keep downloads triggered by game access distinct from catalogue checks and save transfers. This does not require a new download-management system.

Inspect the current **Clear** command. If it only clears completed UI history, label it **Clear completed activity**. If its effect differs, name that effect and keep its current boundaries. It must not acquire journal, save, backup, or server-deletion side effects.

Use a stable scrolling region. Following new logs must not drag a user away from a message they are inspecting. Make full-message expansion work inline or in an existing details surface, without rows of detached `More` buttons.

### Settings and existing setup

Inspect the existing form before editing it. Group current fields under **RomM connection**, **Games folder**, **RetroBat**, and **Application**, where applicable. Relocate existing advanced controls under an Advanced disclosure; do not add speculative settings.

Use persistent labels, concise purpose text, and the existing folder picker. Keep one clear Apply/Save model per section; avoid mixing immediate changes and staged values without indication. Test connection and form validation must acknowledge progress, retain entered values on failure, and place errors beside the responsible field.

Preserve the tested distinction between closing to the tray and explicit Quit. Explain that distinction in the existing application settings/help location. During explicit Quit, show **Closing RomM-FS…** while cleanup runs and prevent repeated quit requests. Do not make the application disappear before required cleanup or bypass cleanup to reduce apparent delay.

## State and feedback rules

Implement these rules through existing models and shared selectors/presentation helpers. Do not create a replacement event architecture.

| Authoritative fact                                                                           | Display                                                                                                           |
| -------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| Save association exists, but current sync result is unknown                                  | Association details plus **Not checked** or **Checking for changes…**, as appropriate; not a green success claim. |
| User explicitly switched automatic sync off                                                  | **Automatic save sync is off**; retain last known history. Checking must not turn it on.                          |
| Discovery/reconciliation is actually running                                                 | **Checking for changes…**; consent remains independently On/Off.                                                  |
| An eligible transfer is queued                                                               | **Waiting to upload** / **Waiting to download**.                                                                  |
| Transfer is actively executing                                                               | **Uploading to RomM…** / **Downloading from RomM…**.                                                              |
| Current save verified against the appropriate remote baseline, with no newer unresolved work | **Up to date**, scoped to that save and last successful check.                                                    |
| Connection unavailable and pending work cannot proceed                                       | **Waiting for RomM** with the known issue; do not claim active transfer or silently drop work.                    |
| Existing safety rule rejects a remote revision                                               | **Needs review**, with the specific protective explanation.                                                       |
| Actual supported two-sided conflict                                                          | **Conflicting changes**, linked to the existing supported workflow.                                               |
| Expected unsupported/auxiliary file                                                          | A skipped-file reason, outside the pending/error counts.                                                          |
| Existing records have no reliable historic scan timestamp                                    | **Last scan time unavailable**, not `No save scan yet`.                                                           |

For aggregate status, separate consent from work. Within enabled sync, surface blocked/actionable saves before normal progress; then active transfers, queued work, checking, and confirmed up-to-date status. Retain secondary counts so a problem in one save does not conceal work on another. Report affected saves and remote versions as different units.

If the backend permits an already-started transfer to finish or stop after automatic sync is switched off, show the Off setting alongside that operation's actual progress until it reaches its terminal state. Preserve existing cancellation/finish semantics and keep unresolved review issues visible.

Start/Stop/check actions must render their pending state before awaiting long work. Use the real completion/failure event to update the screen. Do not rely on switching pages, a later timer, or an optimistic assumption that the operation succeeded. Preserve genuine queued work across restart. Remove each completed revision/operation from pending counts on its own terminal completion. Keep newer unresolved work pending; an older completion must not overwrite its state. The save becomes Up to date only when current verification is valid and no unresolved work remains.

Refresh must never flash or persist **Disabled** unless consent actually changed. Ignore stale completion results from an older account/session or superseded operation using existing request identity where available. A late callback must not undo a user's disable action.

Keep the last successful scan/check time separate from the time an attempt started. A completed scan finding zero files still updates the scan result. Do not infer a scan timestamp from transfer history.

## Implementation steps

Use the repository's instructions and build workflow. The report used `mise x -- mbx` on Windows, but verify the current instructions rather than assuming the old command list is still correct. Keep this plan in the existing UX documentation, or use `docs/ux-remediation.md` if no suitable document exists. Add a compact progress section with current step, changes, verification, and remaining blockers.

### Step 1 Map the current implementation and reproduce the failures

- Identify the actual screen components, controllers, state sources, event subscriptions, action handlers, and relevant tests. Record their real paths in the progress section; do not guess them from crate names.
- Map Start, Stop, close, Quit, both Refresh actions, Restart folder, save consent, retry, Clear, and existing ROM-download reporting to their effects and completion signals.
- Reproduce the stale status and wrong-folder reports. Capture baseline screenshots for the states below. Record the exact build/commit and any local modifications used.
- Recheck current build failures. The supplied report mentions undefined `gamelist`, unresolved `LocalFree`, and undefined `NoopHydratedRemover`; those are historical observations, not instructions to patch a different checkout blindly.

**Done when:** actual action semantics and current state ownership are known, the affected build is identified, and existing validation blockers are distinguished from new defects. Limit investigation to decisions needed for these fixes.

### Step 2 Make state and lifecycle controls trustworthy

- Correct pending/completed save counts and derive consistent summaries for Overview, Saves, and Activity.
- Separate persisted sync consent from checking and transfer state. Fix stale completion handling where reproduced.
- Keep controls in place while busy, prevent duplicate operations, and update the current screen on completion.
- Fix Open games folder to use the configured absolute root, including paths with spaces. Show an actionable error instead of falling back to the working/home directory.
- Make Stop and Quit visibly acknowledge progress while preserving cleanup and tray behaviour.

**Done when:** repeated navigation is unnecessary to observe completion; all screens agree; completed history does not become queued work after restart; checks do not change consent.

### Step 3 Simplify Overview and standardise wording

- Implement the three-section Overview and the vocabulary/control contracts.
- Group RetroBat setup and game-information status, keeping the permission warning separate.
- Explain pending library changes and the interruption caused by applying them.
- Replace dense counts with useful summaries and discoverable details. Route summary actions to the actual affected records.
- Apply consistent field/action wording to existing Settings/setup without expanding their capabilities.

**Done when:** a user can identify games-folder state, next action, save state, and any affected optional feature without reading a technical log.

### Step 4 Rework the Saves list and review flow

- Implement one current row per existing save identity with readable titles, actual sync states, and useful result/action fields.
- Separate eligible unlinked saves from skipped files and correct verified backup reasons.
- Replace the fake Manual row with the secondary linking action.
- Correct scan/check labels and implement accurate empty/filter states.
- Group ambiguous remote history beneath the affected save and apply the protective-pause wording without changing its verification rules.

**Done when:** the supplied ambiguous-history case appears as one affected save with nested remote records; newly linked saves are not automatically labelled synced; expected exclusions are not presented as unfinished work.

### Step 5 Rework Activity around operations and outcomes

- Implement Needs attention, In progress, Recent activity, and collapsed Technical log.
- Group stage events by existing operation identity, keeping newer revisions and separate save files distinct.
- Replace raw default labels with game/action/outcome/time. Preserve full technical records in details.
- Fix ambiguous Clear/More controls and scrolling behaviour.

**Done when:** one completed upload yields a terminal user-visible outcome and no pending copy of its historic stages; logs remain available for diagnosis.

### Step 6 Resolve the isolated functional and responsiveness defects

Treat these as distinct fixes with their own evidence, not reasons to redesign the backend:

- **Metadata publication:** inspect the failing validation call, native reparse tag, managed-root identity, and ownership. The report's shared-save-guard explanation is a hypothesis. If confirmed, make the smallest correction specific to metadata publication into the app-owned root. Keep save-path validation strict, reject untrusted traversal, and retain existing metadata if publication fails. CFAPI placeholders use reparse points, which are not synonymous with symbolic links; this explains why the guard deserves inspection, not why it should be removed. See [Microsoft's reparse-point compatibility section](https://learn.microsoft.com/en-us/windows/win32/cfApi/build-a-cloud-file-sync-engine#compatibility-with-applications-that-use-reparse-points).
- **403 favourites/history:** confirm the failed endpoint and current permission response. Present the feature-specific failure; do not expand permissions, replace credentials, or attribute it to the metadata path error without evidence.
- **Hover/rendering lag:** measure the reproduced interaction and check whether UI-thread work, event volume, expensive formatting, or list rendering explains it. Apply the smallest measured fix using the existing toolkit. Do not assume logs cause it, add arbitrary polling, or introduce a performance framework. Keep full diagnostic logging available.

Add concise operation start/completion/failure context where needed to diagnose affected flows, using existing logging conventions. Do not spam a log on every render or create new telemetry infrastructure.

**Done when:** each reproduced defect is corrected and verified, or has a specific remaining blocker and accurate UI. A copy change does not count as fixing metadata publication. Continue unaffected work when a product decision or environment permission blocks one item.

### Step 7 Apply visual consistency and verify the edited journeys

- Reuse the current dark surfaces and blue accent, existing font, and existing widgets. Establish consistent spacing, heading sizes, button heights, field labels, and row alignment using existing tokens or a small local set.
- Keep status text near its object. Use blue/neutral treatment for active work, green for confirmed success, amber for review/warnings, and red for failures. Always include words/icons so colour is not the only signal.
- Use roughly 14–16 logical-pixel body text and readable secondary text; verify actual rendering and scaling rather than forcing fixed physical pixels. Meet normal-text contrast of at least 4.5:1 and large-text contrast of 3:1.
- Keep primary controls visible and secondary actions compact. Long paths/errors must wrap, truncate with an accessible full value, or expand without pushing controls off-screen.
- Check keyboard navigation, visible focus, activation, row disclosure, form labels, and accessible status names through the existing UI toolkit. Microsoft's [Windows accessibility guidance](https://learn.microsoft.com/en-au/windows/apps/develop/accessibility) covers these requirements; no toolkit migration is requested.
- Inspect the current screenshot-size window (1162 × 952), the application's supported minimum size, and Windows 125%/150% scaling. Verify scrolling and focus during live updates.

**Done when:** the verification checklist passes for the affected scope and the agent has inspected the rendered application, not merely compiled it.

## Focused verification checklist

Use existing tests and fixtures. Add automated coverage for meaningful state/consent regressions, not for static labels or implementation-shaped snapshots. Do not turn this into the application's entire unfinished acceptance programme.

- [ ] **State transitions:** slow start, successful stop, failed start/stop, duplicate clicks, and late callbacks produce accurate visible states. Completion updates without navigation. Repeat at least one sequence with background save activity.
- [ ] **Pending work:** verified completed revisions are absent from pending counts; genuine queued work remains after restart; a newer revision is not hidden by an older completion.
- [ ] **Consent:** check while disabled, disable while checking, and restart with persisted consent. Neither temporary UI state nor a late result enables syncing or presents a false disable transition.
- [ ] **Saves:** distinguish linked-but-unchecked, verified up to date, waiting, active, review, eligible unlinked, and expected skipped files. Check a completed empty scan and restored records without a timestamp.
- [ ] **Protective refusal:** use a controlled fixture for unverified remote history. Show one affected save and accurate nested versions. Confirm no remote adoption, local replacement, backup deletion, or journal reset was introduced.
- [ ] **Normal conflicts only if touched:** test with valid owned revisions from an existing fixture or supported second client. A renamed browser upload is not evidence for the normal two-sided conflict path.
- [ ] **Folder lifecycle on Windows:** open the configured root; enumerate; read a known small ROM; stop; remount and read again. Check the expected placeholder cleanup while retaining the existing policy for frontend files. Close-to-tray continues serving reads; explicit Quit performs cleanup.
- [ ] **Lazy access:** opening Overview/Saves/Activity or checking the catalogue does not read ROM contents. If those paths changed, use existing request/download instrumentation to verify no ROM hydration caused by the UI. Repeated hashes alone do not prove cache reuse or no-download enumeration.
- [ ] **RetroBat and ordinary saves:** list and launch an exercised game. If affected code handles saves, verify a supported ordinary save upload and status completion using a test copy/fixture or a specifically authorised live test. Preserve original progress and revisions.
- [ ] **Metadata:** if repaired, verify the artwork/game-information result independently. For failure fixtures, retain the correct partial-success state and existing metadata. Validate path protection for the touched publication path; no blanket reparse bypass.
- [ ] **Rendered UX:** compare before/after Overview stopped/preparing/available/partial failure, Saves healthy/review/skipped/empty, and Activity active/completed. Check long text, keyboard use, scaling, and live updates.

Report a blocked native test as blocked. Do not count compilation, helper-script tests, or screenshots as proof of native runtime behaviour. Offline queues, cancellation coverage, clean-host driver audits, and second-device acceptance already have gaps in the report; cover them only if the changes affect those behaviours or the repository requires them.

## Agent working and completion rules

- Make one implementation step at a time. Preserve unrelated work in a dirty checkout. Use small focused commits according to repository instructions; do not bundle unrelated repairs into this change.
- Update the progress section after each step with what changed, what ran, the observed result, and the next remaining issue. Record concrete blockers without repeatedly stopping the whole task for routine wording/layout choices.
- Reuse existing dependencies, controls, async mechanisms, and test tooling. Do not install a design system or switch UI frameworks to carry out this plan.
- Do not remove a capability just to make a screen simpler. Move advanced information into details while retaining the existing operation and diagnostic path.
- Treat all screenshot numbers and messages as examples of states, not static production data.
- Finish with a concise change summary, before/after screenshots of the affected states, the actual commands and build tested, and remaining defects or unverified cases. Mark the supplied two-sided conflict test inconclusive unless a valid new test establishes its result.

**Completion standard:** the user can understand the current state and take the appropriate next action on the relevant screen; the core games-folder, RetroBat, and supported save behaviour remains intact; and the remaining functional limitations are accurately represented.

## Evidence index

The filenames below identify the supplied attachments. They are not source-code paths. Read `human-testing-2026-10-17/report.md` for human observations and evidence limitations. Keep its original contents unchanged.

| Ref | Supplied screenshot                                        | Relevant state                                                   |
| --- | ---------------------------------------------------------- | ---------------------------------------------------------------- |
| S01 | `rommfs-app_qVG5bdhZKj.png`                                | Overview stopped; unmatched summary; raw counts.                 |
| S02 | `rommfs-app_PaxHrgvogh.png`                                | Overview preparing; missing start control.                       |
| S03 | `rommfs-app_aicyKCa4eG.png`                                | Activity preparing entries; technical log.                       |
| S04 | `rommfs-app_Gxfitb5tpA.png`                                | Activity mount success; metadata failure; permission warning.    |
| S05 | `rommfs-app_sSDoEcKQLD.png`                                | Available games; raw metadata error on Overview.                 |
| S06 | `rommfs-app_vOwSt0Uck7.png`                                | Remounted Overview with persistent metadata error.               |
| S07 | `codex-clipboard-122e06d0-99b7-4b02-ac9f-1c8486d7d147.png` | Captured/uploading/verified rows and outgoing count.             |
| S08 | `rommfs-app_b02GtaRVMy.png`                                | Activity stages, completed controls, and dense log layout.       |
| S09 | `codex-clipboard-dca3dc46-bd80-4dd4-9d98-6745697ff402.png` | Multiple verified revisions still listed under Operations.       |
| S10 | `rommfs-app_nye92MZPni.png`                                | Zero mapped saves; excluded locations counted as unmatched.      |
| S11 | `codex-clipboard-23259219-6856-4cb7-8f33-93c1f742c66d.png` | Two linked saves after refresh; false scan message.              |
| S12 | `codex-clipboard-50bdd8e9-e82c-49ed-a0cf-974b0e175e5d.png` | Expanded save details dominated by IDs/hashes.                   |
| S13 | `rommfs-app_Xcqr0BfarJ.png`                                | Update ready and Restart folder.                                 |
| S14 | `codex-clipboard-1cec1022-53e4-4744-af40-ff529ffc915c.png` | Restart Activity reports two outgoing.                           |
| S15 | `codex-clipboard-8b1649ad-eb5e-4109-bb6e-2cbda42db188.png` | Restart Saves retains mapping but reports syncing.               |
| S16 | `codex-clipboard-7a41b6b0-0ac8-4762-bb02-f468d2aab712.png` | Overview with save attention and metadata failure.               |
| S17 | `codex-clipboard-a058e9fb-7a5b-420b-9d86-4cac175b0e0b.png` | Protective refusal; ambiguous history; misleading backup reason. |
| S18 | `codex-clipboard-786a6fe5-1d97-4101-8b21-9818e939cd06.png` | Activity incoming/review counts and refusal log.                 |

External references establish platform behaviour and basic accessibility requirements. The proposed information architecture, wording, and layout are design decisions made for this application.
.scratch/design:
total 244
drwxr-xr-x 2 aaron aaron  4096 Oct 25 21:42 .
drwxrwxr-x 4 aaron aaron  4096 Oct 26 08:46 ..
-rw------- 1 aaron aaron     0 Oct 25 21:36 .pupdate.hook.lock
-rw-r--r-- 1 aaron aaron  2918 Oct 25 21:41 Buttons Section.png
-rw-r--r-- 1 aaron aaron  2254 Oct 25 21:41 Nav item Section.png
-rw-r--r-- 1 aaron aaron  1667 Oct 25 21:41 Pills and badges Section.png
-rw-r--r-- 1 aaron aaron  3932 Oct 25 21:41 Settings row Section.png
-rw-r--r-- 1 aaron aaron 14666 Oct 25 21:41 Settings · Application.png
-rw-r--r-- 1 aaron aaron 18023 Oct 25 21:41 Settings · Games folder.png
-rw-r--r-- 1 aaron aaron 18802 Oct 25 21:41 Settings · RetroBat.png
-rw-r--r-- 1 aaron aaron 13546 Oct 25 21:41 Settings · RomM connection.png
-rw-r--r-- 1 aaron aaron 22745 Oct 25 21:41 Settings · Saves.png
-rw-r--r-- 1 aaron aaron  1000 Oct 25 21:41 Toggle Section.png
-rw------- 1 aaron aaron 20632 Oct 25 21:41 activity.png
-rw------- 1 aaron aaron 30852 Oct 25 21:41 components.png
-rw------- 1 aaron aaron 26489 Oct 25 21:41 overview.png
-rw------- 1 aaron aaron 35564 Oct 25 21:41 saves.png

.scratch/human-testing-2026-10-26:
total 1040
drwxrwxr-x 2 aaron aaron   4096 Oct 26 08:46 .
drwxrwxr-x 4 aaron aaron   4096 Oct 26 08:46 ..
-rw-rw-r-- 1 aaron aaron  69785 Oct 26 08:46 10-26-26-192216.png
-rw-rw-r-- 1 aaron aaron  41277 Oct 26 08:46 10-26-26-192225.png
-rw-rw-r-- 1 aaron aaron  76771 Oct 26 08:46 10-26-26-192231.png
-rw-rw-r-- 1 aaron aaron  82116 Oct 26 08:46 10-26-26-192235.png
-rw-rw-r-- 1 aaron aaron  48755 Oct 26 08:46 10-26-26-192304.png
-rw-rw-r-- 1 aaron aaron  36693 Oct 26 08:46 10-26-26-192308.png
-rw-rw-r-- 1 aaron aaron  50319 Oct 26 08:46 10-26-26-192312.png
-rw-rw-r-- 1 aaron aaron  50430 Oct 26 08:46 10-26-26-192315.png
-rw-rw-r-- 1 aaron aaron  61861 Oct 26 08:46 10-26-26-192318.png
-rw-rw-r-- 1 aaron aaron  42313 Oct 26 08:46 10-26-26-192322.png
-rw-rw-r-- 1 aaron aaron  83971 Oct 26 08:46 10-26-26-192330.png
-rw-rw-r-- 1 aaron aaron  83867 Oct 26 08:46 10-26-26-192345.png
-rw-rw-r-- 1 aaron aaron  55527 Oct 26 08:46 10-26-26-192848.png
-rw-rw-r-- 1 aaron aaron  76429 Oct 26 08:46 10-26-26-192854.png
-rw-rw-r-- 1 aaron aaron 157914 Oct 26 08:46 10-26-26-193202.png
-rw-rw-r-- 1 aaron aaron    868 Oct 26 08:46 notes.md

]]></tool_result>
<tool_call id="toolu_01FpE5AcA1zsM247nHYjQtVh" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"sed -n 1,200p docs/validation/2026-10-12.md | head -150","description":"Read validation record status"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ sed -n 1,200p docs/validation/2026-10-12.md | head -150
]]></tool_body>
<tool_result><![CDATA[
# Approved-scope implementation status

Updated 2026-10-12. Scope authority: [approved plan](../../.scratch/archived/planning/PLAN.md).
This file records implementation, not release or native
acceptance. `.scratch/STATE.md` is generated separately and is not edited here.

## Implemented in this checkout

- Browser device pairing, cancellation/rejection, Windows Credential Manager
  storage, authenticated account verification, logout/session isolation.
  Paired tokens have no refresh route; rejection requires pairing again.
- Account/server-scoped SQLite catalogue snapshots, warm restoration, one
  background refresh per connected session, explicit refresh, immutable mounted
  trees, and catalogue reuse on mount/remount.
- Generic and capability-checked RetroBat projections, original per-file
  downloads, nested paths, file-identity download rows, Switch update/DLC `.nsp`
  projection to flat `switchupdates`, and auxiliary/game distinction.
- Library/Mount, Save sync, Activity and Settings views, real catalogue/native
  progress, bounded successful history, and rotating diagnostic files.
- Explicit fresh RetroBat setup: reject/list unowned ROM files without touching
  them; permit empty subdirectories; record link ownership/recovery intent;
  verify existing link targets. Windows symlink privilege is required.
- Targeted `switch.yuzu_mutualize` frontend configuration, local config backups,
  per-game disabling-override rejection, effective save-directory resolution,
  and separate missing-emulator diagnostics. RetroBat generates the distinct
  Eden/Citron emulator INI keys on launch.
- Per-platform XML generation from actual projected paths; retained frontend
  favourites/history by stable ROM/file identity; interrupted-publication state
  index; bounded server-local artwork caching without ROM-body requests.
- Native ownership roles for ROM placeholders, ordinary directories,
  gamelists, artwork and frontend state. Metadata survives stop/remount.
  Directory ACLs deny `FILE_DELETE_CHILD`; ROM files retain protected ACLs.
  Ready metadata waits for exclusive frontend/emulator access before publishing.
- Fixed a save scheduler race where a generation observed during reconciliation
  could be dropped as unchanged when its watcher hint arrived. A deterministic
  regression reproduces the previously stranded generation and duplicate guard.
- HTTP fixture shutdown now releases held download/upload barriers after a
  failing test, instead of hanging during panic cleanup.
- Official stable installer discovery, approved HTTPS redirects, streamed
  temporary downloads, release size/SHA-256 verification, cache validation,
  pre-launch integrity recheck, interactive wizard tracking and cancellation.
  Installer cancellation belongs to its wizard; RomMFS never kills a writing
  installer or removes its partial installation. Reuse setup to verify the
  selected completed installation; no guessed unattended switches.
- Ordinary-file save roots, reversible remote relative-path slots, per-file
  identities/status, companion/nested/extensionless/empty/large-file streams,
  source save-target associations and explicit unmatched-file mapping.
  One account-wide inventory request discovers remote mappings; incoming
  watcher feedback does not upload duplicate revisions. New local files are
  discovered without waiting for the fallback scan.
- Versioned broader-scope consent and transactional legacy identity migration;
  preserves baselines, removal state, queued snapshots and incoming bytes.
  Windows legacy separators are normalized without changing destination meaning.
  SRAM-only consent cannot authorize new files. Shared saves/cards, redirected
  paths and overlapping ROM/settings/auxiliary roots are refused. Paths exceeding
  RomM's 255-character slot contract are reported, never truncated.
- Remote-only changes can replace an unchanged synchronized baseline: journal
  a backup first, deny active Windows writers/deletion, recheck content and
  publish without clobbering a concurrent target. Two-sided changes and deliberate
  removals preserve both versions for review; incoming bytes remain exportable.
- Explicit asynchronous auxiliary materialization using verified firmware and
  original cheat-file routes. Supported destinations: effective BIOS directory,
  installed Eden/Citron portable keys and unpacked `.nca` firmware, and effective
  RetroArch `.cht` directory. Ownership/version intent precedes publication;
  retries preserve unowned or user/emulator-modified files. Logout/Quit cancel
  and join the job; partial downloads are never published.
- Application session owns controller/workers independently of window hiding.
  Windows tray status/Open/Quit, session-local single-instance ownership,
  application/taskbar/tray cartridge icon and per-user optional login registration.
  Ordered hidden startup verifies sign-in, mounts, then resumes existing consent.
  Failed startup mounts block later background refreshes from starting save sync.
- Final lifecycle review made save-transfer, artwork and catalogue threads owned
  and joined during stop/logout/Quit, rather than detached. Deterministic tests
  cover blocked-transfer shutdown, stale-event cancellation, new-file discovery,
  failed-startup refresh gating, per-file status and versioned debounce persistence.

## Next steps after the UI merge

The [UI plan](../../.scratch/archived/planning/UI-PLAN.md) landed in merge `6643626`. The branch adds
Overview, state-specific sign-in and folder actions, reorganized Settings,
searchable save review with 50-row pages, and Activity operation summaries.
Its final fixes preserve folder settings across reconnect/reauthentication,
scope the mount path to its session, and apply Settings edits to the session.
These are implementation observations; they do not close runtime acceptance.

Status on 2026-10-12 (Windows Server 2022 host, diagnostic evidence):

1. ~~Complete UI-plan task 6 on native Windows 10/11.~~ Done on Server 2022:
   build + AUTO-*/NATIVE-* selectors green, CFAPI mount/list/read/stop on the
   local demo server passed, `UI-*` regression cases executed (see ledger
   below). Windows 10/11 client remains the only substitute check.
2. ~~Retest the reported fresh-RetroBat setup problem using a real disposable
   installation~~ Done: SETUP-* retest passed (conflict refusal, idempotent
   re-run, foreign link, recovery, missing-emulator diagnostics); real save
   upload/conflict/export/offline cases SAVE-01..07 passed; tray and login
   startup passed. Zero-filled ROMs still cannot validate game launch or
   emulator-created saves.
3. ~~Fix reproducible failures~~ Done: four Windows-only bugs fixed
   (verified-download staging share-mode collision, FILE_RENAME_INFO
   marshalling, held-source upload test, 8.3/verbatim and separator path
   compares). Pending acceptance entries replaced with dated evidence below.
4. ~~Remove superseded UI rendering paths~~ Done: old `window.rs` renderers
   and the page keep-alive pins removed; live helpers kept. No new behavior.

Remaining runtime acceptance: Windows 10/11 client run, UI-KEYBOARD, resize
persistence, RomM-UI revoke leg, real logoff Run-key cycle, installer
execution, CFAPI ACL denial-matrix cases, Credential Manager/browser
approval, and the six runtime defects listed in the ledger.

The root [development Compose file](../../compose.dev.yaml) and
[demo generator](../../scripts/create-demo-roms.py) provide an isolated local
RomM library. The committed harness remains the source for the strict native
`live_romm` test; generated demo games must not be substituted into that test.

Validation on 2026-10-12: the portable headless suite passed **276 tests**, with
zero failures or ignored tests. The generator passed size/content, rerun,
custom-count/size, invalid-input, and existing-file preservation checks. An
isolated Docker run of the development stack scanned all six platforms and
18 demo games; every authenticated ROM download matched its generated source
byte for byte. Compose validation/lint and the development-credential secret
scan passed. These checks ran on Linux; native Windows acceptance remains open.

## Compatibility limits and remaining validation

- Shared saves/cards remain outside the approved release. Known shared-card
  directories are conservatively excluded; game-specific card-format files in
  supported game paths are ordinary files, not parsed or converted.
- Firmware archives and non-RetroArch cheat formats lack supported extraction/
  destination contracts and get actionable diagnostics. Changed server auxiliary
  versions preserve the installed bytes; explicitly remove an unchanged owned
  file before rematerializing the new version. No BIOS/keys are sourced by RomMFS.
- Slot/path and server/disk limits are explicit compatibility constraints, not
  silent truncation or emulator-format restrictions.
- New contracts are source/fixture-backed, not live-validated against the original
  RomM 5.3.1 instance. The reported live HTTP 500 cause remains unknown; diagnostics
  and portable failure/retry checks are implemented, not a claimed reproduction.
- Windows runtime acceptance ran on Server 2022 on 2026-10-12 (diagnostic
  evidence below); the Windows 10/11 client run and the items listed under
  "Remaining runtime acceptance" stay open.

## Reproducible source contracts

RomM source: `95599dadbe93c8f7b8a8647148fafbe293df3167`; see
[API contract](../contracts/romm-api.md). New extensions are source/fixture evidence,
not live RomM 5.3.1 validation.

RetroBat launcher source: `af2dc472fecfaadf851ecf0e92d1e4eb7c519b31`:

- [Eden generator](https://github.com/RetroBat-Official/emulatorlauncher/blob/af2dc472fecfaadf851ecf0e92d1e4eb7c519b31/emulatorLauncher/Generators/Eden.Generator.cs):
  `emulators/eden/user/config/qt-config.ini`, section `[Data%20Storage]`,
]]></tool_result>
<tool_result tool_call_id="toolu_01FpE5AcA1zsM247nHYjQtVh" source="tool_result" timestamp="2026-10-27T09:04:06.467Z"><![CDATA[
# Approved-scope implementation status

Updated 2026-10-12. Scope authority: [approved plan](../../.scratch/archived/planning/PLAN.md).
This file records implementation, not release or native
acceptance. `.scratch/STATE.md` is generated separately and is not edited here.

## Implemented in this checkout

- Browser device pairing, cancellation/rejection, Windows Credential Manager
  storage, authenticated account verification, logout/session isolation.
  Paired tokens have no refresh route; rejection requires pairing again.
- Account/server-scoped SQLite catalogue snapshots, warm restoration, one
  background refresh per connected session, explicit refresh, immutable mounted
  trees, and catalogue reuse on mount/remount.
- Generic and capability-checked RetroBat projections, original per-file
  downloads, nested paths, file-identity download rows, Switch update/DLC `.nsp`
  projection to flat `switchupdates`, and auxiliary/game distinction.
- Library/Mount, Save sync, Activity and Settings views, real catalogue/native
  progress, bounded successful history, and rotating diagnostic files.
- Explicit fresh RetroBat setup: reject/list unowned ROM files without touching
  them; permit empty subdirectories; record link ownership/recovery intent;
  verify existing link targets. Windows symlink privilege is required.
- Targeted `switch.yuzu_mutualize` frontend configuration, local config backups,
  per-game disabling-override rejection, effective save-directory resolution,
  and separate missing-emulator diagnostics. RetroBat generates the distinct
  Eden/Citron emulator INI keys on launch.
- Per-platform XML generation from actual projected paths; retained frontend
  favourites/history by stable ROM/file identity; interrupted-publication state
  index; bounded server-local artwork caching without ROM-body requests.
- Native ownership roles for ROM placeholders, ordinary directories,
  gamelists, artwork and frontend state. Metadata survives stop/remount.
  Directory ACLs deny `FILE_DELETE_CHILD`; ROM files retain protected ACLs.
  Ready metadata waits for exclusive frontend/emulator access before publishing.
- Fixed a save scheduler race where a generation observed during reconciliation
  could be dropped as unchanged when its watcher hint arrived. A deterministic
  regression reproduces the previously stranded generation and duplicate guard.
- HTTP fixture shutdown now releases held download/upload barriers after a
  failing test, instead of hanging during panic cleanup.
- Official stable installer discovery, approved HTTPS redirects, streamed
  temporary downloads, release size/SHA-256 verification, cache validation,
  pre-launch integrity recheck, interactive wizard tracking and cancellation.
  Installer cancellation belongs to its wizard; RomMFS never kills a writing
  installer or removes its partial installation. Reuse setup to verify the
  selected completed installation; no guessed unattended switches.
- Ordinary-file save roots, reversible remote relative-path slots, per-file
  identities/status, companion/nested/extensionless/empty/large-file streams,
  source save-target associations and explicit unmatched-file mapping.
  One account-wide inventory request discovers remote mappings; incoming
  watcher feedback does not upload duplicate revisions. New local files are
  discovered without waiting for the fallback scan.
- Versioned broader-scope consent and transactional legacy identity migration;
  preserves baselines, removal state, queued snapshots and incoming bytes.
  Windows legacy separators are normalized without changing destination meaning.
  SRAM-only consent cannot authorize new files. Shared saves/cards, redirected
  paths and overlapping ROM/settings/auxiliary roots are refused. Paths exceeding
  RomM's 255-character slot contract are reported, never truncated.
- Remote-only changes can replace an unchanged synchronized baseline: journal
  a backup first, deny active Windows writers/deletion, recheck content and
  publish without clobbering a concurrent target. Two-sided changes and deliberate
  removals preserve both versions for review; incoming bytes remain exportable.
- Explicit asynchronous auxiliary materialization using verified firmware and
  original cheat-file routes. Supported destinations: effective BIOS directory,
  installed Eden/Citron portable keys and unpacked `.nca` firmware, and effective
  RetroArch `.cht` directory. Ownership/version intent precedes publication;
  retries preserve unowned or user/emulator-modified files. Logout/Quit cancel
  and join the job; partial downloads are never published.
- Application session owns controller/workers independently of window hiding.
  Windows tray status/Open/Quit, session-local single-instance ownership,
  application/taskbar/tray cartridge icon and per-user optional login registration.
  Ordered hidden startup verifies sign-in, mounts, then resumes existing consent.
  Failed startup mounts block later background refreshes from starting save sync.
- Final lifecycle review made save-transfer, artwork and catalogue threads owned
  and joined during stop/logout/Quit, rather than detached. Deterministic tests
  cover blocked-transfer shutdown, stale-event cancellation, new-file discovery,
  failed-startup refresh gating, per-file status and versioned debounce persistence.

## Next steps after the UI merge

The [UI plan](../../.scratch/archived/planning/UI-PLAN.md) landed in merge `6643626`. The branch adds
Overview, state-specific sign-in and folder actions, reorganized Settings,
searchable save review with 50-row pages, and Activity operation summaries.
Its final fixes preserve folder settings across reconnect/reauthentication,
scope the mount path to its session, and apply Settings edits to the session.
These are implementation observations; they do not close runtime acceptance.

Status on 2026-10-12 (Windows Server 2022 host, diagnostic evidence):

1. ~~Complete UI-plan task 6 on native Windows 10/11.~~ Done on Server 2022:
   build + AUTO-*/NATIVE-* selectors green, CFAPI mount/list/read/stop on the
   local demo server passed, `UI-*` regression cases executed (see ledger
   below). Windows 10/11 client remains the only substitute check.
2. ~~Retest the reported fresh-RetroBat setup problem using a real disposable
   installation~~ Done: SETUP-* retest passed (conflict refusal, idempotent
   re-run, foreign link, recovery, missing-emulator diagnostics); real save
   upload/conflict/export/offline cases SAVE-01..07 passed; tray and login
   startup passed. Zero-filled ROMs still cannot validate game launch or
   emulator-created saves.
3. ~~Fix reproducible failures~~ Done: four Windows-only bugs fixed
   (verified-download staging share-mode collision, FILE_RENAME_INFO
   marshalling, held-source upload test, 8.3/verbatim and separator path
   compares). Pending acceptance entries replaced with dated evidence below.
4. ~~Remove superseded UI rendering paths~~ Done: old `window.rs` renderers
   and the page keep-alive pins removed; live helpers kept. No new behavior.

Remaining runtime acceptance: Windows 10/11 client run, UI-KEYBOARD, resize
persistence, RomM-UI revoke leg, real logoff Run-key cycle, installer
execution, CFAPI ACL denial-matrix cases, Credential Manager/browser
approval, and the six runtime defects listed in the ledger.

The root [development Compose file](../../compose.dev.yaml) and
[demo generator](../../scripts/create-demo-roms.py) provide an isolated local
RomM library. The committed harness remains the source for the strict native
`live_romm` test; generated demo games must not be substituted into that test.

Validation on 2026-10-12: the portable headless suite passed **276 tests**, with
zero failures or ignored tests. The generator passed size/content, rerun,
custom-count/size, invalid-input, and existing-file preservation checks. An
isolated Docker run of the development stack scanned all six platforms and
18 demo games; every authenticated ROM download matched its generated source
byte for byte. Compose validation/lint and the development-credential secret
scan passed. These checks ran on Linux; native Windows acceptance remains open.

## Compatibility limits and remaining validation

- Shared saves/cards remain outside the approved release. Known shared-card
  directories are conservatively excluded; game-specific card-format files in
  supported game paths are ordinary files, not parsed or converted.
- Firmware archives and non-RetroArch cheat formats lack supported extraction/
  destination contracts and get actionable diagnostics. Changed server auxiliary
  versions preserve the installed bytes; explicitly remove an unchanged owned
  file before rematerializing the new version. No BIOS/keys are sourced by RomMFS.
- Slot/path and server/disk limits are explicit compatibility constraints, not
  silent truncation or emulator-format restrictions.
- New contracts are source/fixture-backed, not live-validated against the original
  RomM 5.3.1 instance. The reported live HTTP 500 cause remains unknown; diagnostics
  and portable failure/retry checks are implemented, not a claimed reproduction.
- Windows runtime acceptance ran on Server 2022 on 2026-10-12 (diagnostic
  evidence below); the Windows 10/11 client run and the items listed under
  "Remaining runtime acceptance" stay open.

## Reproducible source contracts

RomM source: `95599dadbe93c8f7b8a8647148fafbe293df3167`; see
[API contract](../contracts/romm-api.md). New extensions are source/fixture evidence,
not live RomM 5.3.1 validation.

RetroBat launcher source: `af2dc472fecfaadf851ecf0e92d1e4eb7c519b31`:

- [Eden generator](https://github.com/RetroBat-Official/emulatorlauncher/blob/af2dc472fecfaadf851ecf0e92d1e4eb7c519b31/emulatorLauncher/Generators/Eden.Generator.cs):
  `emulators/eden/user/config/qt-config.ini`, section `[Data%20Storage]`,
]]></tool_result>
</message>
<message role="assistant" ordinal="7" timestamp="2026-10-27T09:04:08.518Z" has_tool_use="true">
<tool_call id="toolu_01F5FDDNenmX7hiJ2Rudiqq2" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"ls -la .scratch/design .scratch/human-testing-2026-10-26; grep -n '^#' docs/ux-remediation.md | head -60","description":"List scratch dirs and UX doc headings"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ ls -la .scratch/design .scratch/human-testing-2026-10-26; grep -n '^#' docs/ux-remediation.md | head -60
]]></tool_body>
<tool_result><![CDATA[
.scratch/design:
total 244
drwxr-xr-x 2 aaron aaron  4096 Oct 25 21:42 .
drwxrwxr-x 4 aaron aaron  4096 Oct 26 08:46 ..
-rw------- 1 aaron aaron     0 Oct 25 21:36 .pupdate.hook.lock
-rw-r--r-- 1 aaron aaron  2918 Oct 25 21:41 Buttons Section.png
-rw-r--r-- 1 aaron aaron  2254 Oct 25 21:41 Nav item Section.png
-rw-r--r-- 1 aaron aaron  1667 Oct 25 21:41 Pills and badges Section.png
-rw-r--r-- 1 aaron aaron  3932 Oct 25 21:41 Settings row Section.png
-rw-r--r-- 1 aaron aaron 14666 Oct 25 21:41 Settings · Application.png
-rw-r--r-- 1 aaron aaron 18023 Oct 25 21:41 Settings · Games folder.png
-rw-r--r-- 1 aaron aaron 18802 Oct 25 21:41 Settings · RetroBat.png
-rw-r--r-- 1 aaron aaron 13546 Oct 25 21:41 Settings · RomM connection.png
-rw-r--r-- 1 aaron aaron 22745 Oct 25 21:41 Settings · Saves.png
-rw-r--r-- 1 aaron aaron  1000 Oct 25 21:41 Toggle Section.png
-rw------- 1 aaron aaron 20632 Oct 25 21:41 activity.png
-rw------- 1 aaron aaron 30852 Oct 25 21:41 components.png
-rw------- 1 aaron aaron 26489 Oct 25 21:41 overview.png
-rw------- 1 aaron aaron 35564 Oct 25 21:41 saves.png

.scratch/human-testing-2026-10-26:
total 1040
drwxrwxr-x 2 aaron aaron   4096 Oct 26 08:46 .
drwxrwxr-x 4 aaron aaron   4096 Oct 26 08:46 ..
-rw-rw-r-- 1 aaron aaron  69785 Oct 26 08:46 10-26-26-192216.png
-rw-rw-r-- 1 aaron aaron  41277 Oct 26 08:46 10-26-26-192225.png
-rw-rw-r-- 1 aaron aaron  76771 Oct 26 08:46 10-26-26-192231.png
-rw-rw-r-- 1 aaron aaron  82116 Oct 26 08:46 10-26-26-192235.png
-rw-rw-r-- 1 aaron aaron  48755 Oct 26 08:46 10-26-26-192304.png
-rw-rw-r-- 1 aaron aaron  36693 Oct 26 08:46 10-26-26-192308.png
-rw-rw-r-- 1 aaron aaron  50319 Oct 26 08:46 10-26-26-192312.png
-rw-rw-r-- 1 aaron aaron  50430 Oct 26 08:46 10-26-26-192315.png
-rw-rw-r-- 1 aaron aaron  61861 Oct 26 08:46 10-26-26-192318.png
-rw-rw-r-- 1 aaron aaron  42313 Oct 26 08:46 10-26-26-192322.png
-rw-rw-r-- 1 aaron aaron  83971 Oct 26 08:46 10-26-26-192330.png
-rw-rw-r-- 1 aaron aaron  83867 Oct 26 08:46 10-26-26-192345.png
-rw-rw-r-- 1 aaron aaron  55527 Oct 26 08:46 10-26-26-192848.png
-rw-rw-r-- 1 aaron aaron  76429 Oct 26 08:46 10-26-26-192854.png
-rw-rw-r-- 1 aaron aaron 157914 Oct 26 08:46 10-26-26-193202.png
-rw-rw-r-- 1 aaron aaron    868 Oct 26 08:46 notes.md
1:# UI and UX remediation
3:## Product polish — 26 October 2026
40:## Implementation progress — 17 October 2026
47:### Implementation and ownership
98:### Verification and limits
139:## Implementation brief
143:### RomM-FS UX remediation plan
145:## Objective
151:### Scope and evidence
160:## Findings to address
177:## Product model and navigation
196:## Vocabulary and control contracts
221:## Screen specifications
223:### Overview
272:### Saves
299:### Save review and protective pauses
316:### Activity
335:### Settings and existing setup
343:## State and feedback rules
371:## Implementation steps
375:### Step 1 Map the current implementation and reproduce the failures
384:### Step 2 Make state and lifecycle controls trustworthy
394:### Step 3 Simplify Overview and standardise wording
404:### Step 4 Rework the Saves list and review flow
414:### Step 5 Rework Activity around operations and outcomes
423:### Step 6 Resolve the isolated functional and responsiveness defects
435:### Step 7 Apply visual consistency and verify the edited journeys
446:## Focused verification checklist
464:## Agent working and completion rules
475:## Evidence index
]]></tool_result>
<tool_result tool_call_id="toolu_01F5FDDNenmX7hiJ2Rudiqq2" source="tool_result" timestamp="2026-10-27T09:04:09.3Z"><![CDATA[
.scratch/design:
total 244
drwxr-xr-x 2 aaron aaron  4096 Oct 25 21:42 .
drwxrwxr-x 4 aaron aaron  4096 Oct 26 08:46 ..
-rw------- 1 aaron aaron     0 Oct 25 21:36 .pupdate.hook.lock
-rw-r--r-- 1 aaron aaron  2918 Oct 25 21:41 Buttons Section.png
-rw-r--r-- 1 aaron aaron  2254 Oct 25 21:41 Nav item Section.png
-rw-r--r-- 1 aaron aaron  1667 Oct 25 21:41 Pills and badges Section.png
-rw-r--r-- 1 aaron aaron  3932 Oct 25 21:41 Settings row Section.png
-rw-r--r-- 1 aaron aaron 14666 Oct 25 21:41 Settings · Application.png
-rw-r--r-- 1 aaron aaron 18023 Oct 25 21:41 Settings · Games folder.png
-rw-r--r-- 1 aaron aaron 18802 Oct 25 21:41 Settings · RetroBat.png
-rw-r--r-- 1 aaron aaron 13546 Oct 25 21:41 Settings · RomM connection.png
-rw-r--r-- 1 aaron aaron 22745 Oct 25 21:41 Settings · Saves.png
-rw-r--r-- 1 aaron aaron  1000 Oct 25 21:41 Toggle Section.png
-rw------- 1 aaron aaron 20632 Oct 25 21:41 activity.png
-rw------- 1 aaron aaron 30852 Oct 25 21:41 components.png
-rw------- 1 aaron aaron 26489 Oct 25 21:41 overview.png
-rw------- 1 aaron aaron 35564 Oct 25 21:41 saves.png

.scratch/human-testing-2026-10-26:
total 1040
drwxrwxr-x 2 aaron aaron   4096 Oct 26 08:46 .
drwxrwxr-x 4 aaron aaron   4096 Oct 26 08:46 ..
-rw-rw-r-- 1 aaron aaron  69785 Oct 26 08:46 10-26-26-192216.png
-rw-rw-r-- 1 aaron aaron  41277 Oct 26 08:46 10-26-26-192225.png
-rw-rw-r-- 1 aaron aaron  76771 Oct 26 08:46 10-26-26-192231.png
-rw-rw-r-- 1 aaron aaron  82116 Oct 26 08:46 10-26-26-192235.png
-rw-rw-r-- 1 aaron aaron  48755 Oct 26 08:46 10-26-26-192304.png
-rw-rw-r-- 1 aaron aaron  36693 Oct 26 08:46 10-26-26-192308.png
-rw-rw-r-- 1 aaron aaron  50319 Oct 26 08:46 10-26-26-192312.png
-rw-rw-r-- 1 aaron aaron  50430 Oct 26 08:46 10-26-26-192315.png
-rw-rw-r-- 1 aaron aaron  61861 Oct 26 08:46 10-26-26-192318.png
-rw-rw-r-- 1 aaron aaron  42313 Oct 26 08:46 10-26-26-192322.png
-rw-rw-r-- 1 aaron aaron  83971 Oct 26 08:46 10-26-26-192330.png
-rw-rw-r-- 1 aaron aaron  83867 Oct 26 08:46 10-26-26-192345.png
-rw-rw-r-- 1 aaron aaron  55527 Oct 26 08:46 10-26-26-192848.png
-rw-rw-r-- 1 aaron aaron  76429 Oct 26 08:46 10-26-26-192854.png
-rw-rw-r-- 1 aaron aaron 157914 Oct 26 08:46 10-26-26-193202.png
-rw-rw-r-- 1 aaron aaron    868 Oct 26 08:46 notes.md
1:# UI and UX remediation
3:## Product polish — 26 October 2026
40:## Implementation progress — 17 October 2026
47:### Implementation and ownership
98:### Verification and limits
139:## Implementation brief
143:### RomM-FS UX remediation plan
145:## Objective
151:### Scope and evidence
160:## Findings to address
177:## Product model and navigation
196:## Vocabulary and control contracts
221:## Screen specifications
223:### Overview
272:### Saves
299:### Save review and protective pauses
316:### Activity
335:### Settings and existing setup
343:## State and feedback rules
371:## Implementation steps
375:### Step 1 Map the current implementation and reproduce the failures
384:### Step 2 Make state and lifecycle controls trustworthy
394:### Step 3 Simplify Overview and standardise wording
404:### Step 4 Rework the Saves list and review flow
414:### Step 5 Rework Activity around operations and outcomes
423:### Step 6 Resolve the isolated functional and responsiveness defects
435:### Step 7 Apply visual consistency and verify the edited journeys
446:## Focused verification checklist
464:## Agent working and completion rules
475:## Evidence index
]]></tool_result>
</message>
<message role="assistant" ordinal="9" timestamp="2026-10-27T09:04:11.062Z" has_tool_use="true">
<tool_call id="toolu_01RaCkxHW9gRfQiqSbstje8K" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"cat .scratch/human-testing-2026-10-26/notes.md; sed -n 139,200p docs/ux-remediation.md; sed -n 343,370p docs/ux-remediation.md","description":"Read human testing notes and UX findings"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ cat .scratch/human-testing-2026-10-26/notes.md; sed -n 139,200p docs/ux-remediation.md; sed -n 343,370p docs/ux-remediation.md
]]></tool_body>
<tool_result><![CDATA[
- [ ] it says artwork is done syncing, but it's still showing in progress at the same time

- [ ] are we able to point the artwork to a romm url instead of downloading locally?

- [ ] i think windows defender is causing romm to be slow, because it's  using so much ram and cpu

- [ ] `Exclusive access to C:\RetroBat\retrobat.exe is unavailable for RetroBat setup or metadata publication; close RetroBat, its emulators, or any other program using this file: The process cannot access the file because it is being used by another process. (os error 32)` wtf does this mean? i have not run retrobat at all.

- [ ] there's technical details littered around, pretty unfriendly to users. the data should be displayed properly or removed. 

- [ ] if the romm folder is removed from the retrobat roms folder, romm-fs doesn't notice and says everything is a-ok


## Implementation brief

The original requested brief is retained below for the remaining native checks.

### RomM-FS UX remediation plan

## Objective

Improve the existing Windows application so a user can quickly tell whether their games are available, whether supported saves are up to date, what is happening, and what action they need to take. Keep the existing dark appearance and four main destinations. Prioritise truthful status, clear controls, and understandable recovery before cosmetic polish.

This is an implementation brief for a coding agent. Work through the implementation steps in order and complete one coherent change at a time. The intended result is an improved working application, with evidence from the affected journeys.

### Scope and evidence

- Keep the existing Rust application, UI toolkit, Windows CFAPI backend, lazy ROM access, and RetroBat integration. Alternative filesystem backends and other operating systems are outside this change.
- Reuse existing capabilities. Do not add a game browser, launcher replacement, cache manager, new save formats, a new sync engine, or a general framework for status/events. Small shared presentation helpers and changes to incorrect event handling are appropriate.
- Base this work on the supplied `report.md` dated 17 October 2026 and the 18 attached screenshots. The report references 24 screenshots; six are not in this attachment set. Settings and initial setup are not pictured, so recommendations for those surfaces require inspection of the current application.
- Screenshots establish displayed states. The report also records user-observed failures and partial functional testing. It does not establish the cause of lag, prove cache reuse, or demonstrate a failure of the normal two-sided conflict algorithm.
- Preserve local saves, backups, remote revisions, consent settings, and existing path-validation protections. UX work must not silently adopt an unverified remote save or make a failed operation look successful.
- Use an isolated test configuration, fixtures, and test copies for reproduction that could transfer or replace saves. The report leaves final live consent unknown; its server/path examples do not authorise live syncing. For any separately authorised live testing, inspect current consent and account for transfers that startup or restart may trigger. Leave live settings unchanged when preparing isolated scenarios.

## Findings to address

Evidence references such as S01 identify filenames in the evidence index at the end.

| Priority | Observed problem                                                                                                                                             | Required outcome                                                                                                                                                       | Evidence                                              |
| -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------- |
| P0       | Save headers say `Syncing saves…` or `2 outgoing` while completed `verified` events are visible. The state also persists after restart.                      | Current work comes from authoritative operation state. Completed history cannot create pending work. Preserve genuine queued work.                                     | S07–S09, S14–S15; report finding 7                    |
| P0       | The Start button disappears while preparing. Stop reportedly appears ineffective until changing pages; Quit reportedly leaves an unresponsive taskbar entry. | Stable controls acknowledge the operation immediately, prevent duplicate requests, and update on completion without navigation. Shutdown continues to perform cleanup. | S01–S03; report findings 4–5                          |
| P0       | A rejected remote revision produces one affected save plus six `Incoming` history rows and `6 incoming · 1 need review`.                                     | Show one affected save, an accurate protective-pause explanation, and the remote records under its details. Do not imply six downloads are queued.                     | S16–S18                                               |
| P1       | `Linked` describes association while a user needs to know whether their progress reached RomM. Hashes dominate healthy rows.                                 | Game title and current save status lead each row. Mapping and diagnostic information are secondary.                                                                    | S10–S12, S15                                          |
| P1       | `3 unmatched` refers to rows shown as excluded memory-card locations. App-created backups receive the same explanation as unsupported memory cards.          | Count eligible unlinked saves separately from skipped files. Explain each actual exclusion correctly.                                                                  | S10–S12, S17                                          |
| P1       | `No save scan yet` remains visible after files have been discovered and synced.                                                                              | Show the correct scan/check time, or acknowledge that a historic timestamp is unavailable.                                                                             | S10–S12, S15                                          |
| P1       | The folder is `ready` on Overview, `running` in Activity, and `mounting` while preparing. `Update ready` and `Restart folder` omit the purpose and effect.   | Use the vocabulary and action contracts below across all screens.                                                                                                      | S02–S06, S13, S16                                     |
| P1       | Overview displays a raw reparse-point exception beside green availability indicators.                                                                        | Explain the affected capability and impact. Keep games availability, game-information updates, and server permissions distinct.                                        | S04–S06; report findings 1–2                          |
| P1       | Open games folder reportedly opens the home directory.                                                                                                       | Open exactly the configured managed folder, with a visible error if that cannot be done.                                                                               | Report finding 3; not reproduced by these screenshots |
| P2       | Large logs, UUIDs, repeated stage rows, unexplained badges, and a full-width Review saves button compete with primary tasks.                                 | Clear hierarchy, concise summaries, and progressive disclosure of technical information.                                                                               | S01, S07–S12, S17–S18                                 |

## Product model and navigation

Use one sentence to explain the product near the games-folder controls or in existing setup help:

> Your RomM games appear in this folder. Game data downloads when an emulator or another program reads a game file.

The folder being available does not mean every game has been downloaded or that offline play is guaranteed. Reading ROM contents can trigger CFAPI hydration; the application must not read ROM contents merely to render status, calculate a UI summary, or inspect its own catalogue. Microsoft documents this distinction in its [CFAPI overview](https://learn.microsoft.com/en-us/windows/win32/cfApi/build-a-cloud-file-sync-engine).

Retain four destinations, renaming the current **Save sync** destination to **Saves**:

| Destination | Question it answers                                       | Content                                                                                                      |
| ----------- | --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| Overview    | Are my games available, and does anything need attention? | Games folder and primary controls; concise save summary; RetroBat configuration and game-information status. |
| Saves       | What is happening to my progress?                         | Automatic-sync setting, current supported saves, review actions, and skipped-file details.                   |
| Activity    | What is happening now, and what recently finished?        | Unresolved issues, current operations, recent outcomes, and expandable technical logs.                       |
| Settings    | How is this installation configured?                      | Existing connection, folder, RetroBat, and application settings, grouped by purpose.                         |

Keep ROM availability, catalogue freshness, RetroBat setup, artwork/game-information publication, save synchronisation, and RomM connectivity as separate facts. A reachable server does not imply every endpoint is permitted. A successful mount does not imply artwork publication succeeded. Each problem belongs to its affected feature.

## Vocabulary and control contracts

Use these visible labels consistently. A short label is acceptable when its object is clear from the containing section; the accessible name must remain specific.

| Current wording or concept      | Use instead                                                       | Meaning                                                                                      |
## State and feedback rules

Implement these rules through existing models and shared selectors/presentation helpers. Do not create a replacement event architecture.

| Authoritative fact                                                                           | Display                                                                                                           |
| -------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| Save association exists, but current sync result is unknown                                  | Association details plus **Not checked** or **Checking for changes…**, as appropriate; not a green success claim. |
| User explicitly switched automatic sync off                                                  | **Automatic save sync is off**; retain last known history. Checking must not turn it on.                          |
| Discovery/reconciliation is actually running                                                 | **Checking for changes…**; consent remains independently On/Off.                                                  |
| An eligible transfer is queued                                                               | **Waiting to upload** / **Waiting to download**.                                                                  |
| Transfer is actively executing                                                               | **Uploading to RomM…** / **Downloading from RomM…**.                                                              |
| Current save verified against the appropriate remote baseline, with no newer unresolved work | **Up to date**, scoped to that save and last successful check.                                                    |
| Connection unavailable and pending work cannot proceed                                       | **Waiting for RomM** with the known issue; do not claim active transfer or silently drop work.                    |
| Existing safety rule rejects a remote revision                                               | **Needs review**, with the specific protective explanation.                                                       |
| Actual supported two-sided conflict                                                          | **Conflicting changes**, linked to the existing supported workflow.                                               |
| Expected unsupported/auxiliary file                                                          | A skipped-file reason, outside the pending/error counts.                                                          |
| Existing records have no reliable historic scan timestamp                                    | **Last scan time unavailable**, not `No save scan yet`.                                                           |

For aggregate status, separate consent from work. Within enabled sync, surface blocked/actionable saves before normal progress; then active transfers, queued work, checking, and confirmed up-to-date status. Retain secondary counts so a problem in one save does not conceal work on another. Report affected saves and remote versions as different units.

If the backend permits an already-started transfer to finish or stop after automatic sync is switched off, show the Off setting alongside that operation's actual progress until it reaches its terminal state. Preserve existing cancellation/finish semantics and keep unresolved review issues visible.

Start/Stop/check actions must render their pending state before awaiting long work. Use the real completion/failure event to update the screen. Do not rely on switching pages, a later timer, or an optimistic assumption that the operation succeeded. Preserve genuine queued work across restart. Remove each completed revision/operation from pending counts on its own terminal completion. Keep newer unresolved work pending; an older completion must not overwrite its state. The save becomes Up to date only when current verification is valid and no unresolved work remains.

Refresh must never flash or persist **Disabled** unless consent actually changed. Ignore stale completion results from an older account/session or superseded operation using existing request identity where available. A late callback must not undo a user's disable action.

Keep the last successful scan/check time separate from the time an attempt started. A completed scan finding zero files still updates the scan result. Do not infer a scan timestamp from transfer history.
]]></tool_result>
<tool_result tool_call_id="toolu_01RaCkxHW9gRfQiqSbstje8K" source="tool_result" timestamp="2026-10-27T09:04:11.791Z"><![CDATA[
- [ ] it says artwork is done syncing, but it's still showing in progress at the same time

- [ ] are we able to point the artwork to a romm url instead of downloading locally?

- [ ] i think windows defender is causing romm to be slow, because it's  using so much ram and cpu

- [ ] `Exclusive access to C:\RetroBat\retrobat.exe is unavailable for RetroBat setup or metadata publication; close RetroBat, its emulators, or any other program using this file: The process cannot access the file because it is being used by another process. (os error 32)` wtf does this mean? i have not run retrobat at all.

- [ ] there's technical details littered around, pretty unfriendly to users. the data should be displayed properly or removed. 

- [ ] if the romm folder is removed from the retrobat roms folder, romm-fs doesn't notice and says everything is a-ok


## Implementation brief

The original requested brief is retained below for the remaining native checks.

### RomM-FS UX remediation plan

## Objective

Improve the existing Windows application so a user can quickly tell whether their games are available, whether supported saves are up to date, what is happening, and what action they need to take. Keep the existing dark appearance and four main destinations. Prioritise truthful status, clear controls, and understandable recovery before cosmetic polish.

This is an implementation brief for a coding agent. Work through the implementation steps in order and complete one coherent change at a time. The intended result is an improved working application, with evidence from the affected journeys.

### Scope and evidence

- Keep the existing Rust application, UI toolkit, Windows CFAPI backend, lazy ROM access, and RetroBat integration. Alternative filesystem backends and other operating systems are outside this change.
- Reuse existing capabilities. Do not add a game browser, launcher replacement, cache manager, new save formats, a new sync engine, or a general framework for status/events. Small shared presentation helpers and changes to incorrect event handling are appropriate.
- Base this work on the supplied `report.md` dated 17 October 2026 and the 18 attached screenshots. The report references 24 screenshots; six are not in this attachment set. Settings and initial setup are not pictured, so recommendations for those surfaces require inspection of the current application.
- Screenshots establish displayed states. The report also records user-observed failures and partial functional testing. It does not establish the cause of lag, prove cache reuse, or demonstrate a failure of the normal two-sided conflict algorithm.
- Preserve local saves, backups, remote revisions, consent settings, and existing path-validation protections. UX work must not silently adopt an unverified remote save or make a failed operation look successful.
- Use an isolated test configuration, fixtures, and test copies for reproduction that could transfer or replace saves. The report leaves final live consent unknown; its server/path examples do not authorise live syncing. For any separately authorised live testing, inspect current consent and account for transfers that startup or restart may trigger. Leave live settings unchanged when preparing isolated scenarios.

## Findings to address

Evidence references such as S01 identify filenames in the evidence index at the end.

| Priority | Observed problem                                                                                                                                             | Required outcome                                                                                                                                                       | Evidence                                              |
| -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------- |
| P0       | Save headers say `Syncing saves…` or `2 outgoing` while completed `verified` events are visible. The state also persists after restart.                      | Current work comes from authoritative operation state. Completed history cannot create pending work. Preserve genuine queued work.                                     | S07–S09, S14–S15; report finding 7                    |
| P0       | The Start button disappears while preparing. Stop reportedly appears ineffective until changing pages; Quit reportedly leaves an unresponsive taskbar entry. | Stable controls acknowledge the operation immediately, prevent duplicate requests, and update on completion without navigation. Shutdown continues to perform cleanup. | S01–S03; report findings 4–5                          |
| P0       | A rejected remote revision produces one affected save plus six `Incoming` history rows and `6 incoming · 1 need review`.                                     | Show one affected save, an accurate protective-pause explanation, and the remote records under its details. Do not imply six downloads are queued.                     | S16–S18                                               |
| P1       | `Linked` describes association while a user needs to know whether their progress reached RomM. Hashes dominate healthy rows.                                 | Game title and current save status lead each row. Mapping and diagnostic information are secondary.                                                                    | S10–S12, S15                                          |
| P1       | `3 unmatched` refers to rows shown as excluded memory-card locations. App-created backups receive the same explanation as unsupported memory cards.          | Count eligible unlinked saves separately from skipped files. Explain each actual exclusion correctly.                                                                  | S10–S12, S17                                          |
| P1       | `No save scan yet` remains visible after files have been discovered and synced.                                                                              | Show the correct scan/check time, or acknowledge that a historic timestamp is unavailable.                                                                             | S10–S12, S15                                          |
| P1       | The folder is `ready` on Overview, `running` in Activity, and `mounting` while preparing. `Update ready` and `Restart folder` omit the purpose and effect.   | Use the vocabulary and action contracts below across all screens.                                                                                                      | S02–S06, S13, S16                                     |
| P1       | Overview displays a raw reparse-point exception beside green availability indicators.                                                                        | Explain the affected capability and impact. Keep games availability, game-information updates, and server permissions distinct.                                        | S04–S06; report findings 1–2                          |
| P1       | Open games folder reportedly opens the home directory.                                                                                                       | Open exactly the configured managed folder, with a visible error if that cannot be done.                                                                               | Report finding 3; not reproduced by these screenshots |
| P2       | Large logs, UUIDs, repeated stage rows, unexplained badges, and a full-width Review saves button compete with primary tasks.                                 | Clear hierarchy, concise summaries, and progressive disclosure of technical information.                                                                               | S01, S07–S12, S17–S18                                 |

## Product model and navigation

Use one sentence to explain the product near the games-folder controls or in existing setup help:

> Your RomM games appear in this folder. Game data downloads when an emulator or another program reads a game file.

The folder being available does not mean every game has been downloaded or that offline play is guaranteed. Reading ROM contents can trigger CFAPI hydration; the application must not read ROM contents merely to render status, calculate a UI summary, or inspect its own catalogue. Microsoft documents this distinction in its [CFAPI overview](https://learn.microsoft.com/en-us/windows/win32/cfApi/build-a-cloud-file-sync-engine).

Retain four destinations, renaming the current **Save sync** destination to **Saves**:

| Destination | Question it answers                                       | Content                                                                                                      |
| ----------- | --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| Overview    | Are my games available, and does anything need attention? | Games folder and primary controls; concise save summary; RetroBat configuration and game-information status. |
| Saves       | What is happening to my progress?                         | Automatic-sync setting, current supported saves, review actions, and skipped-file details.                   |
| Activity    | What is happening now, and what recently finished?        | Unresolved issues, current operations, recent outcomes, and expandable technical logs.                       |
| Settings    | How is this installation configured?                      | Existing connection, folder, RetroBat, and application settings, grouped by purpose.                         |

Keep ROM availability, catalogue freshness, RetroBat setup, artwork/game-information publication, save synchronisation, and RomM connectivity as separate facts. A reachable server does not imply every endpoint is permitted. A successful mount does not imply artwork publication succeeded. Each problem belongs to its affected feature.

## Vocabulary and control contracts

Use these visible labels consistently. A short label is acceptable when its object is clear from the containing section; the accessible name must remain specific.

| Current wording or concept      | Use instead                                                       | Meaning                                                                                      |
## State and feedback rules

Implement these rules through existing models and shared selectors/presentation helpers. Do not create a replacement event architecture.

| Authoritative fact                                                                           | Display                                                                                                           |
| -------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| Save association exists, but current sync result is unknown                                  | Association details plus **Not checked** or **Checking for changes…**, as appropriate; not a green success claim. |
| User explicitly switched automatic sync off                                                  | **Automatic save sync is off**; retain last known history. Checking must not turn it on.                          |
| Discovery/reconciliation is actually running                                                 | **Checking for changes…**; consent remains independently On/Off.                                                  |
| An eligible transfer is queued                                                               | **Waiting to upload** / **Waiting to download**.                                                                  |
| Transfer is actively executing                                                               | **Uploading to RomM…** / **Downloading from RomM…**.                                                              |
| Current save verified against the appropriate remote baseline, with no newer unresolved work | **Up to date**, scoped to that save and last successful check.                                                    |
| Connection unavailable and pending work cannot proceed                                       | **Waiting for RomM** with the known issue; do not claim active transfer or silently drop work.                    |
| Existing safety rule rejects a remote revision                                               | **Needs review**, with the specific protective explanation.                                                       |
| Actual supported two-sided conflict                                                          | **Conflicting changes**, linked to the existing supported workflow.                                               |
| Expected unsupported/auxiliary file                                                          | A skipped-file reason, outside the pending/error counts.                                                          |
| Existing records have no reliable historic scan timestamp                                    | **Last scan time unavailable**, not `No save scan yet`.                                                           |

For aggregate status, separate consent from work. Within enabled sync, surface blocked/actionable saves before normal progress; then active transfers, queued work, checking, and confirmed up-to-date status. Retain secondary counts so a problem in one save does not conceal work on another. Report affected saves and remote versions as different units.

If the backend permits an already-started transfer to finish or stop after automatic sync is switched off, show the Off setting alongside that operation's actual progress until it reaches its terminal state. Preserve existing cancellation/finish semantics and keep unresolved review issues visible.

Start/Stop/check actions must render their pending state before awaiting long work. Use the real completion/failure event to update the screen. Do not rely on switching pages, a later timer, or an optimistic assumption that the operation succeeded. Preserve genuine queued work across restart. Remove each completed revision/operation from pending counts on its own terminal completion. Keep newer unresolved work pending; an older completion must not overwrite its state. The save becomes Up to date only when current verification is valid and no unresolved work remains.

Refresh must never flash or persist **Disabled** unless consent actually changed. Ignore stale completion results from an older account/session or superseded operation using existing request identity where available. A late callback must not undo a user's disable action.

Keep the last successful scan/check time separate from the time an attempt started. A completed scan finding zero files still updates the scan result. Do not infer a scan timestamp from transfer history.
]]></tool_result>
</message>
<message role="assistant" ordinal="11" timestamp="2026-10-27T09:04:15.608Z" has_thinking="true" has_tool_use="true">
<thinking><![CDATA[
Let me check the human-testing screenshots to see what the user actually experienced.
]]></thinking>
<tool_call id="toolu_01TSDgoWc3aaHPrfqDvVphkx" name="Read" category="Read">
<arguments><![CDATA[
{"file_path":"/home/aaron/Code/romm-fs/.scratch/human-testing-2026-10-26/10-26-26-192216.png"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
/home/aaron/Code/romm-fs/.scratch/human-testing-2026-10-26/10-26-26-192216.png
]]></tool_body>
<tool_result tool_call_id="toolu_01TSDgoWc3aaHPrfqDvVphkx" source="tool_result" timestamp="2026-10-27T09:04:16.009Z"><![CDATA[

]]></tool_result>
<tool_call id="toolu_01VZVMLkka6uuwr3pT5oP2zJ" name="Read" category="Read">
<arguments><![CDATA[
{"file_path":"/home/aaron/Code/romm-fs/.scratch/human-testing-2026-10-26/10-26-26-192231.png"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
/home/aaron/Code/romm-fs/.scratch/human-testing-2026-10-26/10-26-26-192231.png
]]></tool_body>
<tool_result tool_call_id="toolu_01VZVMLkka6uuwr3pT5oP2zJ" source="tool_result" timestamp="2026-10-27T09:04:16.115Z"><![CDATA[

]]></tool_result>
<tool_call id="toolu_01Vwu36zFj3CsMhhmvM3LRmK" name="Read" category="Read">
<arguments><![CDATA[
{"file_path":"/home/aaron/Code/romm-fs/.scratch/human-testing-2026-10-26/10-26-26-193202.png"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
/home/aaron/Code/romm-fs/.scratch/human-testing-2026-10-26/10-26-26-193202.png
]]></tool_body>
<tool_result tool_call_id="toolu_01Vwu36zFj3CsMhhmvM3LRmK" source="tool_result" timestamp="2026-10-27T09:04:16.24Z"><![CDATA[

]]></tool_result>
</message>
<message role="assistant" ordinal="15" timestamp="2026-10-27T09:04:19.557Z" has_thinking="true" has_tool_use="true">
<thinking><![CDATA[
I'll give a quick update, then check more screenshots along with the controller state and how it's derived.
]]></thinking>
I&#39;m reading the recent human-testing notes and screenshots, then I&#39;ll trace where the UI state comes from.
<tool_call id="toolu_0149c8TeDRcQaX6ZmCQ1evzY" name="Read" category="Read">
<arguments><![CDATA[
{"file_path":"/home/aaron/Code/romm-fs/.scratch/human-testing-2026-10-26/10-26-26-192330.png"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
/home/aaron/Code/romm-fs/.scratch/human-testing-2026-10-26/10-26-26-192330.png
]]></tool_body>
<tool_result tool_call_id="toolu_0149c8TeDRcQaX6ZmCQ1evzY" source="tool_result" timestamp="2026-10-27T09:04:19.959Z"><![CDATA[

]]></tool_result>
<tool_call id="toolu_01UWd8XwA2itfN76tu2FUsRA" name="Read" category="Read">
<arguments><![CDATA[
{"file_path":"/home/aaron/Code/romm-fs/.scratch/human-testing-2026-10-26/10-26-26-192854.png"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
/home/aaron/Code/romm-fs/.scratch/human-testing-2026-10-26/10-26-26-192854.png
]]></tool_body>
<tool_result tool_call_id="toolu_01UWd8XwA2itfN76tu2FUsRA" source="tool_result" timestamp="2026-10-27T09:04:20.088Z"><![CDATA[

]]></tool_result>
</message>
<message role="assistant" ordinal="17" timestamp="2026-10-27T09:04:20.021Z" has_tool_use="true">
<tool_call id="toolu_011wAhq8cdpGixuED2sZMmbD" name="Read" category="Read">
<arguments><![CDATA[
{"file_path":"/home/aaron/Code/romm-fs/.scratch/human-testing-2026-10-26/10-26-26-192304.png"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
/home/aaron/Code/romm-fs/.scratch/human-testing-2026-10-26/10-26-26-192304.png
]]></tool_body>
<tool_result tool_call_id="toolu_011wAhq8cdpGixuED2sZMmbD" source="tool_result" timestamp="2026-10-27T09:04:20.384Z"><![CDATA[

]]></tool_result>
</message>
<message role="assistant" ordinal="20" timestamp="2026-10-27T09:04:24.134Z" has_thinking="true" has_tool_use="true">
<thinking><![CDATA[
I want to check how state is modeled in the controller, looking at state.rs and events.rs to see if status strings get derived in multiple places.
]]></thinking>
<tool_call id="toolu_01LTP2z5nfrpR3ZAgyHeDo62" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"grep -n 'pub struct\\|pub enum\\|^    pub [a-z_]*:' crates/rommfs-app/src/controller/state.rs | head -120","description":"List state types and fields in controller state"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ grep -n 'pub struct\|pub enum\|^    pub [a-z_]*:' crates/rommfs-app/src/controller/state.rs | head -120
]]></tool_body>
<tool_result><![CDATA[
5:pub enum ConnectionState {
14:pub enum MountState {
25:pub enum FolderSummary {
52:pub enum FrontendMetadataSummary {
63:pub enum StepStatus {
75:pub enum RetroBatStep {
82:pub struct RetroBatSteps {
83:    pub install: StepStatus,
84:    pub link: StepStatus,
85:    pub support: StepStatus,
130:pub enum SaveSyncSummary {
175:pub struct DownloadView {
176:    pub file_id: u64,
177:    pub rom_id: u64,
178:    pub file_name: String,
179:    pub received: u64,
180:    pub total: Option<u64>,
182:    pub finished: Option<std::result::Result<(), String>>,
186:pub struct SaveSyncTransferView {
187:    pub rom_id: i64,
188:    pub revision: String,
189:    pub updated_at: u64,
190:    pub phase: String,
191:    pub detail: Option<String>,
206:pub struct UiState {
207:    pub closing: bool,
208:    pub auxiliary_running: bool,
209:    pub start_at_login: bool,
210:    pub startup_error: Option<String>,
211:    pub save_file_games: Vec<(i64, String)>,
212:    pub save_file_unmatched: Vec<UnmatchedFile>,
213:    pub auxiliary_detail: Option<String>,
214:    pub auxiliary_error: Option<String>,
216:    pub auxiliary_summary: Option<rommfs_core::events::AuxiliarySummary>,
217:    pub installer_running: bool,
218:    pub installer_ready: bool,
219:    pub installer_progress: (u64, Option<u64>),
220:    pub installer_detail: Option<String>,
221:    pub installer_error: Option<String>,
222:    pub integration: IntegrationMode,
223:    pub retrobat_root: Option<String>,
224:    pub integration_error: Option<String>,
226:    pub integration_notice: Option<String>,
227:    pub retrobat_setup_running: bool,
230:    pub retrobat_setup_ok: bool,
232:    pub retrobat_setup_migration: bool,
233:    pub retrobat_setup_detail: Option<String>,
234:    pub retrobat_setup_error: Option<String>,
236:    pub retrobat_setup_skipped: Vec<rommfs_core::retrobat::SkippedSystem>,
237:    pub frontend_metadata_pending: bool,
238:    pub frontend_metadata_detail: Option<String>,
239:    pub frontend_metadata_error: Option<String>,
242:    pub frontend_metadata_progress: Option<(usize, usize)>,
245:    pub frontend_metadata_updated_at: Option<u64>,
247:    pub play_data_available: bool,
249:    pub play_data_enabled: bool,
250:    pub play_data_problem: Option<String>,
253:    pub hide_copies_enabled: bool,
254:    pub hide_copies_count: usize,
255:    pub pairing_code: Option<String>,
256:    pub conn: ConnectionState,
257:    pub conn_error: Option<String>,
258:    pub mount: MountState,
259:    pub mount_stopping: bool,
260:    pub save_sync_checking: bool,
261:    pub save_scan_at: Option<u64>,
263:    pub folder_open_error: Option<String>,
264:    pub mount_path: Option<String>,
265:    pub mount_error: Option<String>,
267:    pub foreign_root: Option<String>,
268:    pub foreign_entries: Vec<ForeignEntry>,
270:    pub foreign_removal: Option<std::result::Result<usize, String>>,
271:    pub downloads: Vec<DownloadView>,
272:    pub catalogue_progress: Option<(String, u64, Option<u64>, u64)>,
273:    pub mount_progress: Option<(String, u64, Option<u64>, u64)>,
274:    pub catalogue_refreshed_at: Option<u64>,
275:    pub catalogue_pending_remount: bool,
277:    pub catalogue: Option<(usize, usize, usize, usize)>, // platforms, games, files, skipped
278:    pub save_sync_candidates: Vec<InstallationCandidate>,
279:    pub save_sync_skipped: usize,
280:    pub save_sync_selected_root: Option<String>,
281:    pub save_sync_documented_saves_root: Option<String>,
282:    pub save_sync_effective_saves_root: Option<String>,
283:    pub save_sync_profile_version: Option<String>,
285:    pub connected_server: Option<String>,
288:    pub configured_mount_root: Option<String>,
289:    pub save_sync_account_id: Option<i64>,
290:    pub save_sync_mapped_targets: usize,
291:    pub save_sync_catalogue_unmapped: usize,
292:    pub save_sync_existing_saves: Option<rommfs_core::save_sync::ExistingSavePreview>,
293:    pub save_sync_preview: Vec<String>,
294:    pub save_sync_available: bool,
295:    pub save_sync_problem: Option<String>,
296:    pub save_sync_enabled: bool,
298:    pub save_sync_debounce_secs: u32,
299:    pub save_sync_games: Vec<SaveSyncGameStatus>,
300:    pub save_sync_reconciled_games: usize,
301:    pub save_sync_pending_incoming: usize,
302:    pub save_sync_attention_games: usize,
303:    pub save_sync_failure: Option<String>,
304:    pub save_sync_transfers: Vec<SaveSyncTransferView>,
305:    pub save_sync_session_id: u64,
306:    pub save_sync_server_id: Option<String>,
307:    pub save_sync_queue: Option<SaveSyncQueueStatus>,
308:    pub save_sync_authentication_required: bool,
309:    pub save_sync_export_errors: HashMap<String, String>,
310:    pub save_sync_export_feedback: HashMap<String, String>,
313:    pub memory_card_folders: Vec<MemoryCardFolderStatus>,
314:    pub memory_card_conflicts: Vec<MemoryCardConflictStatus>,
315:    pub memory_cards_available: bool,
316:    pub memory_cards_problem: Option<String>,
317:    pub log: LogBuffer,
]]></tool_result>
<tool_result tool_call_id="toolu_01LTP2z5nfrpR3ZAgyHeDo62" source="tool_result" timestamp="2026-10-27T09:04:24.951Z"><![CDATA[
5:pub enum ConnectionState {
14:pub enum MountState {
25:pub enum FolderSummary {
52:pub enum FrontendMetadataSummary {
63:pub enum StepStatus {
75:pub enum RetroBatStep {
82:pub struct RetroBatSteps {
83:    pub install: StepStatus,
84:    pub link: StepStatus,
85:    pub support: StepStatus,
130:pub enum SaveSyncSummary {
175:pub struct DownloadView {
176:    pub file_id: u64,
177:    pub rom_id: u64,
178:    pub file_name: String,
179:    pub received: u64,
180:    pub total: Option<u64>,
182:    pub finished: Option<std::result::Result<(), String>>,
186:pub struct SaveSyncTransferView {
187:    pub rom_id: i64,
188:    pub revision: String,
189:    pub updated_at: u64,
190:    pub phase: String,
191:    pub detail: Option<String>,
206:pub struct UiState {
207:    pub closing: bool,
208:    pub auxiliary_running: bool,
209:    pub start_at_login: bool,
210:    pub startup_error: Option<String>,
211:    pub save_file_games: Vec<(i64, String)>,
212:    pub save_file_unmatched: Vec<UnmatchedFile>,
213:    pub auxiliary_detail: Option<String>,
214:    pub auxiliary_error: Option<String>,
216:    pub auxiliary_summary: Option<rommfs_core::events::AuxiliarySummary>,
217:    pub installer_running: bool,
218:    pub installer_ready: bool,
219:    pub installer_progress: (u64, Option<u64>),
220:    pub installer_detail: Option<String>,
221:    pub installer_error: Option<String>,
222:    pub integration: IntegrationMode,
223:    pub retrobat_root: Option<String>,
224:    pub integration_error: Option<String>,
226:    pub integration_notice: Option<String>,
227:    pub retrobat_setup_running: bool,
230:    pub retrobat_setup_ok: bool,
232:    pub retrobat_setup_migration: bool,
233:    pub retrobat_setup_detail: Option<String>,
234:    pub retrobat_setup_error: Option<String>,
236:    pub retrobat_setup_skipped: Vec<rommfs_core::retrobat::SkippedSystem>,
237:    pub frontend_metadata_pending: bool,
238:    pub frontend_metadata_detail: Option<String>,
239:    pub frontend_metadata_error: Option<String>,
242:    pub frontend_metadata_progress: Option<(usize, usize)>,
245:    pub frontend_metadata_updated_at: Option<u64>,
247:    pub play_data_available: bool,
249:    pub play_data_enabled: bool,
250:    pub play_data_problem: Option<String>,
253:    pub hide_copies_enabled: bool,
254:    pub hide_copies_count: usize,
255:    pub pairing_code: Option<String>,
256:    pub conn: ConnectionState,
257:    pub conn_error: Option<String>,
258:    pub mount: MountState,
259:    pub mount_stopping: bool,
260:    pub save_sync_checking: bool,
261:    pub save_scan_at: Option<u64>,
263:    pub folder_open_error: Option<String>,
264:    pub mount_path: Option<String>,
265:    pub mount_error: Option<String>,
267:    pub foreign_root: Option<String>,
268:    pub foreign_entries: Vec<ForeignEntry>,
270:    pub foreign_removal: Option<std::result::Result<usize, String>>,
271:    pub downloads: Vec<DownloadView>,
272:    pub catalogue_progress: Option<(String, u64, Option<u64>, u64)>,
273:    pub mount_progress: Option<(String, u64, Option<u64>, u64)>,
274:    pub catalogue_refreshed_at: Option<u64>,
275:    pub catalogue_pending_remount: bool,
277:    pub catalogue: Option<(usize, usize, usize, usize)>, // platforms, games, files, skipped
278:    pub save_sync_candidates: Vec<InstallationCandidate>,
279:    pub save_sync_skipped: usize,
280:    pub save_sync_selected_root: Option<String>,
281:    pub save_sync_documented_saves_root: Option<String>,
282:    pub save_sync_effective_saves_root: Option<String>,
283:    pub save_sync_profile_version: Option<String>,
285:    pub connected_server: Option<String>,
288:    pub configured_mount_root: Option<String>,
289:    pub save_sync_account_id: Option<i64>,
290:    pub save_sync_mapped_targets: usize,
291:    pub save_sync_catalogue_unmapped: usize,
292:    pub save_sync_existing_saves: Option<rommfs_core::save_sync::ExistingSavePreview>,
293:    pub save_sync_preview: Vec<String>,
294:    pub save_sync_available: bool,
295:    pub save_sync_problem: Option<String>,
296:    pub save_sync_enabled: bool,
298:    pub save_sync_debounce_secs: u32,
299:    pub save_sync_games: Vec<SaveSyncGameStatus>,
300:    pub save_sync_reconciled_games: usize,
301:    pub save_sync_pending_incoming: usize,
302:    pub save_sync_attention_games: usize,
303:    pub save_sync_failure: Option<String>,
304:    pub save_sync_transfers: Vec<SaveSyncTransferView>,
305:    pub save_sync_session_id: u64,
306:    pub save_sync_server_id: Option<String>,
307:    pub save_sync_queue: Option<SaveSyncQueueStatus>,
308:    pub save_sync_authentication_required: bool,
309:    pub save_sync_export_errors: HashMap<String, String>,
310:    pub save_sync_export_feedback: HashMap<String, String>,
313:    pub memory_card_folders: Vec<MemoryCardFolderStatus>,
314:    pub memory_card_conflicts: Vec<MemoryCardConflictStatus>,
315:    pub memory_cards_available: bool,
316:    pub memory_cards_problem: Option<String>,
317:    pub log: LogBuffer,
]]></tool_result>
<tool_call id="toolu_01FXsRRpjKJ3dLdEueyqYy1S" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"ls crates/rommfs-app/src crates/rommfs-app/src/* -d && wc -l crates/rommfs-app/src/window/*.rs crates/rommfs-app/src/window/pages/*.rs crates/rommfs-app/src/*.rs | sort -n | tail -30","description":"List app source files and sizes"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ ls crates/rommfs-app/src crates/rommfs-app/src/* -d && wc -l crates/rommfs-app/src/window/*.rs crates/rommfs-app/src/window/pages/*.rs crates/rommfs-app/src/*.rs | sort -n | tail -30
]]></tool_body>
<tool_result><![CDATA[
crates/rommfs-app/src
crates/rommfs-app/src/controller
crates/rommfs-app/src/controller.rs
crates/rommfs-app/src/desktop
crates/rommfs-app/src/desktop.rs
crates/rommfs-app/src/fuse_mount.rs
crates/rommfs-app/src/icon.rs
crates/rommfs-app/src/lib.rs
crates/rommfs-app/src/main.rs
crates/rommfs-app/src/problem.rs
crates/rommfs-app/src/retrobat_discovery_windows.rs
crates/rommfs-app/src/retrobat_setup
crates/rommfs-app/src/retrobat_setup.rs
crates/rommfs-app/src/save_sync_agent
crates/rommfs-app/src/save_sync_agent.rs
crates/rommfs-app/src/session.rs
crates/rommfs-app/src/trace_bridge.rs
crates/rommfs-app/src/window
crates/rommfs-app/src/window.rs
    10 crates/rommfs-app/src/main.rs
    10 crates/rommfs-app/src/window/pages.rs
    20 crates/rommfs-app/src/lib.rs
    25 crates/rommfs-app/src/save_sync_agent.rs
    56 crates/rommfs-app/src/icon.rs
    70 crates/rommfs-app/src/session.rs
   101 crates/rommfs-app/src/trace_bridge.rs
   103 crates/rommfs-app/src/retrobat_discovery_windows.rs
   132 crates/rommfs-app/src/retrobat_setup.rs
   147 crates/rommfs-app/src/window/pages/setup.rs
   232 crates/rommfs-app/src/desktop.rs
   243 crates/rommfs-app/src/window/pages/overview.rs
   272 crates/rommfs-app/src/window/actions.rs
   291 crates/rommfs-app/src/fuse_mount.rs
   376 crates/rommfs-app/src/problem.rs
   394 crates/rommfs-app/src/window/folder_controls.rs
   594 crates/rommfs-app/src/window/components.rs
  1044 crates/rommfs-app/src/window/pages/save_review.rs
  1101 crates/rommfs-app/src/window.rs
  1113 crates/rommfs-app/src/window/pages/settings.rs
  1114 crates/rommfs-app/src/window/pages/activity.rs
  1509 crates/rommfs-app/src/controller.rs
  8957 total
]]></tool_result>
<tool_result tool_call_id="toolu_01FXsRRpjKJ3dLdEueyqYy1S" source="tool_result" timestamp="2026-10-27T09:04:25.029Z"><![CDATA[
crates/rommfs-app/src
crates/rommfs-app/src/controller
crates/rommfs-app/src/controller.rs
crates/rommfs-app/src/desktop
crates/rommfs-app/src/desktop.rs
crates/rommfs-app/src/fuse_mount.rs
crates/rommfs-app/src/icon.rs
crates/rommfs-app/src/lib.rs
crates/rommfs-app/src/main.rs
crates/rommfs-app/src/problem.rs
crates/rommfs-app/src/retrobat_discovery_windows.rs
crates/rommfs-app/src/retrobat_setup
crates/rommfs-app/src/retrobat_setup.rs
crates/rommfs-app/src/save_sync_agent
crates/rommfs-app/src/save_sync_agent.rs
crates/rommfs-app/src/session.rs
crates/rommfs-app/src/trace_bridge.rs
crates/rommfs-app/src/window
crates/rommfs-app/src/window.rs
    10 crates/rommfs-app/src/main.rs
    10 crates/rommfs-app/src/window/pages.rs
    20 crates/rommfs-app/src/lib.rs
    25 crates/rommfs-app/src/save_sync_agent.rs
    56 crates/rommfs-app/src/icon.rs
    70 crates/rommfs-app/src/session.rs
   101 crates/rommfs-app/src/trace_bridge.rs
   103 crates/rommfs-app/src/retrobat_discovery_windows.rs
   132 crates/rommfs-app/src/retrobat_setup.rs
   147 crates/rommfs-app/src/window/pages/setup.rs
   232 crates/rommfs-app/src/desktop.rs
   243 crates/rommfs-app/src/window/pages/overview.rs
   272 crates/rommfs-app/src/window/actions.rs
   291 crates/rommfs-app/src/fuse_mount.rs
   376 crates/rommfs-app/src/problem.rs
   394 crates/rommfs-app/src/window/folder_controls.rs
   594 crates/rommfs-app/src/window/components.rs
  1044 crates/rommfs-app/src/window/pages/save_review.rs
  1101 crates/rommfs-app/src/window.rs
  1113 crates/rommfs-app/src/window/pages/settings.rs
  1114 crates/rommfs-app/src/window/pages/activity.rs
  1509 crates/rommfs-app/src/controller.rs
  8957 total
]]></tool_result>
</message>
<message role="assistant" ordinal="23" timestamp="2026-10-27T09:04:31.835Z" has_thinking="true" has_tool_use="true">
<thinking><![CDATA[
UiState is a sprawling flat structure with roughly 100 fields, many of them separate bool/Option pairs for running/ok/detail/error status. This is exactly the kind of design that invites conflicting state, since these independent flags can drift out of sync with each other across different subsystems like auxiliary setup, retrobat setup, and frontend metadata.

Mounting and save sync have similar sprawl across several independent fields tracking state, errors, and progress. I want to check whether the summary enums in state.rs act as derived selectors that reconcile these, so I'm going to look at the first part of that file.
]]></thinking>
<tool_call id="toolu_01CvmSfZDWadcoqrF8J1syVF" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"sed -n 1,205p crates/rommfs-app/src/controller/state.rs","description":"Read state enums and summaries"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ sed -n 1,205p crates/rommfs-app/src/controller/state.rs
]]></tool_body>
<tool_result><![CDATA[
//! Event-derived UI state and the summaries rendered by the window.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    Idle,
    Connecting,
    Connected,
    Failed,
    SignInRequired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountState {
    NotMounted,
    Mounting,
    Mounted,
    Failed,
}

/// The derived games-folder summary the whole UI shares (UI-PLAN §3): a folder
/// is "ready" only after a real MountStarted — never inferred from the
/// persisted enabled setting or the existence of a directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderSummary {
    /// No completed folder setup for this server yet.
    SetupNeeded,
    /// A folder is configured but the provider is stopped.
    Stopped,
    /// A start operation is running.
    Starting,
    /// Provider active and folder usable.
    Ready,
    /// The last start failed (reason in `mount_error`).
    Failed,
}

impl FolderSummary {
    pub fn label(self) -> &'static str {
        match self {
            Self::SetupNeeded => "No games folder chosen yet",
            Self::Stopped => "Games folder stopped",
            Self::Starting => "Preparing games…",
            Self::Ready => "Available",
            Self::Failed => "Couldn't start the games folder",
        }
    }
}

/// Artwork checking is only the first stage; RetroBat must also receive the update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrontendMetadataSummary {
    NotStarted,
    CheckingArtwork,
    Publishing,
    Waiting,
    Failed,
    UpToDate,
}

/// Where one step of the RetroBat flow stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepStatus {
    Done,
    /// Actionable now.
    Ready,
    Running,
    /// The last attempt failed; the action is a retry.
    Failed,
    /// Unavailable until something else happens (the reason).
    Blocked(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetroBatStep {
    Install,
    Link,
    Support,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetroBatSteps {
    pub install: StepStatus,
    pub link: StepStatus,
    pub support: StepStatus,
}

impl RetroBatSteps {
    /// The one step whose action is highlighted: the first that is neither
    /// done nor blocked. Nothing is highlighted while an operation runs.
    pub fn primary(&self) -> Option<RetroBatStep> {
        [
            (RetroBatStep::Install, self.install),
            (RetroBatStep::Link, self.link),
            (RetroBatStep::Support, self.support),
        ]
        .into_iter()
        .find_map(|(step, status)| match status {
            StepStatus::Running => Some(None),
            StepStatus::Ready | StepStatus::Failed => Some(Some(step)),
            _ => None,
        })
        .flatten()
    }
}

/// Parses a trailing "N / M" count out of a status line, for determinate
/// progress ("Auxiliary files: 32 / 146").
pub fn parse_progress_counts(detail: &str) -> Option<(u64, u64)> {
    let (left, right) = detail.split_once('/')?;
    let done = left
        .trim_end()
        .rsplit(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()?;
    let total = right
        .trim_start()
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()?;
    (total > 0).then_some((done, total))
}

/// The derived save-sync summary the whole UI shares (UI-PLAN §5): configured,
/// enabled, running, and successfully checked stay distinct — unknown state
/// and zero linked files are never presented as "up to date".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveSyncSummary {
    /// Signed out or the server rejected the session.
    AuthRequired,
    /// The RomM connection itself failed.
    Offline,
    /// The user has not enabled sync — no implied protection.
    Off,
    /// The source configuration is invalid or incomplete.
    NeedsSetup,
    /// Enabled but no completed check exists yet.
    Checking,
    /// A check ran but no eligible files are linked to games.
    NoLinkedFiles,
    /// Still reconciling known links.
    Reconciling,
    /// Transfers are pending or running.
    Syncing,
    /// The current operation failed or items need review.
    NeedsAttention,
    /// The worker paused syncing (for example on network errors).
    Paused,
    /// Linked files checked with no work pending. Unlinked files keep a
    /// separate count — they do not downgrade the summary.
    UpToDate,
}

impl SaveSyncSummary {
    pub fn label(self) -> &'static str {
        match self {
            Self::AuthRequired => "Sign-in required",
            Self::Offline => "RomM unreachable",
            Self::Off => "Automatic save sync is off",
            Self::NeedsSetup => "Save sync needs setup",
            Self::Checking => "Checking for changes…",
            Self::NoLinkedFiles => "Nothing to sync yet",
            Self::Reconciling => "Checking for changes…",
            Self::Syncing => "Syncing saves…",
            Self::NeedsAttention => "Save sync needs attention",
            Self::Paused => "Save sync paused",
            Self::UpToDate => "Tracked saves are up to date",
        }
    }
}

#[derive(Clone, Debug)]
pub struct DownloadView {
    pub file_id: u64,
    pub rom_id: u64,
    pub file_name: String,
    pub received: u64,
    pub total: Option<u64>,
    /// None = in flight; Some(Ok) done; Some(Err) terminal failure state.
    pub finished: Option<std::result::Result<(), String>>,
}

#[derive(Clone, Debug)]
pub struct SaveSyncTransferView {
    pub rom_id: i64,
    pub revision: String,
    pub updated_at: u64,
    pub phase: String,
    pub detail: Option<String>,
}

/// How many download rows are kept on screen; oldest finished rows are
/// dropped first (the log carries the durable record anyway).
const MAX_DOWNLOAD_ROWS: usize = 128;

#[derive(PartialEq, Eq)]
struct SaveScanScope {
    server: Option<String>,
    account: Option<i64>,
    root: Option<String>,
}

/// Everything the window draws — derived ONLY from real worker events.
]]></tool_result>
<tool_result tool_call_id="toolu_01CvmSfZDWadcoqrF8J1syVF" source="tool_result" timestamp="2026-10-27T09:04:32.657Z"><![CDATA[
//! Event-derived UI state and the summaries rendered by the window.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    Idle,
    Connecting,
    Connected,
    Failed,
    SignInRequired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountState {
    NotMounted,
    Mounting,
    Mounted,
    Failed,
}

/// The derived games-folder summary the whole UI shares (UI-PLAN §3): a folder
/// is "ready" only after a real MountStarted — never inferred from the
/// persisted enabled setting or the existence of a directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderSummary {
    /// No completed folder setup for this server yet.
    SetupNeeded,
    /// A folder is configured but the provider is stopped.
    Stopped,
    /// A start operation is running.
    Starting,
    /// Provider active and folder usable.
    Ready,
    /// The last start failed (reason in `mount_error`).
    Failed,
}

impl FolderSummary {
    pub fn label(self) -> &'static str {
        match self {
            Self::SetupNeeded => "No games folder chosen yet",
            Self::Stopped => "Games folder stopped",
            Self::Starting => "Preparing games…",
            Self::Ready => "Available",
            Self::Failed => "Couldn't start the games folder",
        }
    }
}

/// Artwork checking is only the first stage; RetroBat must also receive the update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrontendMetadataSummary {
    NotStarted,
    CheckingArtwork,
    Publishing,
    Waiting,
    Failed,
    UpToDate,
}

/// Where one step of the RetroBat flow stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepStatus {
    Done,
    /// Actionable now.
    Ready,
    Running,
    /// The last attempt failed; the action is a retry.
    Failed,
    /// Unavailable until something else happens (the reason).
    Blocked(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetroBatStep {
    Install,
    Link,
    Support,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetroBatSteps {
    pub install: StepStatus,
    pub link: StepStatus,
    pub support: StepStatus,
}

impl RetroBatSteps {
    /// The one step whose action is highlighted: the first that is neither
    /// done nor blocked. Nothing is highlighted while an operation runs.
    pub fn primary(&self) -> Option<RetroBatStep> {
        [
            (RetroBatStep::Install, self.install),
            (RetroBatStep::Link, self.link),
            (RetroBatStep::Support, self.support),
        ]
        .into_iter()
        .find_map(|(step, status)| match status {
            StepStatus::Running => Some(None),
            StepStatus::Ready | StepStatus::Failed => Some(Some(step)),
            _ => None,
        })
        .flatten()
    }
}

/// Parses a trailing "N / M" count out of a status line, for determinate
/// progress ("Auxiliary files: 32 / 146").
pub fn parse_progress_counts(detail: &str) -> Option<(u64, u64)> {
    let (left, right) = detail.split_once('/')?;
    let done = left
        .trim_end()
        .rsplit(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()?;
    let total = right
        .trim_start()
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()?;
    (total > 0).then_some((done, total))
}

/// The derived save-sync summary the whole UI shares (UI-PLAN §5): configured,
/// enabled, running, and successfully checked stay distinct — unknown state
/// and zero linked files are never presented as "up to date".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveSyncSummary {
    /// Signed out or the server rejected the session.
    AuthRequired,
    /// The RomM connection itself failed.
    Offline,
    /// The user has not enabled sync — no implied protection.
    Off,
    /// The source configuration is invalid or incomplete.
    NeedsSetup,
    /// Enabled but no completed check exists yet.
    Checking,
    /// A check ran but no eligible files are linked to games.
    NoLinkedFiles,
    /// Still reconciling known links.
    Reconciling,
    /// Transfers are pending or running.
    Syncing,
    /// The current operation failed or items need review.
    NeedsAttention,
    /// The worker paused syncing (for example on network errors).
    Paused,
    /// Linked files checked with no work pending. Unlinked files keep a
    /// separate count — they do not downgrade the summary.
    UpToDate,
}

impl SaveSyncSummary {
    pub fn label(self) -> &'static str {
        match self {
            Self::AuthRequired => "Sign-in required",
            Self::Offline => "RomM unreachable",
            Self::Off => "Automatic save sync is off",
            Self::NeedsSetup => "Save sync needs setup",
            Self::Checking => "Checking for changes…",
            Self::NoLinkedFiles => "Nothing to sync yet",
            Self::Reconciling => "Checking for changes…",
            Self::Syncing => "Syncing saves…",
            Self::NeedsAttention => "Save sync needs attention",
            Self::Paused => "Save sync paused",
            Self::UpToDate => "Tracked saves are up to date",
        }
    }
}

#[derive(Clone, Debug)]
pub struct DownloadView {
    pub file_id: u64,
    pub rom_id: u64,
    pub file_name: String,
    pub received: u64,
    pub total: Option<u64>,
    /// None = in flight; Some(Ok) done; Some(Err) terminal failure state.
    pub finished: Option<std::result::Result<(), String>>,
}

#[derive(Clone, Debug)]
pub struct SaveSyncTransferView {
    pub rom_id: i64,
    pub revision: String,
    pub updated_at: u64,
    pub phase: String,
    pub detail: Option<String>,
}

/// How many download rows are kept on screen; oldest finished rows are
/// dropped first (the log carries the durable record anyway).
const MAX_DOWNLOAD_ROWS: usize = 128;

#[derive(PartialEq, Eq)]
struct SaveScanScope {
    server: Option<String>,
    account: Option<i64>,
    root: Option<String>,
}

/// Everything the window draws — derived ONLY from real worker events.
]]></tool_result>
<tool_call id="toolu_011oqq9nS9wa2JocteRHeJ7Q" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"grep -n 'pub fn \\|fn [a-z_]*(' crates/rommfs-app/src/controller/state.rs | head -80","description":"List functions in state module"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ grep -n 'pub fn \|fn [a-z_]*(' crates/rommfs-app/src/controller/state.rs | head -80
]]></tool_body>
<tool_result><![CDATA[
39:    pub fn label(self) -> &'static str {
91:    pub fn primary(&self) -> Option<RetroBatStep> {
109:pub fn parse_progress_counts(detail: &str) -> Option<(u64, u64)> {
157:    pub fn label(self) -> &'static str {
321:    pub fn new(log_cap: usize) -> Self {
418:    pub fn apply_event(&mut self, event: &AppEvent) {
1099:    pub fn retrobat_steps(&self) -> RetroBatSteps {
1150:    pub fn folder_summary(&self) -> FolderSummary {
1171:    pub fn save_files_ignored(&self) -> usize {
1178:    pub fn frontend_metadata_summary(&self) -> FrontendMetadataSummary {
1202:    pub fn save_sync_attention(&self) -> Option<String> {
1218:    pub fn save_sync_off_hint(&self) -> Option<&'static str> {
1226:    pub fn save_sync_summary(&self) -> SaveSyncSummary {
1286:    pub fn save_sync_review_versions(
1306:    pub fn request_save_sync_enabled(&mut self, enabled: bool) {
1311:    pub fn save_sync_review_count(&self) -> usize {
1322:    pub fn save_sync_summary_text(&self) -> String {
1395:    pub fn request_mount(&mut self, path: String) {
1401:    fn download_row(&mut self, rom_id: u64, file_id: u64, file_name: &str) -> &mut DownloadView {
1424:    pub fn clear_completed_downloads(&mut self) {
1429:    fn prune_downloads(&mut self) {
]]></tool_result>
<tool_result tool_call_id="toolu_011oqq9nS9wa2JocteRHeJ7Q" source="tool_result" timestamp="2026-10-27T09:04:32.733Z"><![CDATA[
39:    pub fn label(self) -> &'static str {
91:    pub fn primary(&self) -> Option<RetroBatStep> {
109:pub fn parse_progress_counts(detail: &str) -> Option<(u64, u64)> {
157:    pub fn label(self) -> &'static str {
321:    pub fn new(log_cap: usize) -> Self {
418:    pub fn apply_event(&mut self, event: &AppEvent) {
1099:    pub fn retrobat_steps(&self) -> RetroBatSteps {
1150:    pub fn folder_summary(&self) -> FolderSummary {
1171:    pub fn save_files_ignored(&self) -> usize {
1178:    pub fn frontend_metadata_summary(&self) -> FrontendMetadataSummary {
1202:    pub fn save_sync_attention(&self) -> Option<String> {
1218:    pub fn save_sync_off_hint(&self) -> Option<&'static str> {
1226:    pub fn save_sync_summary(&self) -> SaveSyncSummary {
1286:    pub fn save_sync_review_versions(
1306:    pub fn request_save_sync_enabled(&mut self, enabled: bool) {
1311:    pub fn save_sync_review_count(&self) -> usize {
1322:    pub fn save_sync_summary_text(&self) -> String {
1395:    pub fn request_mount(&mut self, path: String) {
1401:    fn download_row(&mut self, rom_id: u64, file_id: u64, file_name: &str) -> &mut DownloadView {
1424:    pub fn clear_completed_downloads(&mut self) {
1429:    fn prune_downloads(&mut self) {
]]></tool_result>
</message>
<message role="assistant" ordinal="26" timestamp="2026-10-27T09:04:37.435Z" has_thinking="true" has_tool_use="true">
<thinking><![CDATA[
The apply_event function spans roughly 680 lines, acting as a massive event reducer that mutates about a hundred flat fields—this is likely the root cause of conflicting state. I also want to dig into the artwork bug where it shows "complete" and "in progress" simultaneously, so I should check frontend_metadata_summary and how the overview renders artwork status.
]]></thinking>
<tool_call id="toolu_015ifyiHsLcQqFDCFCuiwWfH" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"sed -n 1099,1400p crates/rommfs-app/src/controller/state.rs","description":"Read derived summary selectors"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ sed -n 1099,1400p crates/rommfs-app/src/controller/state.rs
]]></tool_body>
<tool_result><![CDATA[
    pub fn retrobat_steps(&self) -> RetroBatSteps {
        let folder_running = matches!(self.mount, MountState::Mounted | MountState::Mounting);
        let connected = self.conn == ConnectionState::Connected;
        let install = if self.installer_running {
            StepStatus::Running
        } else if self.retrobat_root.is_some() {
            StepStatus::Done
        } else {
            StepStatus::Ready
        };
        let link = if self.retrobat_root.is_none() {
            StepStatus::Blocked("Install RetroBat first.")
        } else if self.retrobat_setup_running {
            StepStatus::Running
        } else if self.retrobat_setup_ok {
            StepStatus::Done
        } else if !connected {
            StepStatus::Blocked("Connect to RomM first.")
        } else if folder_running {
            StepStatus::Blocked("Stop the games folder first.")
        } else if self.retrobat_setup_error.is_some() {
            StepStatus::Failed
        } else {
            StepStatus::Ready
        };
        let support = if self.auxiliary_running {
            StepStatus::Running
        } else if self.retrobat_root.is_none() {
            StepStatus::Blocked("Install RetroBat first.")
        } else if !connected {
            StepStatus::Blocked("Connect to RomM first.")
        } else if self.catalogue.is_none() {
            StepStatus::Blocked("Wait for your game library to load.")
        } else if folder_running {
            StepStatus::Blocked("Stop the games folder first.")
        } else if self.auxiliary_error.is_some() {
            StepStatus::Failed
        } else if self.auxiliary_summary.is_some() {
            StepStatus::Done
        } else {
            StepStatus::Ready
        };
        RetroBatSteps {
            install,
            link,
            support,
        }
    }

    /// Whether the games folder is usable right now, derived only from real
    /// mount events — never from connection, configuration, or sync state.
    pub fn folder_summary(&self) -> FolderSummary {
        match self.mount {
            MountState::Mounted => FolderSummary::Ready,
            MountState::Mounting => FolderSummary::Starting,
            MountState::Failed => FolderSummary::Failed,
            MountState::NotMounted => {
                // A loaded catalogue means a folder was already set up, even
                // when its path hasn't been reported to this session.
                if self.mount_path.is_some()
                    || self.configured_mount_root.is_some()
                    || self.catalogue.is_some()
                {
                    FolderSummary::Stopped
                } else {
                    FolderSummary::SetupNeeded
                }
            }
        }
    }

    /// Structural exclusions are skipped; eligible unlinked saves stay actionable.
    pub fn save_files_ignored(&self) -> usize {
        self.save_file_unmatched
            .iter()
            .filter(|file| !file.mappable)
            .count()
    }

    pub fn frontend_metadata_summary(&self) -> FrontendMetadataSummary {
        if self.frontend_metadata_pending {
            if self.frontend_metadata_error.is_some() {
                FrontendMetadataSummary::Waiting
            } else if self
                .frontend_metadata_progress
                .is_some_and(|(completed, total)| completed >= total)
            {
                FrontendMetadataSummary::Publishing
            } else {
                FrontendMetadataSummary::CheckingArtwork
            }
        } else if self.frontend_metadata_error.is_some() {
            FrontendMetadataSummary::Failed
        } else if self.frontend_metadata_updated_at.is_some() {
            FrontendMetadataSummary::UpToDate
        } else {
            FrontendMetadataSummary::NotStarted
        }
    }

    /// The save-sync problem worth interrupting the user for. Save sync is
    /// silent until the user enables it: a missing folder or unverified
    /// account is a setup hint then, not something needing attention.
    pub fn save_sync_attention(&self) -> Option<String> {
        self.save_sync_failure
            .clone()
            .or_else(|| {
                self.save_sync_enabled
                    .then(|| self.save_sync_problem.clone())
                    .flatten()
            })
            .or_else(|| {
                self.save_sync_queue
                    .as_ref()
                    .and_then(|queue| queue.failure.clone())
            })
    }

    /// One-line hint shown while save sync is off and cannot be enabled yet.
    pub fn save_sync_off_hint(&self) -> Option<&'static str> {
        (!self.save_sync_enabled && !self.save_sync_available && self.save_sync_problem.is_some())
            .then_some("Save sync is off. Choose a save folder or RetroBat installation in Settings to use it.")
    }

    /// The one derived save-sync status (UI-PLAN §5 table). Zero eligible
    /// files is not "up to date"; an unlinked-file count does not downgrade
    /// an otherwise healthy linked set.
    pub fn save_sync_summary(&self) -> SaveSyncSummary {
        if self.conn == ConnectionState::SignInRequired || self.save_sync_authentication_required {
            return SaveSyncSummary::AuthRequired;
        }
        if self.conn == ConnectionState::Failed {
            return SaveSyncSummary::Offline;
        }
        if !self.save_sync_enabled {
            return SaveSyncSummary::Off;
        }
        if self.save_sync_checking {
            return if self.save_sync_attention_games > 0 || self.save_sync_attention().is_some() {
                SaveSyncSummary::NeedsAttention
            } else {
                SaveSyncSummary::Checking
            };
        }
        if !self.memory_card_conflicts.is_empty() {
            return SaveSyncSummary::NeedsAttention;
        }
        if !self.save_sync_available {
            return SaveSyncSummary::NeedsSetup;
        }
        let Some(queue) = self.save_sync_queue.as_ref() else {
            return SaveSyncSummary::Checking;
        };
        if queue.actor_failed || queue.failure.is_some() || self.save_sync_failure.is_some() {
            return SaveSyncSummary::NeedsAttention;
        }
        if queue.attention_games > 0 {
            return SaveSyncSummary::NeedsAttention;
        }
        if queue.network_paused {
            return SaveSyncSummary::Paused;
        }
        if queue.mapped_games == 0 {
            return SaveSyncSummary::NoLinkedFiles;
        }
        if queue.pending_outbound > 0 {
            return SaveSyncSummary::Syncing;
        }
        if queue.attention_games > 0 || queue.pending_incoming > 0 {
            return SaveSyncSummary::NeedsAttention;
        }
        if queue.reconciled_games < queue.mapped_games {
            return SaveSyncSummary::Reconciling;
        }
        if queue.games.len() != queue.mapped_games
            || queue
                .games
                .iter()
                .any(|game| game.work != rommfs_core::events::SaveSyncWork::UpToDate)
        {
            return SaveSyncSummary::Reconciling;
        }
        SaveSyncSummary::UpToDate
    }

    /// Review versions belong to the existing save identity, never to a
    /// game title or to a count of queued downloads.
    pub fn save_sync_review_versions(
        &self,
        rom_id: i64,
        file_id: i64,
    ) -> impl Iterator<Item = &rommfs_core::events::SaveSyncIncomingStatus> {
        let game = self
            .save_sync_games
            .iter()
            .find(|game| (game.rom_id, game.file_id) == (rom_id, file_id));
        self.save_sync_queue
            .iter()
            .flat_map(|queue| queue.incoming.iter())
            .filter(move |incoming| {
                incoming.rom_id == rom_id
                    && game.is_some_and(|game| game.incoming_ids.contains(&incoming.incoming_id))
            })
    }

    /// Keep a deliberate consent change visible until a newer worker session
    /// acknowledges it. Old discovery results cannot undo the user's choice.
    pub fn request_save_sync_enabled(&mut self, enabled: bool) {
        self.save_sync_enabled = enabled;
        self.requested_save_sync_consent = Some((self.save_sync_session_id, enabled));
    }

    pub fn save_sync_review_count(&self) -> usize {
        self.save_sync_games
            .iter()
            .filter(|game| {
                game.work == rommfs_core::events::SaveSyncWork::NeedsReview
                    || !game.incoming_ids.is_empty()
            })
            .count()
            + self.memory_card_conflicts.len()
    }

    pub fn save_sync_summary_text(&self) -> String {
        use rommfs_core::events::SaveSyncWork;
        let review = self.save_sync_review_count();
        let active = self
            .save_sync_games
            .iter()
            .filter(|game| game.work == SaveSyncWork::Uploading)
            .count();
        let waiting = self
            .save_sync_games
            .iter()
            .filter(|game| game.work == SaveSyncWork::WaitingToUpload)
            .count();
        let mut parts = Vec::new();
        if !self.save_sync_enabled {
            parts.push("Automatic save sync is off".to_owned());
        }
        let summary = self.save_sync_summary();
        if matches!(
            summary,
            SaveSyncSummary::AuthRequired
                | SaveSyncSummary::Offline
                | SaveSyncSummary::NeedsSetup
                | SaveSyncSummary::Paused
        ) || (summary == SaveSyncSummary::NeedsAttention && review == 0)
        {
            parts.push(summary.label().to_owned());
        }
        if review > 0 {
            parts.push(format!(
                "{review} {} review",
                if review == 1 {
                    "save needs"
                } else {
                    "saves need"
                }
            ));
        }
        if self.save_sync_checking && review == 0 {
            parts.push(SaveSyncSummary::Checking.label().to_owned());
        }
        if active > 0 && !self.save_sync_checking {
            parts.push(format!(
                "Uploading {active} {}",
                if active == 1 { "save" } else { "saves" }
            ));
        }
        if waiting > 0 && !self.save_sync_checking {
            parts.push(format!(
                "{waiting} {} waiting to upload",
                if waiting == 1 { "save" } else { "saves" }
            ));
        }
        if parts.is_empty() {
            if self.save_sync_summary() == SaveSyncSummary::UpToDate {
                parts.push(format!(
                    "{} tracked {} up to date",
                    self.save_sync_games.len(),
                    if self.save_sync_games.len() == 1 {
                        "save"
                    } else {
                        "saves"
                    }
                ));
            } else {
                parts.push(self.save_sync_summary().label().to_owned());
            }
        }
        parts.join(" · ")
    }

    /// Mark a mount request pending immediately so a second UI click cannot
    /// enqueue another start before the worker's MountStarting event arrives.
    pub fn request_mount(&mut self, path: String) {
        self.mount = MountState::Mounting;
        self.mount_path = Some(path);
        self.mount_error = None;
    }
]]></tool_result>
<tool_result tool_call_id="toolu_015ifyiHsLcQqFDCFCuiwWfH" source="tool_result" timestamp="2026-10-27T09:04:38.308Z"><![CDATA[
    pub fn retrobat_steps(&self) -> RetroBatSteps {
        let folder_running = matches!(self.mount, MountState::Mounted | MountState::Mounting);
        let connected = self.conn == ConnectionState::Connected;
        let install = if self.installer_running {
            StepStatus::Running
        } else if self.retrobat_root.is_some() {
            StepStatus::Done
        } else {
            StepStatus::Ready
        };
        let link = if self.retrobat_root.is_none() {
            StepStatus::Blocked("Install RetroBat first.")
        } else if self.retrobat_setup_running {
            StepStatus::Running
        } else if self.retrobat_setup_ok {
            StepStatus::Done
        } else if !connected {
            StepStatus::Blocked("Connect to RomM first.")
        } else if folder_running {
            StepStatus::Blocked("Stop the games folder first.")
        } else if self.retrobat_setup_error.is_some() {
            StepStatus::Failed
        } else {
            StepStatus::Ready
        };
        let support = if self.auxiliary_running {
            StepStatus::Running
        } else if self.retrobat_root.is_none() {
            StepStatus::Blocked("Install RetroBat first.")
        } else if !connected {
            StepStatus::Blocked("Connect to RomM first.")
        } else if self.catalogue.is_none() {
            StepStatus::Blocked("Wait for your game library to load.")
        } else if folder_running {
            StepStatus::Blocked("Stop the games folder first.")
        } else if self.auxiliary_error.is_some() {
            StepStatus::Failed
        } else if self.auxiliary_summary.is_some() {
            StepStatus::Done
        } else {
            StepStatus::Ready
        };
        RetroBatSteps {
            install,
            link,
            support,
        }
    }

    /// Whether the games folder is usable right now, derived only from real
    /// mount events — never from connection, configuration, or sync state.
    pub fn folder_summary(&self) -> FolderSummary {
        match self.mount {
            MountState::Mounted => FolderSummary::Ready,
            MountState::Mounting => FolderSummary::Starting,
            MountState::Failed => FolderSummary::Failed,
            MountState::NotMounted => {
                // A loaded catalogue means a folder was already set up, even
                // when its path hasn't been reported to this session.
                if self.mount_path.is_some()
                    || self.configured_mount_root.is_some()
                    || self.catalogue.is_some()
                {
                    FolderSummary::Stopped
                } else {
                    FolderSummary::SetupNeeded
                }
            }
        }
    }

    /// Structural exclusions are skipped; eligible unlinked saves stay actionable.
    pub fn save_files_ignored(&self) -> usize {
        self.save_file_unmatched
            .iter()
            .filter(|file| !file.mappable)
            .count()
    }

    pub fn frontend_metadata_summary(&self) -> FrontendMetadataSummary {
        if self.frontend_metadata_pending {
            if self.frontend_metadata_error.is_some() {
                FrontendMetadataSummary::Waiting
            } else if self
                .frontend_metadata_progress
                .is_some_and(|(completed, total)| completed >= total)
            {
                FrontendMetadataSummary::Publishing
            } else {
                FrontendMetadataSummary::CheckingArtwork
            }
        } else if self.frontend_metadata_error.is_some() {
            FrontendMetadataSummary::Failed
        } else if self.frontend_metadata_updated_at.is_some() {
            FrontendMetadataSummary::UpToDate
        } else {
            FrontendMetadataSummary::NotStarted
        }
    }

    /// The save-sync problem worth interrupting the user for. Save sync is
    /// silent until the user enables it: a missing folder or unverified
    /// account is a setup hint then, not something needing attention.
    pub fn save_sync_attention(&self) -> Option<String> {
        self.save_sync_failure
            .clone()
            .or_else(|| {
                self.save_sync_enabled
                    .then(|| self.save_sync_problem.clone())
                    .flatten()
            })
            .or_else(|| {
                self.save_sync_queue
                    .as_ref()
                    .and_then(|queue| queue.failure.clone())
            })
    }

    /// One-line hint shown while save sync is off and cannot be enabled yet.
    pub fn save_sync_off_hint(&self) -> Option<&'static str> {
        (!self.save_sync_enabled && !self.save_sync_available && self.save_sync_problem.is_some())
            .then_some("Save sync is off. Choose a save folder or RetroBat installation in Settings to use it.")
    }

    /// The one derived save-sync status (UI-PLAN §5 table). Zero eligible
    /// files is not "up to date"; an unlinked-file count does not downgrade
    /// an otherwise healthy linked set.
    pub fn save_sync_summary(&self) -> SaveSyncSummary {
        if self.conn == ConnectionState::SignInRequired || self.save_sync_authentication_required {
            return SaveSyncSummary::AuthRequired;
        }
        if self.conn == ConnectionState::Failed {
            return SaveSyncSummary::Offline;
        }
        if !self.save_sync_enabled {
            return SaveSyncSummary::Off;
        }
        if self.save_sync_checking {
            return if self.save_sync_attention_games > 0 || self.save_sync_attention().is_some() {
                SaveSyncSummary::NeedsAttention
            } else {
                SaveSyncSummary::Checking
            };
        }
        if !self.memory_card_conflicts.is_empty() {
            return SaveSyncSummary::NeedsAttention;
        }
        if !self.save_sync_available {
            return SaveSyncSummary::NeedsSetup;
        }
        let Some(queue) = self.save_sync_queue.as_ref() else {
            return SaveSyncSummary::Checking;
        };
        if queue.actor_failed || queue.failure.is_some() || self.save_sync_failure.is_some() {
            return SaveSyncSummary::NeedsAttention;
        }
        if queue.attention_games > 0 {
            return SaveSyncSummary::NeedsAttention;
        }
        if queue.network_paused {
            return SaveSyncSummary::Paused;
        }
        if queue.mapped_games == 0 {
            return SaveSyncSummary::NoLinkedFiles;
        }
        if queue.pending_outbound > 0 {
            return SaveSyncSummary::Syncing;
        }
        if queue.attention_games > 0 || queue.pending_incoming > 0 {
            return SaveSyncSummary::NeedsAttention;
        }
        if queue.reconciled_games < queue.mapped_games {
            return SaveSyncSummary::Reconciling;
        }
        if queue.games.len() != queue.mapped_games
            || queue
                .games
                .iter()
                .any(|game| game.work != rommfs_core::events::SaveSyncWork::UpToDate)
        {
            return SaveSyncSummary::Reconciling;
        }
        SaveSyncSummary::UpToDate
    }

    /// Review versions belong to the existing save identity, never to a
    /// game title or to a count of queued downloads.
    pub fn save_sync_review_versions(
        &self,
        rom_id: i64,
        file_id: i64,
    ) -> impl Iterator<Item = &rommfs_core::events::SaveSyncIncomingStatus> {
        let game = self
            .save_sync_games
            .iter()
            .find(|game| (game.rom_id, game.file_id) == (rom_id, file_id));
        self.save_sync_queue
            .iter()
            .flat_map(|queue| queue.incoming.iter())
            .filter(move |incoming| {
                incoming.rom_id == rom_id
                    && game.is_some_and(|game| game.incoming_ids.contains(&incoming.incoming_id))
            })
    }

    /// Keep a deliberate consent change visible until a newer worker session
    /// acknowledges it. Old discovery results cannot undo the user's choice.
    pub fn request_save_sync_enabled(&mut self, enabled: bool) {
        self.save_sync_enabled = enabled;
        self.requested_save_sync_consent = Some((self.save_sync_session_id, enabled));
    }

    pub fn save_sync_review_count(&self) -> usize {
        self.save_sync_games
            .iter()
            .filter(|game| {
                game.work == rommfs_core::events::SaveSyncWork::NeedsReview
                    || !game.incoming_ids.is_empty()
            })
            .count()
            + self.memory_card_conflicts.len()
    }

    pub fn save_sync_summary_text(&self) -> String {
        use rommfs_core::events::SaveSyncWork;
        let review = self.save_sync_review_count();
        let active = self
            .save_sync_games
            .iter()
            .filter(|game| game.work == SaveSyncWork::Uploading)
            .count();
        let waiting = self
            .save_sync_games
            .iter()
            .filter(|game| game.work == SaveSyncWork::WaitingToUpload)
            .count();
        let mut parts = Vec::new();
        if !self.save_sync_enabled {
            parts.push("Automatic save sync is off".to_owned());
        }
        let summary = self.save_sync_summary();
        if matches!(
            summary,
            SaveSyncSummary::AuthRequired
                | SaveSyncSummary::Offline
                | SaveSyncSummary::NeedsSetup
                | SaveSyncSummary::Paused
        ) || (summary == SaveSyncSummary::NeedsAttention && review == 0)
        {
            parts.push(summary.label().to_owned());
        }
        if review > 0 {
            parts.push(format!(
                "{review} {} review",
                if review == 1 {
                    "save needs"
                } else {
                    "saves need"
                }
            ));
        }
        if self.save_sync_checking && review == 0 {
            parts.push(SaveSyncSummary::Checking.label().to_owned());
        }
        if active > 0 && !self.save_sync_checking {
            parts.push(format!(
                "Uploading {active} {}",
                if active == 1 { "save" } else { "saves" }
            ));
        }
        if waiting > 0 && !self.save_sync_checking {
            parts.push(format!(
                "{waiting} {} waiting to upload",
                if waiting == 1 { "save" } else { "saves" }
            ));
        }
        if parts.is_empty() {
            if self.save_sync_summary() == SaveSyncSummary::UpToDate {
                parts.push(format!(
                    "{} tracked {} up to date",
                    self.save_sync_games.len(),
                    if self.save_sync_games.len() == 1 {
                        "save"
                    } else {
                        "saves"
                    }
                ));
            } else {
                parts.push(self.save_sync_summary().label().to_owned());
            }
        }
        parts.join(" · ")
    }

    /// Mark a mount request pending immediately so a second UI click cannot
    /// enqueue another start before the worker's MountStarting event arrives.
    pub fn request_mount(&mut self, path: String) {
        self.mount = MountState::Mounting;
        self.mount_path = Some(path);
        self.mount_error = None;
    }
]]></tool_result>
<tool_call id="toolu_01S7wtFUGDxphw2woZZd1Jp5" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"sed -n 1,243p crates/rommfs-app/src/window/pages/overview.rs","description":"Read Overview page rendering"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ sed -n 1,243p crates/rommfs-app/src/window/pages/overview.rs
]]></tool_body>
<tool_result><![CDATA[
use super::super::*;

pub fn render(
    view: &mut RommfsWindow,
    window: &mut Window,
    cx: &mut Context<RommfsWindow>,
) -> impl IntoElement {
    view.render_overview(window, cx)
}

impl RommfsWindow {
    fn render_overview(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let needs_sign_in = self.state.conn == ConnectionState::SignInRequired;
        let show_connection = needs_sign_in || self.state.conn_error.is_some();
        v_flex()
            .gap_4()
            .child(page_header(
                "Overview",
                "Your library and anything that needs attention.",
                None,
                cx,
            ))
            .when(show_connection, |this| {
                let problem = self.state.conn_error.as_deref().map_or_else(
                    || {
                        UserProblem::error(
                            "Sign in to RomM",
                            Some("Sign in again to reconnect your library.".into()),
                            None,
                        )
                    },
                    |error| UserProblem::from_text("Couldn't connect to RomM", error),
                );
                this.child(problem_block(
                    "overview-connection-problem",
                    &problem,
                    Some(self.settings_nav_button(
                        "overview-connection",
                        "Connection settings",
                        0,
                        needs_sign_in,
                        cx,
                    )),
                    window,
                    cx,
                ))
            })
            .child(self.render_mount(window, cx))
            .child(self.render_overview_retrobat(window, cx))
            .child(self.render_overview_saves(cx))
            .when(
                self.state.integration == IntegrationMode::RetroBat,
                |this| this.child(self.render_overview_metadata(window, cx)),
            )
    }

    fn render_overview_saves(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let (text, kind) = self.save_sync_pill();
        let review_count = self.state.save_sync_review_count();
        let description = if review_count > 0 {
            format!(
                "{} {} your review. Open Saves to see what happened and choose the next step.",
                plural(review_count, "save"),
                if review_count == 1 { "needs" } else { "need" }
            )
        } else if !self.state.save_sync_enabled {
            "Automatic sync is off. You can enable it in Saves to copy supported saves to RomM."
                .into()
        } else {
            self.state.save_sync_summary_text()
        };
        let action = Button::new("overview-saves")
            .label(if review_count > 0 {
                "Review saves"
            } else {
                "View saves"
            })
            .outline()
            .small()
            .on_click(cx.listener(|view, _, window, cx| {
                view.active_view = 1;
                view.save_review_filter = usize::from(view.state.save_sync_review_count() > 0);
                if view.save_review_filter == 1 {
                    view.save_review_search
                        .update(cx, |input, cx| input.set_value("", window, cx));
                    view.save_review_page = 0;
                }
                cx.notify();
            }));
        card(cx).p_4().child(
            v_flex()
                .gap_2()
                .child(status_row("Saves", kind, text, Some(action), cx))
                .child(muted(description, cx))
                .when_some(self.state.save_sync_attention(), |this, error| {
                    this.child(muted(failure_explanation(&error), cx))
                }),
        )
    }

    fn render_overview_retrobat(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let migration = self.state.retrobat_setup_migration
            && !self.state.retrobat_setup_ok
            && !self.state.retrobat_setup_running;
        let (text, kind, description) = if self.state.integration != IntegrationMode::RetroBat {
            (
                "Optional",
                StatusKind::Muted,
                "Use RetroBat to browse and play your RomM games.",
            )
        } else if self.state.retrobat_setup_running {
            (
                "Setting up…",
                StatusKind::Muted,
                "Connecting your RomM games to RetroBat.",
            )
        } else if self.state.retrobat_setup_error.is_some() {
            (
                "Needs attention",
                StatusKind::Danger,
                "RetroBat setup couldn't finish. Open its settings to try again.",
            )
        } else if self.state.retrobat_setup_ok {
            (
                "Setup complete",
                StatusKind::Success,
                "Open RetroBat to browse your library. Keep RomMFS running while you play.",
            )
        } else if migration {
            (
                "Update needed",
                StatusKind::Warning,
                "Update the RetroBat connection before making games available.",
            )
        } else {
            (
                "Setup needed",
                StatusKind::Muted,
                "Choose your RetroBat installation and connect it to your library.",
            )
        };
        let action = self
            .settings_nav_button(
                "overview-retrobat",
                if self.state.retrobat_setup_ok {
                    "RetroBat settings"
                } else {
                    "Set up RetroBat"
                },
                2,
                false,
                cx,
            )
            .small();
        card(cx).p_4().child(
            v_flex().gap_2()
                .child(status_row("RetroBat", kind, text, Some(action), cx))
                .child(muted(description, cx))
                .when(migration, |this| {
                    this.child(h_flex().child(
                        Button::new("overview-retrobat-update")
                            .label("Update RetroBat connection")
                            .outline()
                            .on_click(cx.listener(|view, _, _, cx| view.run_retrobat_setup(cx))),
                    ))
                })
                .when_some(self.state.retrobat_setup_error.clone(), |this, error| {
                    this.child(muted(failure_explanation(&error), cx))
                        .child(details_disclosure("overview-retrobat-error", error, window, cx))
                })
                .when_some(self.state.play_data_problem.clone(), |this, problem| {
                    this.child(muted(
                        if problem.contains("403") {
                            "RomM denied access to favourites and play history. Check your account permissions in RomM."
                        } else {
                            "Favourites and play history couldn't update. Check your RomM connection."
                        },
                        cx,
                    ))
                    .child(details_disclosure("play-data-problem", problem, window, cx))
                    .child(h_flex().child(self.settings_nav_button("play-data-connection", "Connection settings", 0, false, cx)))
                }),
        )
    }

    fn render_overview_metadata(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let summary = self.state.frontend_metadata_summary();
        let (kind, text) = match summary {
            FrontendMetadataSummary::NotStarted => (StatusKind::Muted, "Not updated yet"),
            FrontendMetadataSummary::CheckingArtwork | FrontendMetadataSummary::Publishing => {
                (StatusKind::Muted, "Updating…")
            }
            FrontendMetadataSummary::Waiting => (StatusKind::Warning, "Waiting for access"),
            FrontendMetadataSummary::Failed => (StatusKind::Danger, "Update failed"),
            FrontendMetadataSummary::UpToDate => (StatusKind::Success, "Up to date"),
        };
        let retry = (summary == FrontendMetadataSummary::Failed).then(|| {
            Button::new("overview-metadata-retry")
                .label("Retry update")
                .accessibility_label("Retry artwork and game information update")
                .outline()
                .small()
                .on_click(cx.listener(|view, _, _, cx| {
                    view.controller.send(Command::RefreshMetadata);
                    cx.notify();
                }))
        });
        card(cx).p_4().child(
            v_flex().gap_2()
                .child(status_row("Artwork and game information", kind, text, retry, cx))
                .when(self.state.frontend_metadata_pending, |this| {
                    this.child(metadata_progress("overview-metadata-progress", &self.state, cx))
                })
                .when(summary == FrontendMetadataSummary::NotStarted, |this| {
                    this.child(muted("Updates automatically when your games are available in RetroBat.", cx))
                })
                .when(summary == FrontendMetadataSummary::UpToDate, |this| {
                    this.child(muted(
                        self.state.frontend_metadata_updated_at.map_or_else(
                            || "Artwork and game information are ready in RetroBat.".into(),
                            |at| format!("Updated {}. Artwork and game information are ready in RetroBat.", relative_time(at)),
                        ),
                        cx,
                    ))
                })
                .when_some(self.state.frontend_metadata_error.clone(), |this, error| {
                    this.child(muted(failure_explanation(&error), cx))
                        .when(self.state.mount == MountState::Mounted && !self.state.mount_stopping, |this| {
                            this.child(muted("Your games remain available. Artwork and game information may be out of date.", cx))
                        })
                        .child(details_disclosure("overview-metadata-error", error, window, cx))
                }),
        )
    }
}
]]></tool_result>
<tool_result tool_call_id="toolu_01S7wtFUGDxphw2woZZd1Jp5" source="tool_result" timestamp="2026-10-27T09:04:38.228Z"><![CDATA[
use super::super::*;

pub fn render(
    view: &mut RommfsWindow,
    window: &mut Window,
    cx: &mut Context<RommfsWindow>,
) -> impl IntoElement {
    view.render_overview(window, cx)
}

impl RommfsWindow {
    fn render_overview(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let needs_sign_in = self.state.conn == ConnectionState::SignInRequired;
        let show_connection = needs_sign_in || self.state.conn_error.is_some();
        v_flex()
            .gap_4()
            .child(page_header(
                "Overview",
                "Your library and anything that needs attention.",
                None,
                cx,
            ))
            .when(show_connection, |this| {
                let problem = self.state.conn_error.as_deref().map_or_else(
                    || {
                        UserProblem::error(
                            "Sign in to RomM",
                            Some("Sign in again to reconnect your library.".into()),
                            None,
                        )
                    },
                    |error| UserProblem::from_text("Couldn't connect to RomM", error),
                );
                this.child(problem_block(
                    "overview-connection-problem",
                    &problem,
                    Some(self.settings_nav_button(
                        "overview-connection",
                        "Connection settings",
                        0,
                        needs_sign_in,
                        cx,
                    )),
                    window,
                    cx,
                ))
            })
            .child(self.render_mount(window, cx))
            .child(self.render_overview_retrobat(window, cx))
            .child(self.render_overview_saves(cx))
            .when(
                self.state.integration == IntegrationMode::RetroBat,
                |this| this.child(self.render_overview_metadata(window, cx)),
            )
    }

    fn render_overview_saves(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let (text, kind) = self.save_sync_pill();
        let review_count = self.state.save_sync_review_count();
        let description = if review_count > 0 {
            format!(
                "{} {} your review. Open Saves to see what happened and choose the next step.",
                plural(review_count, "save"),
                if review_count == 1 { "needs" } else { "need" }
            )
        } else if !self.state.save_sync_enabled {
            "Automatic sync is off. You can enable it in Saves to copy supported saves to RomM."
                .into()
        } else {
            self.state.save_sync_summary_text()
        };
        let action = Button::new("overview-saves")
            .label(if review_count > 0 {
                "Review saves"
            } else {
                "View saves"
            })
            .outline()
            .small()
            .on_click(cx.listener(|view, _, window, cx| {
                view.active_view = 1;
                view.save_review_filter = usize::from(view.state.save_sync_review_count() > 0);
                if view.save_review_filter == 1 {
                    view.save_review_search
                        .update(cx, |input, cx| input.set_value("", window, cx));
                    view.save_review_page = 0;
                }
                cx.notify();
            }));
        card(cx).p_4().child(
            v_flex()
                .gap_2()
                .child(status_row("Saves", kind, text, Some(action), cx))
                .child(muted(description, cx))
                .when_some(self.state.save_sync_attention(), |this, error| {
                    this.child(muted(failure_explanation(&error), cx))
                }),
        )
    }

    fn render_overview_retrobat(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let migration = self.state.retrobat_setup_migration
            && !self.state.retrobat_setup_ok
            && !self.state.retrobat_setup_running;
        let (text, kind, description) = if self.state.integration != IntegrationMode::RetroBat {
            (
                "Optional",
                StatusKind::Muted,
                "Use RetroBat to browse and play your RomM games.",
            )
        } else if self.state.retrobat_setup_running {
            (
                "Setting up…",
                StatusKind::Muted,
                "Connecting your RomM games to RetroBat.",
            )
        } else if self.state.retrobat_setup_error.is_some() {
            (
                "Needs attention",
                StatusKind::Danger,
                "RetroBat setup couldn't finish. Open its settings to try again.",
            )
        } else if self.state.retrobat_setup_ok {
            (
                "Setup complete",
                StatusKind::Success,
                "Open RetroBat to browse your library. Keep RomMFS running while you play.",
            )
        } else if migration {
            (
                "Update needed",
                StatusKind::Warning,
                "Update the RetroBat connection before making games available.",
            )
        } else {
            (
                "Setup needed",
                StatusKind::Muted,
                "Choose your RetroBat installation and connect it to your library.",
            )
        };
        let action = self
            .settings_nav_button(
                "overview-retrobat",
                if self.state.retrobat_setup_ok {
                    "RetroBat settings"
                } else {
                    "Set up RetroBat"
                },
                2,
                false,
                cx,
            )
            .small();
        card(cx).p_4().child(
            v_flex().gap_2()
                .child(status_row("RetroBat", kind, text, Some(action), cx))
                .child(muted(description, cx))
                .when(migration, |this| {
                    this.child(h_flex().child(
                        Button::new("overview-retrobat-update")
                            .label("Update RetroBat connection")
                            .outline()
                            .on_click(cx.listener(|view, _, _, cx| view.run_retrobat_setup(cx))),
                    ))
                })
                .when_some(self.state.retrobat_setup_error.clone(), |this, error| {
                    this.child(muted(failure_explanation(&error), cx))
                        .child(details_disclosure("overview-retrobat-error", error, window, cx))
                })
                .when_some(self.state.play_data_problem.clone(), |this, problem| {
                    this.child(muted(
                        if problem.contains("403") {
                            "RomM denied access to favourites and play history. Check your account permissions in RomM."
                        } else {
                            "Favourites and play history couldn't update. Check your RomM connection."
                        },
                        cx,
                    ))
                    .child(details_disclosure("play-data-problem", problem, window, cx))
                    .child(h_flex().child(self.settings_nav_button("play-data-connection", "Connection settings", 0, false, cx)))
                }),
        )
    }

    fn render_overview_metadata(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let summary = self.state.frontend_metadata_summary();
        let (kind, text) = match summary {
            FrontendMetadataSummary::NotStarted => (StatusKind::Muted, "Not updated yet"),
            FrontendMetadataSummary::CheckingArtwork | FrontendMetadataSummary::Publishing => {
                (StatusKind::Muted, "Updating…")
            }
            FrontendMetadataSummary::Waiting => (StatusKind::Warning, "Waiting for access"),
            FrontendMetadataSummary::Failed => (StatusKind::Danger, "Update failed"),
            FrontendMetadataSummary::UpToDate => (StatusKind::Success, "Up to date"),
        };
        let retry = (summary == FrontendMetadataSummary::Failed).then(|| {
            Button::new("overview-metadata-retry")
                .label("Retry update")
                .accessibility_label("Retry artwork and game information update")
                .outline()
                .small()
                .on_click(cx.listener(|view, _, _, cx| {
                    view.controller.send(Command::RefreshMetadata);
                    cx.notify();
                }))
        });
        card(cx).p_4().child(
            v_flex().gap_2()
                .child(status_row("Artwork and game information", kind, text, retry, cx))
                .when(self.state.frontend_metadata_pending, |this| {
                    this.child(metadata_progress("overview-metadata-progress", &self.state, cx))
                })
                .when(summary == FrontendMetadataSummary::NotStarted, |this| {
                    this.child(muted("Updates automatically when your games are available in RetroBat.", cx))
                })
                .when(summary == FrontendMetadataSummary::UpToDate, |this| {
                    this.child(muted(
                        self.state.frontend_metadata_updated_at.map_or_else(
                            || "Artwork and game information are ready in RetroBat.".into(),
                            |at| format!("Updated {}. Artwork and game information are ready in RetroBat.", relative_time(at)),
                        ),
                        cx,
                    ))
                })
                .when_some(self.state.frontend_metadata_error.clone(), |this, error| {
                    this.child(muted(failure_explanation(&error), cx))
                        .when(self.state.mount == MountState::Mounted && !self.state.mount_stopping, |this| {
                            this.child(muted("Your games remain available. Artwork and game information may be out of date.", cx))
                        })
                        .child(details_disclosure("overview-metadata-error", error, window, cx))
                }),
        )
    }
}
]]></tool_result>
</message>
<message role="assistant" ordinal="29" timestamp="2026-10-27T09:04:42.426Z" has_thinking="true" has_tool_use="true">
<thinking><![CDATA[
It looks like the code was already updated after the human testing in commit 9492a7e, which clarified app status and setup guidance. Since the screenshots are from 10-26, I should check the commit dates to see if this update came after.
]]></thinking>
<tool_call id="toolu_01ALs32qPgAc3DiJm2baBP9L" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"git log --format='%h %ad %s' --date=iso -15 && git show --stat 9492a7e | head -60","description":"Show recent commit dates and latest commit stats"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ git log --format='%h %ad %s' --date=iso -15 && git show --stat 9492a7e | head -60
]]></tool_body>
<tool_result><![CDATA[
9492a7e 2026-10-26 21:36:51 +0000 feat(ui): clarify app status and setup guidance
6f46a7c 2026-10-26 18:18:35 +1100 Merge pull request #6 from auron-labs/linux-fuse-redesign
c3b6a01 2026-10-26 05:54:29 +0000 test: update stale gamelist-published log assertion
733a9d8 2026-10-26 05:54:29 +0000 feat: apply new dark shell design across app pages
f62096f 2026-10-26 05:54:22 +0000 feat: add read-only FUSE mount backend for Linux testing
fea973b 2026-10-26 05:07:27 +0000 chore: cleanup
40e59cf 2026-10-24 07:27:00 +0000 chore(agents): add workflow skills and config
360e13a 2026-10-19 14:04:12 +1100 feat: improve windows
52513e8 2026-10-18 01:00:45 +0000 feat: improve save sync review and shutdown handling
55b5c10 2026-10-17 09:58:28 +0000 chore: human-testing
84068e6 2026-10-17 04:14:12 +0000 chore: add wizard
4b95a63 2026-10-15 06:04:05 +0000 feat: finish human-test fixes and memory-card sync
230b9a1 2026-10-15 02:07:15 +0000 feat: sync favourites and play history with RomM
a822d74 2026-10-15 01:32:16 +0000 feat(app): reorganize Settings into tabs and trim page copy
5a7be56 2026-10-15 01:01:45 +0000 feat: fix ui issues with scrolling
commit 9492a7e99987cb6bdae45e9311ff671975107bd9
Author: Aaron Florey <azza@jcaks.net>
Date:   Mon Oct 26 21:36:51 2026 +0000

    feat(ui): clarify app status and setup guidance
    
    - Improve games folder actions, progress states, and user friendly errors
    - Refresh onboarding and user guide; add human testing notes and screenshots

 .../human-testing-2026-10-26/10-26-26-192216.png   | Bin 0 -> 69785 bytes
 .../human-testing-2026-10-26/10-26-26-192225.png   | Bin 0 -> 41277 bytes
 .../human-testing-2026-10-26/10-26-26-192231.png   | Bin 0 -> 76771 bytes
 .../human-testing-2026-10-26/10-26-26-192235.png   | Bin 0 -> 82116 bytes
 .../human-testing-2026-10-26/10-26-26-192304.png   | Bin 0 -> 48755 bytes
 .../human-testing-2026-10-26/10-26-26-192308.png   | Bin 0 -> 36693 bytes
 .../human-testing-2026-10-26/10-26-26-192312.png   | Bin 0 -> 50319 bytes
 .../human-testing-2026-10-26/10-26-26-192315.png   | Bin 0 -> 50430 bytes
 .../human-testing-2026-10-26/10-26-26-192318.png   | Bin 0 -> 61861 bytes
 .../human-testing-2026-10-26/10-26-26-192322.png   | Bin 0 -> 42313 bytes
 .../human-testing-2026-10-26/10-26-26-192330.png   | Bin 0 -> 83971 bytes
 .../human-testing-2026-10-26/10-26-26-192345.png   | Bin 0 -> 83867 bytes
 .../human-testing-2026-10-26/10-26-26-192848.png   | Bin 0 -> 55527 bytes
 .../human-testing-2026-10-26/10-26-26-192854.png   | Bin 0 -> 76429 bytes
 .../human-testing-2026-10-26/10-26-26-193202.png   | Bin 0 -> 157914 bytes
 .scratch/human-testing-2026-10-26/notes.md         |  13 +
 README.md                                          |  50 +--
 crates/rommfs-app/src/controller/state.rs          |  32 ++
 crates/rommfs-app/src/problem.rs                   |  16 +-
 crates/rommfs-app/src/window.rs                    |  25 +-
 crates/rommfs-app/src/window/components.rs         | 125 ++++--
 crates/rommfs-app/src/window/folder_controls.rs    | 347 ++++++-----------
 crates/rommfs-app/src/window/pages/activity.rs     |  52 +--
 crates/rommfs-app/src/window/pages/overview.rs     | 423 +++++++++------------
 crates/rommfs-app/src/window/pages/save_review.rs  |  21 +-
 crates/rommfs-app/src/window/pages/settings.rs     |  50 ++-
 crates/rommfs-app/src/window/pages/setup.rs        |  25 +-
 .../tests/controller/session_and_downloads.rs      |  83 ++++
 docs/architecture.md                               |  18 +
 docs/user-guide.md                                 |  63 ++-
 docs/ux-remediation.md                             |  37 ++
 31 files changed, 744 insertions(+), 636 deletions(-)
]]></tool_result>
<tool_result tool_call_id="toolu_01ALs32qPgAc3DiJm2baBP9L" source="tool_result" timestamp="2026-10-27T09:04:43.327Z"><![CDATA[
9492a7e 2026-10-26 21:36:51 +0000 feat(ui): clarify app status and setup guidance
6f46a7c 2026-10-26 18:18:35 +1100 Merge pull request #6 from auron-labs/linux-fuse-redesign
c3b6a01 2026-10-26 05:54:29 +0000 test: update stale gamelist-published log assertion
733a9d8 2026-10-26 05:54:29 +0000 feat: apply new dark shell design across app pages
f62096f 2026-10-26 05:54:22 +0000 feat: add read-only FUSE mount backend for Linux testing
fea973b 2026-10-26 05:07:27 +0000 chore: cleanup
40e59cf 2026-10-24 07:27:00 +0000 chore(agents): add workflow skills and config
360e13a 2026-10-19 14:04:12 +1100 feat: improve windows
52513e8 2026-10-18 01:00:45 +0000 feat: improve save sync review and shutdown handling
55b5c10 2026-10-17 09:58:28 +0000 chore: human-testing
84068e6 2026-10-17 04:14:12 +0000 chore: add wizard
4b95a63 2026-10-15 06:04:05 +0000 feat: finish human-test fixes and memory-card sync
230b9a1 2026-10-15 02:07:15 +0000 feat: sync favourites and play history with RomM
a822d74 2026-10-15 01:32:16 +0000 feat(app): reorganize Settings into tabs and trim page copy
5a7be56 2026-10-15 01:01:45 +0000 feat: fix ui issues with scrolling
commit 9492a7e99987cb6bdae45e9311ff671975107bd9
Author: Aaron Florey <azza@jcaks.net>
Date:   Mon Oct 26 21:36:51 2026 +0000

    feat(ui): clarify app status and setup guidance
    
    - Improve games folder actions, progress states, and user friendly errors
    - Refresh onboarding and user guide; add human testing notes and screenshots

 .../human-testing-2026-10-26/10-26-26-192216.png   | Bin 0 -> 69785 bytes
 .../human-testing-2026-10-26/10-26-26-192225.png   | Bin 0 -> 41277 bytes
 .../human-testing-2026-10-26/10-26-26-192231.png   | Bin 0 -> 76771 bytes
 .../human-testing-2026-10-26/10-26-26-192235.png   | Bin 0 -> 82116 bytes
 .../human-testing-2026-10-26/10-26-26-192304.png   | Bin 0 -> 48755 bytes
 .../human-testing-2026-10-26/10-26-26-192308.png   | Bin 0 -> 36693 bytes
 .../human-testing-2026-10-26/10-26-26-192312.png   | Bin 0 -> 50319 bytes
 .../human-testing-2026-10-26/10-26-26-192315.png   | Bin 0 -> 50430 bytes
 .../human-testing-2026-10-26/10-26-26-192318.png   | Bin 0 -> 61861 bytes
 .../human-testing-2026-10-26/10-26-26-192322.png   | Bin 0 -> 42313 bytes
 .../human-testing-2026-10-26/10-26-26-192330.png   | Bin 0 -> 83971 bytes
 .../human-testing-2026-10-26/10-26-26-192345.png   | Bin 0 -> 83867 bytes
 .../human-testing-2026-10-26/10-26-26-192848.png   | Bin 0 -> 55527 bytes
 .../human-testing-2026-10-26/10-26-26-192854.png   | Bin 0 -> 76429 bytes
 .../human-testing-2026-10-26/10-26-26-193202.png   | Bin 0 -> 157914 bytes
 .scratch/human-testing-2026-10-26/notes.md         |  13 +
 README.md                                          |  50 +--
 crates/rommfs-app/src/controller/state.rs          |  32 ++
 crates/rommfs-app/src/problem.rs                   |  16 +-
 crates/rommfs-app/src/window.rs                    |  25 +-
 crates/rommfs-app/src/window/components.rs         | 125 ++++--
 crates/rommfs-app/src/window/folder_controls.rs    | 347 ++++++-----------
 crates/rommfs-app/src/window/pages/activity.rs     |  52 +--
 crates/rommfs-app/src/window/pages/overview.rs     | 423 +++++++++------------
 crates/rommfs-app/src/window/pages/save_review.rs  |  21 +-
 crates/rommfs-app/src/window/pages/settings.rs     |  50 ++-
 crates/rommfs-app/src/window/pages/setup.rs        |  25 +-
 .../tests/controller/session_and_downloads.rs      |  83 ++++
 docs/architecture.md                               |  18 +
 docs/user-guide.md                                 |  63 ++-
 docs/ux-remediation.md                             |  37 ++
 31 files changed, 744 insertions(+), 636 deletions(-)
]]></tool_result>
</message>
<message role="assistant" ordinal="31" timestamp="2026-10-27T09:04:48.164Z" has_thinking="true" has_tool_use="true">
<thinking><![CDATA[
Since these testing notes were added in the same commit as the screenshots, they likely reflect the state at testing time and may already be partially addressed. The notes are still unchecked, so I want to verify one in particular—why RomMFS would require exclusive access to retrobat.exe—by searching the code for that.
]]></thinking>
<tool_call id="toolu_018kDdGAPBzPny3AFwAoZC4k" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"rg -n 'Exclusive access' crates | head; rg -n 'retrobat.exe' crates/rommfs-core/src crates/rommfs-app/src | head -20","description":"Find exclusive access message and retrobat.exe usage"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ rg -n 'Exclusive access' crates | head; rg -n 'retrobat.exe' crates/rommfs-core/src crates/rommfs-app/src | head -20
]]></tool_body>
<tool_result><![CDATA[
crates/rommfs-app/src/controller.rs:1092:                .open(&path).with_context(|| format!("Exclusive access to {} is unavailable for RetroBat setup or metadata publication; close RetroBat, its emulators, or any other program using this file", path.display()))?);
crates/rommfs-core/src/save_sync/installation.rs:181:    let source = if executable.eq_ignore_ascii_case("retrobat.exe") {
crates/rommfs-app/src/retrobat_discovery_windows.rs:96:            name.eq_ignore_ascii_case("retrobat.exe")
crates/rommfs-app/src/controller.rs:1080:            "retrobat.exe",
crates/rommfs-core/src/retrobat/setup.rs:99:    let executable = installation.join("retrobat.exe");
crates/rommfs-app/src/controller/tests/connection.rs:54:    std::fs::write(installation.join("retrobat.exe"), b"fixture").unwrap();
]]></tool_result>
<tool_result tool_call_id="toolu_018kDdGAPBzPny3AFwAoZC4k" source="tool_result" timestamp="2026-10-27T09:04:48.995Z"><![CDATA[
crates/rommfs-app/src/controller.rs:1092:                .open(&path).with_context(|| format!("Exclusive access to {} is unavailable for RetroBat setup or metadata publication; close RetroBat, its emulators, or any other program using this file", path.display()))?);
crates/rommfs-core/src/save_sync/installation.rs:181:    let source = if executable.eq_ignore_ascii_case("retrobat.exe") {
crates/rommfs-app/src/retrobat_discovery_windows.rs:96:            name.eq_ignore_ascii_case("retrobat.exe")
crates/rommfs-app/src/controller.rs:1080:            "retrobat.exe",
crates/rommfs-core/src/retrobat/setup.rs:99:    let executable = installation.join("retrobat.exe");
crates/rommfs-app/src/controller/tests/connection.rs:54:    std::fs::write(installation.join("retrobat.exe"), b"fixture").unwrap();
]]></tool_result>
</message>
<message role="assistant" ordinal="33" timestamp="2026-10-27T09:04:50.719Z" has_tool_use="true">
<tool_call id="toolu_01C9rwHcLMUJPF1gfmaWGioi" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"sed -n 1040,1130p crates/rommfs-app/src/controller.rs","description":"Read exclusive-access lock code"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ sed -n 1040,1130p crates/rommfs-app/src/controller.rs
]]></tool_body>
<tool_result><![CDATA[
}

// ---------------------------------------------------------------------------
// Download wiring: RomM client -> ContentSource for the DownloadManager.
// ---------------------------------------------------------------------------

struct ClientSource {
    client: Arc<RommClient>,
    /// `RomKey -> content_name` (the `file_name` used in the content URL).
    names: HashMap<RomKey, String>,
}

impl ContentSource for ClientSource {
    fn fetch(
        &self,
        key: &RomKey,
        writer: &mut dyn std::io::Write,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<u64> {
        let name = self.names.get(key).ok_or_else(|| {
            Error::InvalidCatalogue(format!("rom {} not in catalogue", key.rom_id))
        })?;
        self.client
            .download_original_file(key.file_id, name, writer, progress)
    }
}

// ---------------------------------------------------------------------------
// Mount root + backend seam.
// ---------------------------------------------------------------------------

/// Deny setup/publication while Windows has a frontend or Switch emulator image
/// open. Exclusive writable handles also prevent launch during the operation.
pub(crate) fn stopped_retrobat_guard(installation: &Path) -> anyhow::Result<Vec<std::fs::File>> {
    rommfs_core::save_sync::ensure_no_reparse_components(installation)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let mut guards = Vec::new();
        for relative in [
            "retrobat.exe",
            "emulationstation/emulationstation.exe",
            "emulators/eden/eden.exe",
            "emulators/citron/citron.exe",
            "emulators/retroarch/retroarch.exe",
        ] {
            let path = installation.join(relative);
            rommfs_core::save_sync::ensure_no_reparse_components(&path)?;
            if !path.try_exists()? {
                continue;
            }
            guards.push(std::fs::OpenOptions::new().read(true).write(true).share_mode(0)
                .open(&path).with_context(|| format!("Exclusive access to {} is unavailable for RetroBat setup or metadata publication; close RetroBat, its emulators, or any other program using this file", path.display()))?);
        }
        Ok(guards)
    }
    #[cfg(not(windows))]
    {
        Ok(Vec::new())
    }
}

/// Whether a PS1/PS2 card emulator image is clear to touch a card.
pub(crate) enum CardEmulatorState {
    /// Exclusive handles are held for the pass.
    Stopped(Vec<std::fs::File>),
    /// An emulator image is open, so a shared card may be in use.
    #[cfg_attr(not(windows), allow(dead_code))]
    Running,
    /// The guard could not be established; the caller must not sync.
    Unavailable(String),
}

/// Deny card access while a DuckStation or PCSX2 image is open. Instead of
/// guessing executable names, every executable inside each installed emulator
/// folder is opened exclusively; an installed folder with no executable yields
/// [`CardEmulatorState::Unavailable`], never a silent pass.
pub(crate) fn card_emulator_state(installation: &Path) -> CardEmulatorState {
    if let Err(error) = rommfs_core::save_sync::ensure_no_reparse_components(installation) {
        return CardEmulatorState::Unavailable(error.to_string());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let mut guards = Vec::new();
        for emulator in ["duckstation", "pcsx2"] {
            let directory = installation.join("emulators").join(emulator);
            match std::fs::symlink_metadata(&directory) {
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => {
                    return CardEmulatorState::Unavailable(format!(
]]></tool_result>
<tool_result tool_call_id="toolu_01C9rwHcLMUJPF1gfmaWGioi" source="tool_result" timestamp="2026-10-27T09:04:51.444Z"><![CDATA[
}

// ---------------------------------------------------------------------------
// Download wiring: RomM client -> ContentSource for the DownloadManager.
// ---------------------------------------------------------------------------

struct ClientSource {
    client: Arc<RommClient>,
    /// `RomKey -> content_name` (the `file_name` used in the content URL).
    names: HashMap<RomKey, String>,
}

impl ContentSource for ClientSource {
    fn fetch(
        &self,
        key: &RomKey,
        writer: &mut dyn std::io::Write,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<u64> {
        let name = self.names.get(key).ok_or_else(|| {
            Error::InvalidCatalogue(format!("rom {} not in catalogue", key.rom_id))
        })?;
        self.client
            .download_original_file(key.file_id, name, writer, progress)
    }
}

// ---------------------------------------------------------------------------
// Mount root + backend seam.
// ---------------------------------------------------------------------------

/// Deny setup/publication while Windows has a frontend or Switch emulator image
/// open. Exclusive writable handles also prevent launch during the operation.
pub(crate) fn stopped_retrobat_guard(installation: &Path) -> anyhow::Result<Vec<std::fs::File>> {
    rommfs_core::save_sync::ensure_no_reparse_components(installation)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let mut guards = Vec::new();
        for relative in [
            "retrobat.exe",
            "emulationstation/emulationstation.exe",
            "emulators/eden/eden.exe",
            "emulators/citron/citron.exe",
            "emulators/retroarch/retroarch.exe",
        ] {
            let path = installation.join(relative);
            rommfs_core::save_sync::ensure_no_reparse_components(&path)?;
            if !path.try_exists()? {
                continue;
            }
            guards.push(std::fs::OpenOptions::new().read(true).write(true).share_mode(0)
                .open(&path).with_context(|| format!("Exclusive access to {} is unavailable for RetroBat setup or metadata publication; close RetroBat, its emulators, or any other program using this file", path.display()))?);
        }
        Ok(guards)
    }
    #[cfg(not(windows))]
    {
        Ok(Vec::new())
    }
}

/// Whether a PS1/PS2 card emulator image is clear to touch a card.
pub(crate) enum CardEmulatorState {
    /// Exclusive handles are held for the pass.
    Stopped(Vec<std::fs::File>),
    /// An emulator image is open, so a shared card may be in use.
    #[cfg_attr(not(windows), allow(dead_code))]
    Running,
    /// The guard could not be established; the caller must not sync.
    Unavailable(String),
}

/// Deny card access while a DuckStation or PCSX2 image is open. Instead of
/// guessing executable names, every executable inside each installed emulator
/// folder is opened exclusively; an installed folder with no executable yields
/// [`CardEmulatorState::Unavailable`], never a silent pass.
pub(crate) fn card_emulator_state(installation: &Path) -> CardEmulatorState {
    if let Err(error) = rommfs_core::save_sync::ensure_no_reparse_components(installation) {
        return CardEmulatorState::Unavailable(error.to_string());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let mut guards = Vec::new();
        for emulator in ["duckstation", "pcsx2"] {
            let directory = installation.join("emulators").join(emulator);
            match std::fs::symlink_metadata(&directory) {
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => {
                    return CardEmulatorState::Unavailable(format!(
]]></tool_result>
</message>
<message role="assistant" ordinal="35" timestamp="2026-10-27T09:05:09.056Z" has_thinking="true" has_tool_use="true">
<thinking><![CDATA[
Opening exe files for exclusive read+write without proper share mode likely triggers Windows Defender to scan them on close, which could explain both the "os error 32" bug when RetroBat isn't running and the reported high Defender CPU usage—this feels like a solid root cause candidate.

I'm also wondering about the mount slowness: creating placeholders for 11.5k files, setting ACLs, handling artwork for 2,466 games, and scanning 1,864 save files all add up. I want to check the architecture doc for data flow details, plus look into the note about romm-fs not noticing when the romm folder gets removed from RetroBat's roms directory. I need to stay efficient here since the user wants a plan.

Given the two-day deadline, I should focus on diagnosing why things feel confusing or conflicting using concrete code evidence, defining a realistic "finished" scope by cutting ambition down from full RomM feature parity, and laying out a day-by-day plan. To do that I want to count how many places compute status labels (overview, activity, settings, sidebar, tray), check the size of apply_event and its variants, and tally the feature surface — memory cards, play data, favourites, BIOS/firmware/cheats, installer, legacy GB profiles, hide copies, Switch updates, artwork, save sync — since that's a lot for a two-day scope. I also want to check the mount flow in controller/mount.rs and see if the test suite currently passes, since running tests without making changes should be safe.

Likely found the cause: RomMFS opens retrobat.exe with an exclusive write lock while Windows Defender is also holding it open. I'm now kicking off the portable test suite in the background to establish a baseline.
]]></thinking>
<tool_call id="toolu_01DkQx1X9h2aTdEepFnPg58Y" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"cargo test --locked -p rommfs-core -p rommfs-fixture -p rommfs-cfapi -p rommfs-app --no-default-features -- --test-threads=1 2>&1 | grep -E '^test result|FAILED|panicked|error(\\[|:)' | tail -40","description":"Run portable test suite for baseline","timeout":1200000,"run_in_background":true}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ cargo test --locked -p rommfs-core -p rommfs-fixture -p rommfs-cfapi -p rommfs-app --no-default-features -- --test-threads=1 2>&1 | grep -E '^test result|FAILED|panicked|error(\[|:)' | tail -40
]]></tool_body>
<tool_result><![CDATA[
Command running in background with ID: ba6v4ztu3. Output is being written to: /tmp/claude-1000/-home-aaron-Code-romm-fs/9d37bbd8-bbbc-4344-ab8a-4a7faabe6c42/tasks/ba6v4ztu3.output. You will be notified when it completes. To check interim output, use Read on that file path.
]]></tool_result>
<tool_result tool_call_id="toolu_01DkQx1X9h2aTdEepFnPg58Y" source="tool_result" timestamp="2026-10-27T09:05:09.806Z"><![CDATA[
Command running in background with ID: ba6v4ztu3. Output is being written to: /tmp/claude-1000/-home-aaron-Code-romm-fs/9d37bbd8-bbbc-4344-ab8a-4a7faabe6c42/tasks/ba6v4ztu3.output. You will be notified when it completes. To check interim output, use Read on that file path.
]]></tool_result>
<tool_call id="toolu_01RKZusCRxANJn2kB4Cpyemc" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"grep -c 'AppEvent::' crates/rommfs-app/src/controller/state.rs; grep -n '^    [A-Z][A-Za-z]*' crates/rommfs-core/src/events.rs | wc -l; rg -n 'enum AppEvent' -A3 crates/rommfs-core/src/events.rs; ls crates/rommfs-app/src/controller/; wc -l crates/rommfs-app/src/controller/*.rs | tail -1","description":"Count event variants and controller modules"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ grep -c 'AppEvent::' crates/rommfs-app/src/controller/state.rs; grep -n '^    [A-Z][A-Za-z]*' crates/rommfs-core/src/events.rs | wc -l; rg -n 'enum AppEvent' -A3 crates/rommfs-core/src/events.rs; ls crates/rommfs-app/src/controller/; wc -l crates/rommfs-app/src/controller/*.rs | tail -1
]]></tool_body>
<tool_result><![CDATA[
50
67
166:pub enum AppEvent {
167-    ShutdownStarting,
168-    SaveSyncCheckStarted {
169-        session_id: u64,
auxiliary.rs
catalogue.rs
commands.rs
connection.rs
installer.rs
memory_cards.rs
mount.rs
play_data.rs
retrobat_setup.rs
save_files.rs
save_sync_setup.rs
state.rs
tests
tests.rs
  5915 total
]]></tool_result>
<tool_result tool_call_id="toolu_01RKZusCRxANJn2kB4Cpyemc" source="tool_result" timestamp="2026-10-27T09:05:10.111Z"><![CDATA[
50
67
166:pub enum AppEvent {
167-    ShutdownStarting,
168-    SaveSyncCheckStarted {
169-        session_id: u64,
auxiliary.rs
catalogue.rs
commands.rs
connection.rs
installer.rs
memory_cards.rs
mount.rs
play_data.rs
retrobat_setup.rs
save_files.rs
save_sync_setup.rs
state.rs
tests
tests.rs
  5915 total
]]></tool_result>
</message>
<message role="assistant" ordinal="38" timestamp="2026-10-27T09:05:14.823Z" has_thinking="true" has_tool_use="true">
<thinking><![CDATA[
I should investigate what's actually slow here, looking at how "Preparing games" is implemented in cfapi's windows.rs and how catalogue refreshes require remounting for library changes—that restart interrupts file access temporarily, which is a real UX friction point tied to the snapshot invariant. Let me trace through the mount flow to understand this better.
]]></thinking>
<tool_call id="toolu_01TZioKA7P6joyYVBqtUQPBH" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"sed -n 1,80p docs/architecture.md","description":"Read architecture doc overview"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ sed -n 1,80p docs/architecture.md
]]></tool_body>
<tool_result><![CDATA[
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
]]></tool_result>
<tool_result tool_call_id="toolu_01TZioKA7P6joyYVBqtUQPBH" source="tool_result" timestamp="2026-10-27T09:05:15.797Z"><![CDATA[
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
]]></tool_result>
<tool_call id="toolu_01MnqHsvyFwL1hdGGvyyHfFs" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"rg -n 'fn |SetSecurity|set_security|SetFileSecurity|CfCreatePlaceholders|create_placeholders|thread::sleep|sleep\\(' crates/rommfs-cfapi/src/windows.rs | head -60","description":"Scan CFAPI adapter for mount hot paths"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ rg -n 'fn |SetSecurity|set_security|SetFileSecurity|CfCreatePlaceholders|create_placeholders|thread::sleep|sleep\(' crates/rommfs-cfapi/src/windows.rs | head -60
]]></tool_body>
<tool_result><![CDATA[
25:fn wide(path: &Path) -> anyhow::Result<Vec<u16>> {
31:fn hr(result: i32, action: &str) -> anyhow::Result<()> {
42:pub fn check_prerequisites(root: &Path) -> anyhow::Result<()> {
82:fn open_metadata(path: &Path, exclusive: bool) -> anyhow::Result<File> {
95:pub(crate) fn check_root_directory(root: &Path) -> anyhow::Result<()> {
129:fn placeholder_info(file: &File) -> anyhow::Result<(CF_PLACEHOLDER_STANDARD_INFO, Vec<u8>)> {
170:fn verify_handle(file: &File, entry: &OwnedEntry) -> anyhow::Result<()> {
198:pub(crate) fn verify_owned(path: &Path, entry: &OwnedEntry) -> anyhow::Result<()> {
202:pub(crate) fn make_directory_writable(path: &Path) -> anyhow::Result<()> {
206:pub(crate) fn make_local_writable(path: &Path) -> anyhow::Result<()> {
213:pub(crate) fn hold_local_directory_chain(
253:    pub fn new(root: impl AsRef<Path>) -> Self {
260:    fn remove_hydrated(&self, relative: &str) -> std::io::Result<()> {
314:    fn drop(&mut self) {
321:    fn enter(&self) -> CallbackGuard<'_> {
325:    fn inode(&self, info: &CF_CALLBACK_INFO) -> Option<u64> {
348:    pub fn mount(core: Arc<RommFs>, root: impl AsRef<Path>) -> anyhow::Result<Self> {
522:    pub fn stop(self) {
525:    pub fn root(&self) -> &Path {
531:    fn drop(&mut self) {
640:fn manifest_for(core: &RommFs, previous: &Manifest) -> anyhow::Result<Manifest> {
711:fn registration_flags(root: &Path, identity: &str) -> anyhow::Result<CF_REGISTER_FLAGS> {
777:fn register_root(root: &Path, server: &str) -> anyhow::Result<()> {
812:fn create_placeholder(root: &Path, entry: &OwnedEntry) -> anyhow::Result<()> {
857:            CfCreatePlaceholders(
885:fn remove_entries(root: &Path, manifest: &Manifest) -> Vec<String> {
992:fn operation(info: &CF_CALLBACK_INFO, kind: CF_OPERATION_TYPE) -> CF_OPERATION_INFO {
1004:fn transfer(
1029:unsafe extern "system" fn fetch_data(
1131:unsafe extern "system" fn opened(
1157:unsafe extern "system" fn deny_delete(
1189:unsafe extern "system" fn deny_rename(
]]></tool_result>
<tool_result tool_call_id="toolu_01MnqHsvyFwL1hdGGvyyHfFs" source="tool_result" timestamp="2026-10-27T09:05:15.872Z"><![CDATA[
25:fn wide(path: &Path) -> anyhow::Result<Vec<u16>> {
31:fn hr(result: i32, action: &str) -> anyhow::Result<()> {
42:pub fn check_prerequisites(root: &Path) -> anyhow::Result<()> {
82:fn open_metadata(path: &Path, exclusive: bool) -> anyhow::Result<File> {
95:pub(crate) fn check_root_directory(root: &Path) -> anyhow::Result<()> {
129:fn placeholder_info(file: &File) -> anyhow::Result<(CF_PLACEHOLDER_STANDARD_INFO, Vec<u8>)> {
170:fn verify_handle(file: &File, entry: &OwnedEntry) -> anyhow::Result<()> {
198:pub(crate) fn verify_owned(path: &Path, entry: &OwnedEntry) -> anyhow::Result<()> {
202:pub(crate) fn make_directory_writable(path: &Path) -> anyhow::Result<()> {
206:pub(crate) fn make_local_writable(path: &Path) -> anyhow::Result<()> {
213:pub(crate) fn hold_local_directory_chain(
253:    pub fn new(root: impl AsRef<Path>) -> Self {
260:    fn remove_hydrated(&self, relative: &str) -> std::io::Result<()> {
314:    fn drop(&mut self) {
321:    fn enter(&self) -> CallbackGuard<'_> {
325:    fn inode(&self, info: &CF_CALLBACK_INFO) -> Option<u64> {
348:    pub fn mount(core: Arc<RommFs>, root: impl AsRef<Path>) -> anyhow::Result<Self> {
522:    pub fn stop(self) {
525:    pub fn root(&self) -> &Path {
531:    fn drop(&mut self) {
640:fn manifest_for(core: &RommFs, previous: &Manifest) -> anyhow::Result<Manifest> {
711:fn registration_flags(root: &Path, identity: &str) -> anyhow::Result<CF_REGISTER_FLAGS> {
777:fn register_root(root: &Path, server: &str) -> anyhow::Result<()> {
812:fn create_placeholder(root: &Path, entry: &OwnedEntry) -> anyhow::Result<()> {
857:            CfCreatePlaceholders(
885:fn remove_entries(root: &Path, manifest: &Manifest) -> Vec<String> {
992:fn operation(info: &CF_CALLBACK_INFO, kind: CF_OPERATION_TYPE) -> CF_OPERATION_INFO {
1004:fn transfer(
1029:unsafe extern "system" fn fetch_data(
1131:unsafe extern "system" fn opened(
1157:unsafe extern "system" fn deny_delete(
1189:unsafe extern "system" fn deny_rename(
]]></tool_result>
</message>
<message role="assistant" ordinal="41" timestamp="2026-10-27T09:05:17.715Z" has_tool_use="true">
<tool_call id="toolu_01GyVvtukPCKMFauTXQN6m4C" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"sed -n 348,520p crates/rommfs-cfapi/src/windows.rs","description":"Read CFAPI mount function"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ sed -n 348,520p crates/rommfs-cfapi/src/windows.rs
]]></tool_body>
<tool_result><![CDATA[
    pub fn mount(core: Arc<RommFs>, root: impl AsRef<Path>) -> anyhow::Result<Self> {
        let started = std::time::Instant::now();
        core.events()
            .emit(rommfs_core::events::AppEvent::MountProgress {
                stage: "Root preparation".into(),
                completed: 0,
                total: None,
                elapsed_ms: 0,
            });
        root::check_directory(root.as_ref())?;
        let root = std::fs::canonicalize(root.as_ref())?;
        check_prerequisites(&root)?;
        let lock_path = root::sidecar_path(&root, ".rommfs-lock")?;
        std::fs::create_dir_all(lock_path.parent().context("missing parent")?)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .open(&lock_path)
            .with_context(|| {
                format!(
                    "open exclusive mount lock {}; another session may hold it",
                    lock_path.display()
                )
            })?;
        let directories = rommfs_core::save_sync::hold_save_directory_chain(
            root.parent().context("missing root parent")?,
        )?;
        let mut previous = root::read_manifest(&root).context("claim the root before mounting")?;
        // Adopt the files a frontend legitimately wrote between mounts; report
        // every other foreign entry at once instead of failing on the first.
        let scan = root::scan_tree(&root, &previous, verify_owned)?;
        let unowned = root::adopt_entries(&root, &mut previous, scan.unowned)?;
        if !unowned.is_empty() {
            return Err(anyhow::Error::new(root::UnownedEntries(unowned)));
        }
        anyhow::ensure!(
            scan.failures.is_empty(),
            "owned entries failed verification: {}",
            scan.failures.join("; ")
        );
        let mut manifest = manifest_for(&core, &previous)?;
        let root_handle = open_metadata(&root, false)?;
        manifest.root_security = if previous.root_security.is_empty() {
            security::snapshot(&root_handle)
                .with_context(|| format!("read original ACL for {}", root.display()))?
        } else {
            previous.root_security.clone()
        };
        // Persist the original ACL before protecting the root, including crash recovery.
        previous.root_security = manifest.root_security.clone();
        root::write_manifest(&root, &previous)?;
        let inodes = manifest
            .entries
            .iter()
            .filter(|entry| !entry.is_local() && !entry.directory)
            .map(|entry| {
                let parts: Vec<_> = entry.path.split('/').collect();
                let inode = core
                    .inode_for_path(&parts)
                    .expect("manifest constructed from core tree");
                (entry.identity.clone(), inode)
            })
            .collect();
        let mut mount = Self {
            connection: None,
            registered: false,
            context: Box::new(CallbackContext {
                core,
                inodes,
                stopping: AtomicBool::new(false),
                callbacks: Mutex::new(0),
                idle: Condvar::new(),
            }),
            root,
            manifest: previous,
            _lock: lock,
            _root_handle: root_handle.try_clone()?,
            _directories: directories,
        };
        register_root(&mount.root, &manifest.server_id)?;
        mount.registered = true;
        security::set(&root_handle, security::MIXED_DIRECTORY).with_context(|| {
            format!(
                "apply protected mixed-content directory ACL to {}",
                mount.root.display()
            )
        })?;
        let scan = root::scan_tree(&mount.root, &mount.manifest, verify_owned)?;
        let unowned = root::adopt_entries(&mount.root, &mut mount.manifest, scan.unowned)?;
        // ponytail: rebuilds owned placeholders on start; reconcile unchanged entries
        // if catalogue startup costs become significant. Private bytes survive.
        let mut failures = scan.failures;
        failures.extend(remove_entries(&mount.root, &mount.manifest));
        if !unowned.is_empty() {
            failures.push(format!("{}", root::UnownedEntries(unowned)));
        }
        anyhow::ensure!(failures.is_empty(), "{}", failures.join("; "));
        root::write_manifest(&mount.root, &manifest)?;
        mount.manifest = manifest;
        let callbacks = [
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_FETCH_DATA,
                Callback: Some(fetch_data),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NOTIFY_FILE_OPEN_COMPLETION,
                Callback: Some(opened),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NOTIFY_DELETE,
                Callback: Some(deny_delete),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NOTIFY_RENAME,
                Callback: Some(deny_rename),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NONE,
                Callback: None,
            },
        ];
        let path = wide(&mount.root)?;
        let mut connection = 0;
        // SAFETY: context allocation stays fixed until disconnect and callbacks drain.
        // CFAPI copies the terminated registration table during this call.
        hr(
            unsafe {
                CfConnectSyncRoot(
                    path.as_ptr(),
                    callbacks.as_ptr(),
                    (&*mount.context as *const CallbackContext).cast(),
                    CF_CONNECT_FLAG_REQUIRE_PROCESS_INFO,
                    &mut connection,
                )
            },
            "connect Cloud Files sync root",
        )?;
        mount.connection = Some(connection);
        let total = mount.manifest.entries.len() as u64;
        mount
            .context
            .core
            .events()
            .emit(rommfs_core::events::AppEvent::MountProgress {
                stage: "Placeholder creation".into(),
                completed: 0,
                total: Some(total),
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
        for (index, entry) in mount.manifest.entries.iter().enumerate() {
            create_placeholder(&mount.root, entry).with_context(|| {
                format!(
                    "create placeholder {}",
                    mount.root.join(&entry.path).display()
                )
            })?;
            if (index + 1) % 64 == 0 || index + 1 == total as usize {
                mount
                    .context
                    .core
                    .events()
                    .emit(rommfs_core::events::AppEvent::MountProgress {
                        stage: "Placeholder creation".into(),
                        completed: (index + 1) as u64,
                        total: Some(total),
                        elapsed_ms: started.elapsed().as_millis() as u64,
                    });
            }
        }
        Ok(mount)
]]></tool_result>
<tool_result tool_call_id="toolu_01GyVvtukPCKMFauTXQN6m4C" source="tool_result" timestamp="2026-10-27T09:05:19.385Z"><![CDATA[
    pub fn mount(core: Arc<RommFs>, root: impl AsRef<Path>) -> anyhow::Result<Self> {
        let started = std::time::Instant::now();
        core.events()
            .emit(rommfs_core::events::AppEvent::MountProgress {
                stage: "Root preparation".into(),
                completed: 0,
                total: None,
                elapsed_ms: 0,
            });
        root::check_directory(root.as_ref())?;
        let root = std::fs::canonicalize(root.as_ref())?;
        check_prerequisites(&root)?;
        let lock_path = root::sidecar_path(&root, ".rommfs-lock")?;
        std::fs::create_dir_all(lock_path.parent().context("missing parent")?)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .open(&lock_path)
            .with_context(|| {
                format!(
                    "open exclusive mount lock {}; another session may hold it",
                    lock_path.display()
                )
            })?;
        let directories = rommfs_core::save_sync::hold_save_directory_chain(
            root.parent().context("missing root parent")?,
        )?;
        let mut previous = root::read_manifest(&root).context("claim the root before mounting")?;
        // Adopt the files a frontend legitimately wrote between mounts; report
        // every other foreign entry at once instead of failing on the first.
        let scan = root::scan_tree(&root, &previous, verify_owned)?;
        let unowned = root::adopt_entries(&root, &mut previous, scan.unowned)?;
        if !unowned.is_empty() {
            return Err(anyhow::Error::new(root::UnownedEntries(unowned)));
        }
        anyhow::ensure!(
            scan.failures.is_empty(),
            "owned entries failed verification: {}",
            scan.failures.join("; ")
        );
        let mut manifest = manifest_for(&core, &previous)?;
        let root_handle = open_metadata(&root, false)?;
        manifest.root_security = if previous.root_security.is_empty() {
            security::snapshot(&root_handle)
                .with_context(|| format!("read original ACL for {}", root.display()))?
        } else {
            previous.root_security.clone()
        };
        // Persist the original ACL before protecting the root, including crash recovery.
        previous.root_security = manifest.root_security.clone();
        root::write_manifest(&root, &previous)?;
        let inodes = manifest
            .entries
            .iter()
            .filter(|entry| !entry.is_local() && !entry.directory)
            .map(|entry| {
                let parts: Vec<_> = entry.path.split('/').collect();
                let inode = core
                    .inode_for_path(&parts)
                    .expect("manifest constructed from core tree");
                (entry.identity.clone(), inode)
            })
            .collect();
        let mut mount = Self {
            connection: None,
            registered: false,
            context: Box::new(CallbackContext {
                core,
                inodes,
                stopping: AtomicBool::new(false),
                callbacks: Mutex::new(0),
                idle: Condvar::new(),
            }),
            root,
            manifest: previous,
            _lock: lock,
            _root_handle: root_handle.try_clone()?,
            _directories: directories,
        };
        register_root(&mount.root, &manifest.server_id)?;
        mount.registered = true;
        security::set(&root_handle, security::MIXED_DIRECTORY).with_context(|| {
            format!(
                "apply protected mixed-content directory ACL to {}",
                mount.root.display()
            )
        })?;
        let scan = root::scan_tree(&mount.root, &mount.manifest, verify_owned)?;
        let unowned = root::adopt_entries(&mount.root, &mut mount.manifest, scan.unowned)?;
        // ponytail: rebuilds owned placeholders on start; reconcile unchanged entries
        // if catalogue startup costs become significant. Private bytes survive.
        let mut failures = scan.failures;
        failures.extend(remove_entries(&mount.root, &mount.manifest));
        if !unowned.is_empty() {
            failures.push(format!("{}", root::UnownedEntries(unowned)));
        }
        anyhow::ensure!(failures.is_empty(), "{}", failures.join("; "));
        root::write_manifest(&mount.root, &manifest)?;
        mount.manifest = manifest;
        let callbacks = [
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_FETCH_DATA,
                Callback: Some(fetch_data),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NOTIFY_FILE_OPEN_COMPLETION,
                Callback: Some(opened),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NOTIFY_DELETE,
                Callback: Some(deny_delete),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NOTIFY_RENAME,
                Callback: Some(deny_rename),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NONE,
                Callback: None,
            },
        ];
        let path = wide(&mount.root)?;
        let mut connection = 0;
        // SAFETY: context allocation stays fixed until disconnect and callbacks drain.
        // CFAPI copies the terminated registration table during this call.
        hr(
            unsafe {
                CfConnectSyncRoot(
                    path.as_ptr(),
                    callbacks.as_ptr(),
                    (&*mount.context as *const CallbackContext).cast(),
                    CF_CONNECT_FLAG_REQUIRE_PROCESS_INFO,
                    &mut connection,
                )
            },
            "connect Cloud Files sync root",
        )?;
        mount.connection = Some(connection);
        let total = mount.manifest.entries.len() as u64;
        mount
            .context
            .core
            .events()
            .emit(rommfs_core::events::AppEvent::MountProgress {
                stage: "Placeholder creation".into(),
                completed: 0,
                total: Some(total),
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
        for (index, entry) in mount.manifest.entries.iter().enumerate() {
            create_placeholder(&mount.root, entry).with_context(|| {
                format!(
                    "create placeholder {}",
                    mount.root.join(&entry.path).display()
                )
            })?;
            if (index + 1) % 64 == 0 || index + 1 == total as usize {
                mount
                    .context
                    .core
                    .events()
                    .emit(rommfs_core::events::AppEvent::MountProgress {
                        stage: "Placeholder creation".into(),
                        completed: (index + 1) as u64,
                        total: Some(total),
                        elapsed_ms: started.elapsed().as_millis() as u64,
                    });
            }
        }
        Ok(mount)
]]></tool_result>
</message>
<message role="assistant" ordinal="43" timestamp="2026-10-27T09:05:25.356Z" has_thinking="true" has_tool_use="true">
<thinking><![CDATA[
I'm pinpointing the real bottleneck: on mount, ponytail deletes and rebuilds all ~11.5k placeholder files/directories, scans the tree twice to verify ownership by opening each file handle, then creates placeholders one at a time—and each creation likely triggers Defender to scan the new file, compounding the slowdown.

Now I'm checking whether create_placeholder sets security attributes per file, which could add further overhead.
]]></thinking>
<tool_call id="toolu_01FdAgtuQNAqfSM8yCKpkYf9" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"sed -n 812,990p crates/rommfs-cfapi/src/windows.rs","description":"Read placeholder creation and removal"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ sed -n 812,990p crates/rommfs-cfapi/src/windows.rs
]]></tool_body>
<tool_result><![CDATA[
fn create_placeholder(root: &Path, entry: &OwnedEntry) -> anyhow::Result<()> {
    let target = root.join(&entry.path);
    if entry.directory && entry.kind == OwnedKind::ManagedDirectory {
        if !target.try_exists()? {
            std::fs::create_dir(&target)?;
        }
        verify_owned(&target, entry)?;
        return make_directory_writable(&target);
    }
    if entry.is_local() {
        return Ok(());
    }
    if entry.directory && target.try_exists()? {
        verify_owned(&target, entry)?;
        return make_directory_writable(&target);
    }
    let parent = wide(target.parent().context("missing parent")?)?;
    let name = wide(Path::new(target.file_name().context("missing filename")?))?;
    let mut info = CF_PLACEHOLDER_CREATE_INFO {
        RelativeFileName: name.as_ptr(),
        FsMetadata: CF_FS_METADATA {
            BasicInfo: FILE_BASIC_INFO {
                FileAttributes: if entry.directory {
                    FILE_ATTRIBUTE_DIRECTORY
                } else {
                    FILE_ATTRIBUTE_NORMAL
                },
                ..FILE_BASIC_INFO::default()
            },
            FileSize: entry.size as i64,
        },
        FileIdentity: entry.identity.as_ptr().cast(),
        FileIdentityLength: entry.identity.len() as u32,
        Flags: CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC
            | if entry.directory {
                CF_PLACEHOLDER_CREATE_FLAG_DISABLE_ON_DEMAND_POPULATION
            } else {
                0
            },
        ..CF_PLACEHOLDER_CREATE_INFO::default()
    };
    let mut processed = 0;
    // SAFETY: valid structures, buffers, identity and output for this call.
    hr(
        unsafe {
            CfCreatePlaceholders(
                parent.as_ptr(),
                &mut info,
                1,
                CF_CREATE_FLAG_STOP_ON_ERROR,
                &mut processed,
            )
        },
        "create placeholder",
    )?;
    anyhow::ensure!(processed == 1, "placeholder was not processed");
    hr(info.Result, "create placeholder entry")?;
    // Apply explicitly as well as inheriting from the protected root.
    security::set(
        &open_metadata(&target, false)?,
        if entry.directory {
            security::MIXED_DIRECTORY
        } else {
            security::READ_ONLY
        },
    )
}

/// Delete each verified owned entry, collecting failures instead of aborting
/// on the first so one blocked file cannot strand the rest of the tree. Every
/// entry is still re-verified before deleting; foreign content is never
/// touched, and directories holding foreign files stay because they are
/// nonempty, not because they are trusted.
fn remove_entries(root: &Path, manifest: &Manifest) -> Vec<String> {
    let _parent_guard = match root
        .parent()
        .context("mount root has no parent")
        .and_then(|parent| {
            rommfs_core::save_sync::hold_save_directory_chain(parent).map_err(anyhow::Error::from)
        }) {
        Ok(guard) => guard,
        Err(e) => return vec![format!("protect mount parent: {e:#}")],
    };
    let mut failures = Vec::new();
    for entry in manifest.entries.iter().rev() {
        if entry.is_local() && !entry.directory {
            continue;
        }
        let path = root.join(&entry.path);
        if std::fs::symlink_metadata(&path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        {
            continue;
        }
        let _directory_guard = match hold_local_directory_chain(
            root,
            path.parent()
                .expect("validated manifest entry has a parent"),
        ) {
            Ok(guard) => guard,
            Err(e) => {
                failures.push(format!("{}: {e:#}", entry.path));
                continue;
            }
        };
        let file = match OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES | READ_CONTROL | WRITE_DAC)
            .share_mode(FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(&path)
        {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                failures.push(format!("{}: {e}", entry.path));
                continue;
            }
        };
        if let Err(e) = verify_handle(&file, entry) {
            failures.push(format!("{}: {e:#}", entry.path));
            continue;
        }
        if entry.directory {
            match std::fs::read_dir(&path) {
                Ok(mut entries) => {
                    if entries.next().is_some() {
                        continue;
                    }
                }
                Err(e) => {
                    failures.push(format!("{}: {e}", entry.path));
                    continue;
                }
            }
        }
        if let Err(e) = security::allow_delete(&file) {
            failures.push(format!("{}: {e:#}", entry.path));
            continue;
        }
        let result = (|| -> anyhow::Result<()> {
            let deleting = OpenOptions::new()
                .access_mode(DELETE | FILE_READ_ATTRIBUTES | WRITE_DAC)
                .share_mode(0)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
                .open(&path)?;
            verify_handle(&deleting, entry)?;
            let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
            // SAFETY: exclusive handle to verified app data; nonempty dirs fail.
            if unsafe {
                SetFileInformationByHandle(
                    deleting.as_raw_handle(),
                    FileDispositionInfo,
                    (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                    size_of::<FILE_DISPOSITION_INFO>() as u32,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            Ok(())
        })();
        // Keep the first handle until permissions are restored on any failed delete.
        let restored = security::set(
            &file,
            if entry.directory {
                security::MIXED_DIRECTORY
            } else {
                security::READ_ONLY
            },
        );
        if let Err(error) = result {
            let message = match restored.context("restore ROM permissions after failed cleanup") {
                Ok(()) => format!("{}: {error:#}", entry.path),
                Err(e) => format!("{}: {error:#}; {e:#}", entry.path),
            };
            failures.push(message);
        }
    }
    failures
}
]]></tool_result>
<tool_result tool_call_id="toolu_01FdAgtuQNAqfSM8yCKpkYf9" source="tool_result" timestamp="2026-10-27T09:05:26.368Z"><![CDATA[
fn create_placeholder(root: &Path, entry: &OwnedEntry) -> anyhow::Result<()> {
    let target = root.join(&entry.path);
    if entry.directory && entry.kind == OwnedKind::ManagedDirectory {
        if !target.try_exists()? {
            std::fs::create_dir(&target)?;
        }
        verify_owned(&target, entry)?;
        return make_directory_writable(&target);
    }
    if entry.is_local() {
        return Ok(());
    }
    if entry.directory && target.try_exists()? {
        verify_owned(&target, entry)?;
        return make_directory_writable(&target);
    }
    let parent = wide(target.parent().context("missing parent")?)?;
    let name = wide(Path::new(target.file_name().context("missing filename")?))?;
    let mut info = CF_PLACEHOLDER_CREATE_INFO {
        RelativeFileName: name.as_ptr(),
        FsMetadata: CF_FS_METADATA {
            BasicInfo: FILE_BASIC_INFO {
                FileAttributes: if entry.directory {
                    FILE_ATTRIBUTE_DIRECTORY
                } else {
                    FILE_ATTRIBUTE_NORMAL
                },
                ..FILE_BASIC_INFO::default()
            },
            FileSize: entry.size as i64,
        },
        FileIdentity: entry.identity.as_ptr().cast(),
        FileIdentityLength: entry.identity.len() as u32,
        Flags: CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC
            | if entry.directory {
                CF_PLACEHOLDER_CREATE_FLAG_DISABLE_ON_DEMAND_POPULATION
            } else {
                0
            },
        ..CF_PLACEHOLDER_CREATE_INFO::default()
    };
    let mut processed = 0;
    // SAFETY: valid structures, buffers, identity and output for this call.
    hr(
        unsafe {
            CfCreatePlaceholders(
                parent.as_ptr(),
                &mut info,
                1,
                CF_CREATE_FLAG_STOP_ON_ERROR,
                &mut processed,
            )
        },
        "create placeholder",
    )?;
    anyhow::ensure!(processed == 1, "placeholder was not processed");
    hr(info.Result, "create placeholder entry")?;
    // Apply explicitly as well as inheriting from the protected root.
    security::set(
        &open_metadata(&target, false)?,
        if entry.directory {
            security::MIXED_DIRECTORY
        } else {
            security::READ_ONLY
        },
    )
}

/// Delete each verified owned entry, collecting failures instead of aborting
/// on the first so one blocked file cannot strand the rest of the tree. Every
/// entry is still re-verified before deleting; foreign content is never
/// touched, and directories holding foreign files stay because they are
/// nonempty, not because they are trusted.
fn remove_entries(root: &Path, manifest: &Manifest) -> Vec<String> {
    let _parent_guard = match root
        .parent()
        .context("mount root has no parent")
        .and_then(|parent| {
            rommfs_core::save_sync::hold_save_directory_chain(parent).map_err(anyhow::Error::from)
        }) {
        Ok(guard) => guard,
        Err(e) => return vec![format!("protect mount parent: {e:#}")],
    };
    let mut failures = Vec::new();
    for entry in manifest.entries.iter().rev() {
        if entry.is_local() && !entry.directory {
            continue;
        }
        let path = root.join(&entry.path);
        if std::fs::symlink_metadata(&path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        {
            continue;
        }
        let _directory_guard = match hold_local_directory_chain(
            root,
            path.parent()
                .expect("validated manifest entry has a parent"),
        ) {
            Ok(guard) => guard,
            Err(e) => {
                failures.push(format!("{}: {e:#}", entry.path));
                continue;
            }
        };
        let file = match OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES | READ_CONTROL | WRITE_DAC)
            .share_mode(FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(&path)
        {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                failures.push(format!("{}: {e}", entry.path));
                continue;
            }
        };
        if let Err(e) = verify_handle(&file, entry) {
            failures.push(format!("{}: {e:#}", entry.path));
            continue;
        }
        if entry.directory {
            match std::fs::read_dir(&path) {
                Ok(mut entries) => {
                    if entries.next().is_some() {
                        continue;
                    }
                }
                Err(e) => {
                    failures.push(format!("{}: {e}", entry.path));
                    continue;
                }
            }
        }
        if let Err(e) = security::allow_delete(&file) {
            failures.push(format!("{}: {e:#}", entry.path));
            continue;
        }
        let result = (|| -> anyhow::Result<()> {
            let deleting = OpenOptions::new()
                .access_mode(DELETE | FILE_READ_ATTRIBUTES | WRITE_DAC)
                .share_mode(0)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
                .open(&path)?;
            verify_handle(&deleting, entry)?;
            let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
            // SAFETY: exclusive handle to verified app data; nonempty dirs fail.
            if unsafe {
                SetFileInformationByHandle(
                    deleting.as_raw_handle(),
                    FileDispositionInfo,
                    (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                    size_of::<FILE_DISPOSITION_INFO>() as u32,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            Ok(())
        })();
        // Keep the first handle until permissions are restored on any failed delete.
        let restored = security::set(
            &file,
            if entry.directory {
                security::MIXED_DIRECTORY
            } else {
                security::READ_ONLY
            },
        );
        if let Err(error) = result {
            let message = match restored.context("restore ROM permissions after failed cleanup") {
                Ok(()) => format!("{}: {error:#}", entry.path),
                Err(e) => format!("{}: {error:#}; {e:#}", entry.path),
            };
            failures.push(message);
        }
    }
    failures
}
]]></tool_result>
</message>
<message role="assistant" ordinal="45" timestamp="2026-10-27T09:05:38.324Z" has_thinking="true" has_tool_use="true">
<thinking><![CDATA[
Each mount cycle does a lot of per-file overhead: for every one of the ~11.5k files and directories, removal involves holding the local directory chain, opening the file, verifying the handle, changing ACLs to allow delete, deleting, and restoring ACLs on the deleted entry. Creation isn't batched either—placeholders get created one at a time with separate ACL operations—and then there are two full tree scans verifying ownership for every entry. That's a huge number of redundant filesystem operations per mount.

This adds up to over 100k file operations per mount, all of which Defender's minifilter intercepts, which explains both the mount slowness and Defender's CPU spikes. The rebuild also dehydrates previously hydrated files since placeholders get recreated from scratch, forcing re-reads from cache even though the underlying cached bytes survive. The fix should be to reconcile unchanged placeholders instead of deleting and recreating them, batch the placeholder creation calls, and skip redundant ACL writes when permissions are already inherited.

Now I'm shifting to the state management mess—UiState has around 110 flat fields and apply_event runs about 680 lines, with different UI surfaces computing status from inconsistent fields (like the RetroBat card and metadata progress indicators showing conflicting messages). I need to trace through how the metadata progress component actually derives its displayed state.
]]></thinking>
<tool_call id="toolu_01QNVdRZtJ5YmSa94WyJufSZ" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"rg -n 'fn metadata_progress' -A45 crates/rommfs-app/src/window/","description":"Read metadata progress component"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ rg -n 'fn metadata_progress' -A45 crates/rommfs-app/src/window/
]]></tool_body>
<tool_result><![CDATA[
crates/rommfs-app/src/window/components.rs:472:pub(super) fn metadata_progress(id: &'static str, state: &UiState, cx: &App) -> impl IntoElement {
crates/rommfs-app/src/window/components.rs-473-    match state.frontend_metadata_summary() {
crates/rommfs-app/src/window/components.rs-474-        FrontendMetadataSummary::CheckingArtwork => {
crates/rommfs-app/src/window/components.rs-475-            let (completed, total) = state.frontend_metadata_progress.unwrap_or((0, 0));
crates/rommfs-app/src/window/components.rs-476-            operation_progress(
crates/rommfs-app/src/window/components.rs-477-                id,
crates/rommfs-app/src/window/components.rs-478-                "Checking artwork",
crates/rommfs-app/src/window/components.rs-479-                completed as u64,
crates/rommfs-app/src/window/components.rs-480-                (total > 0).then_some(total as u64),
crates/rommfs-app/src/window/components.rs-481-                0,
crates/rommfs-app/src/window/components.rs-482-                cx,
crates/rommfs-app/src/window/components.rs-483-            ).into_any_element()
crates/rommfs-app/src/window/components.rs-484-        }
crates/rommfs-app/src/window/components.rs-485-        FrontendMetadataSummary::Publishing => v_flex()
crates/rommfs-app/src/window/components.rs-486-            .gap_2()
crates/rommfs-app/src/window/components.rs-487-            .child(muted("Artwork is ready. Adding artwork and game information to RetroBat…", cx))
crates/rommfs-app/src/window/components.rs-488-            .child(Progress::new(id).accessibility_label("Updating RetroBat game information").loading(true).small())
crates/rommfs-app/src/window/components.rs-489-            .into_any_element(),
crates/rommfs-app/src/window/components.rs-490-        FrontendMetadataSummary::Waiting => muted(
crates/rommfs-app/src/window/components.rs-491-            "Game information is waiting for access to RetroBat files. RomMFS will retry automatically.",
crates/rommfs-app/src/window/components.rs-492-            cx,
crates/rommfs-app/src/window/components.rs-493-        ).into_any_element(),
crates/rommfs-app/src/window/components.rs-494-        _ => div().into_any_element(),
crates/rommfs-app/src/window/components.rs-495-    }
crates/rommfs-app/src/window/components.rs-496-}
crates/rommfs-app/src/window/components.rs-497-
crates/rommfs-app/src/window/components.rs-498-pub(super) fn section_heading(text: &'static str) -> impl IntoElement {
crates/rommfs-app/src/window/components.rs-499-    div()
crates/rommfs-app/src/window/components.rs-500-        .text_base()
crates/rommfs-app/src/window/components.rs-501-        .font_weight(FontWeight::SEMIBOLD)
crates/rommfs-app/src/window/components.rs-502-        .child(text)
crates/rommfs-app/src/window/components.rs-503-}
crates/rommfs-app/src/window/components.rs-504-
crates/rommfs-app/src/window/components.rs-505-/// One row shape shared by Overview and Settings: a heading, a status dot and
crates/rommfs-app/src/window/components.rs-506-/// phrase, and an optional trailing action (phase 2d consistency pass).
crates/rommfs-app/src/window/components.rs-507-pub(super) fn status_row(
crates/rommfs-app/src/window/components.rs-508-    label: &'static str,
crates/rommfs-app/src/window/components.rs-509-    kind: StatusKind,
crates/rommfs-app/src/window/components.rs-510-    status: impl Into<SharedString>,
crates/rommfs-app/src/window/components.rs-511-    action: Option<Button>,
crates/rommfs-app/src/window/components.rs-512-    cx: &App,
crates/rommfs-app/src/window/components.rs-513-) -> impl IntoElement {
crates/rommfs-app/src/window/components.rs-514-    h_flex()
crates/rommfs-app/src/window/components.rs-515-        .flex_wrap()
crates/rommfs-app/src/window/components.rs-516-        .justify_between()
crates/rommfs-app/src/window/components.rs-517-        .gap_3()
{"content_type":"terminal","tokens_before":1023,"tokens_after":1023,"token_count_basis":"o200k_base","ratio":0,"basis":"inferred","error":"cave_ccr_budget_exceeded"}
]]></tool_result>
<tool_result tool_call_id="toolu_01QNVdRZtJ5YmSa94WyJufSZ" source="tool_result" timestamp="2026-10-27T09:05:42.007Z"><![CDATA[
crates/rommfs-app/src/window/components.rs:472:pub(super) fn metadata_progress(id: &'static str, state: &UiState, cx: &App) -> impl IntoElement {
crates/rommfs-app/src/window/components.rs-473-    match state.frontend_metadata_summary() {
crates/rommfs-app/src/window/components.rs-474-        FrontendMetadataSummary::CheckingArtwork => {
crates/rommfs-app/src/window/components.rs-475-            let (completed, total) = state.frontend_metadata_progress.unwrap_or((0, 0));
crates/rommfs-app/src/window/components.rs-476-            operation_progress(
crates/rommfs-app/src/window/components.rs-477-                id,
crates/rommfs-app/src/window/components.rs-478-                "Checking artwork",
crates/rommfs-app/src/window/components.rs-479-                completed as u64,
crates/rommfs-app/src/window/components.rs-480-                (total > 0).then_some(total as u64),
crates/rommfs-app/src/window/components.rs-481-                0,
crates/rommfs-app/src/window/components.rs-482-                cx,
crates/rommfs-app/src/window/components.rs-483-            ).into_any_element()
crates/rommfs-app/src/window/components.rs-484-        }
crates/rommfs-app/src/window/components.rs-485-        FrontendMetadataSummary::Publishing => v_flex()
crates/rommfs-app/src/window/components.rs-486-            .gap_2()
crates/rommfs-app/src/window/components.rs-487-            .child(muted("Artwork is ready. Adding artwork and game information to RetroBat…", cx))
crates/rommfs-app/src/window/components.rs-488-            .child(Progress::new(id).accessibility_label("Updating RetroBat game information").loading(true).small())
crates/rommfs-app/src/window/components.rs-489-            .into_any_element(),
crates/rommfs-app/src/window/components.rs-490-        FrontendMetadataSummary::Waiting => muted(
crates/rommfs-app/src/window/components.rs-491-            "Game information is waiting for access to RetroBat files. RomMFS will retry automatically.",
crates/rommfs-app/src/window/components.rs-492-            cx,
crates/rommfs-app/src/window/components.rs-493-        ).into_any_element(),
crates/rommfs-app/src/window/components.rs-494-        _ => div().into_any_element(),
crates/rommfs-app/src/window/components.rs-495-    }
crates/rommfs-app/src/window/components.rs-496-}
crates/rommfs-app/src/window/components.rs-497-
crates/rommfs-app/src/window/components.rs-498-pub(super) fn section_heading(text: &'static str) -> impl IntoElement {
crates/rommfs-app/src/window/components.rs-499-    div()
crates/rommfs-app/src/window/components.rs-500-        .text_base()
crates/rommfs-app/src/window/components.rs-501-        .font_weight(FontWeight::SEMIBOLD)
crates/rommfs-app/src/window/components.rs-502-        .child(text)
crates/rommfs-app/src/window/components.rs-503-}
crates/rommfs-app/src/window/components.rs-504-
crates/rommfs-app/src/window/components.rs-505-/// One row shape shared by Overview and Settings: a heading, a status dot and
crates/rommfs-app/src/window/components.rs-506-/// phrase, and an optional trailing action (phase 2d consistency pass).
crates/rommfs-app/src/window/components.rs-507-pub(super) fn status_row(
crates/rommfs-app/src/window/components.rs-508-    label: &'static str,
crates/rommfs-app/src/window/components.rs-509-    kind: StatusKind,
crates/rommfs-app/src/window/components.rs-510-    status: impl Into<SharedString>,
crates/rommfs-app/src/window/components.rs-511-    action: Option<Button>,
crates/rommfs-app/src/window/components.rs-512-    cx: &App,
crates/rommfs-app/src/window/components.rs-513-) -> impl IntoElement {
crates/rommfs-app/src/window/components.rs-514-    h_flex()
crates/rommfs-app/src/window/components.rs-515-        .flex_wrap()
crates/rommfs-app/src/window/components.rs-516-        .justify_between()
crates/rommfs-app/src/window/components.rs-517-        .gap_3()
{"content_type":"terminal","tokens_before":1023,"tokens_after":1023,"token_count_basis":"o200k_base","ratio":0,"basis":"inferred","error":"cave_ccr_budget_exceeded"}
]]></tool_result>
<tool_call id="toolu_01NqtaLLWjj83qa2sue4n5xx" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"rg -n 'save_sync_pill|folder_summary\\(\\)|frontend_metadata_summary\\(\\)|save_sync_summary\\(\\)|retrobat_setup_ok' crates/rommfs-app/src/window crates/rommfs-app/src/desktop* | awk -F: '{print $1}' | sort | uniq -c","description":"Count where status selectors are used per file"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ rg -n 'save_sync_pill|folder_summary\(\)|frontend_metadata_summary\(\)|save_sync_summary\(\)|retrobat_setup_ok' crates/rommfs-app/src/window crates/rommfs-app/src/desktop* | awk -F: '{print $1}' | sort | uniq -c
]]></tool_body>
<tool_result><![CDATA[
      1 crates/rommfs-app/src/window/components.rs
      3 crates/rommfs-app/src/window/folder_controls.rs
      5 crates/rommfs-app/src/window/pages/overview.rs
      1 crates/rommfs-app/src/window/pages/save_review.rs
]]></tool_result>
<tool_result tool_call_id="toolu_01NqtaLLWjj83qa2sue4n5xx" source="tool_result" timestamp="2026-10-27T09:05:44.552Z"><![CDATA[
      1 crates/rommfs-app/src/window/components.rs
      3 crates/rommfs-app/src/window/folder_controls.rs
      5 crates/rommfs-app/src/window/pages/overview.rs
      1 crates/rommfs-app/src/window/pages/save_review.rs
]]></tool_result>
</message>
<message role="assistant" ordinal="48" timestamp="2026-10-27T09:05:48.418Z" has_thinking="true" has_tool_use="true">
<thinking><![CDATA[
Let me check whether pages are reading raw state fields directly instead of going through the selectors, by scanning for direct accesses to things like the retrobat setup flag or mount state.
]]></thinking>
<tool_call id="toolu_01EDdPKSamTHUgYZbrtAoT9r" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"rg -o 'state\\.[a-z_]+' crates/rommfs-app/src/window crates/rommfs-app/src/desktop* | sed 's/.*state\\.//' | sort | uniq -c | sort -rn | head -40; echo; rg -o 'state\\.[a-z_]+' crates/rommfs-app/src/window | sed 's/.*state\\.//' | sort -u | wc -l","description":"Count raw UiState field reads from window code"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ rg -o 'state\.[a-z_]+' crates/rommfs-app/src/window crates/rommfs-app/src/desktop* | sed 's/.*state\.//' | sort | uniq -c | sort -rn | head -40; echo; rg -o 'state\.[a-z_]+' crates/rommfs-app/src/window | sed 's/.*state\.//' | sort -u | wc -l
]]></tool_body>
<tool_result><![CDATA[
     12 mount_stopping
     11 mount
     10 conn
      8 save_sync_enabled
      5 retrobat_setup_running
      5 retrobat_setup_ok
      5 catalogue_progress
      4 save_sync_checking
      4 save_sync_available
      4 foreign_entries
      4 conn_error
      4 configured_mount_root
      4 auxiliary_running
      3 retrobat_setup_error
      3 retrobat_root
      3 mount_progress
      3 mount_error
      3 memory_card_conflicts
      3 integration
      3 frontend_metadata_pending
      3 foreign_removal
      2 startup_error
      2 start_at_login
      2 save_sync_session_id
      2 save_sync_review_count
      2 save_sync_queue
      2 save_sync_games
      2 save_sync_effective_saves_root
      2 save_sync_attention
      2 save_files_ignored
      2 save_file_unmatched
      2 retrobat_setup_migration
      2 request_save_sync_enabled
      2 request_mount
      2 play_data_problem
      2 play_data_available
      2 log
      2 integration_error
      2 installer_running
      2 installer_ready

81
]]></tool_result>
<tool_result tool_call_id="toolu_01EDdPKSamTHUgYZbrtAoT9r" source="tool_result" timestamp="2026-10-27T09:05:49.467Z"><![CDATA[
     12 mount_stopping
     11 mount
     10 conn
      8 save_sync_enabled
      5 retrobat_setup_running
      5 retrobat_setup_ok
      5 catalogue_progress
      4 save_sync_checking
      4 save_sync_available
      4 foreign_entries
      4 conn_error
      4 configured_mount_root
      4 auxiliary_running
      3 retrobat_setup_error
      3 retrobat_root
      3 mount_progress
      3 mount_error
      3 memory_card_conflicts
      3 integration
      3 frontend_metadata_pending
      3 foreign_removal
      2 startup_error
      2 start_at_login
      2 save_sync_session_id
      2 save_sync_review_count
      2 save_sync_queue
      2 save_sync_games
      2 save_sync_effective_saves_root
      2 save_sync_attention
      2 save_files_ignored
      2 save_file_unmatched
      2 retrobat_setup_migration
      2 request_save_sync_enabled
      2 request_mount
      2 play_data_problem
      2 play_data_available
      2 log
      2 integration_error
      2 installer_running
      2 installer_ready

81
]]></tool_result>
</message>
<message role="assistant" ordinal="50" timestamp="2026-10-27T09:05:54.617Z" has_thinking="true" has_tool_use="true">
<thinking><![CDATA[
I'm seeing 81 distinct raw state fields in the window, with `mount_stopping` referenced 12 times since MountState doesn't include a Stopping value — each render site has to manually combine `mount` and `mount_stopping` itself, which is the root of the conflicting-state problem. I want to check folder_controls next to see how it handles the combinations around mount_stopping and catalogue_pending_remount.
]]></thinking>
I&#39;ve found the main causes of the slowness and the conflicting status. Next I&#39;m checking how the folder controls combine those flags, then I&#39;ll pull the plan together.
<tool_call id="toolu_011BrRFbAPTGTgVgsip372zb" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"rg -n 'mount_stopping|catalogue_pending_remount|mount_progress|catalogue_progress' crates/rommfs-app/src/window crates/rommfs-app/src/controller/state.rs | head -50","description":"Find mount flag combinations in UI and state"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ rg -n 'mount_stopping|catalogue_pending_remount|mount_progress|catalogue_progress' crates/rommfs-app/src/window crates/rommfs-app/src/controller/state.rs | head -50
]]></tool_body>
<tool_result><![CDATA[
crates/rommfs-app/src/controller/state.rs:259:    pub mount_stopping: bool,
crates/rommfs-app/src/controller/state.rs:272:    pub catalogue_progress: Option<(String, u64, Option<u64>, u64)>,
crates/rommfs-app/src/controller/state.rs:273:    pub mount_progress: Option<(String, u64, Option<u64>, u64)>,
crates/rommfs-app/src/controller/state.rs:275:    pub catalogue_pending_remount: bool,
crates/rommfs-app/src/controller/state.rs:362:            mount_stopping: false,
crates/rommfs-app/src/controller/state.rs:373:            catalogue_progress: None,
crates/rommfs-app/src/controller/state.rs:374:            mount_progress: None,
crates/rommfs-app/src/controller/state.rs:376:            catalogue_pending_remount: false,
crates/rommfs-app/src/controller/state.rs:650:                self.mount_stopping = false;
crates/rommfs-app/src/controller/state.rs:655:                self.catalogue_pending_remount = false;
crates/rommfs-app/src/controller/state.rs:656:                self.catalogue_progress = None;
crates/rommfs-app/src/controller/state.rs:666:                self.mount_progress = None;
crates/rommfs-app/src/controller/state.rs:696:                self.catalogue_progress = None;
crates/rommfs-app/src/controller/state.rs:698:                self.catalogue_pending_remount = false;
crates/rommfs-app/src/controller/state.rs:765:                    self.catalogue_progress =
crates/rommfs-app/src/controller/state.rs:775:                self.catalogue_pending_remount = *pending_remount;
crates/rommfs-app/src/controller/state.rs:783:                self.mount_progress = Some((stage.clone(), *completed, *total, *elapsed_ms));
crates/rommfs-app/src/controller/state.rs:795:                self.catalogue_progress = None;
crates/rommfs-app/src/controller/state.rs:803:                self.catalogue_progress = None;
crates/rommfs-app/src/controller/state.rs:815:                self.mount_stopping = false;
crates/rommfs-app/src/controller/state.rs:817:                self.catalogue_pending_remount = false;
crates/rommfs-app/src/controller/state.rs:818:                self.mount_progress = None;
crates/rommfs-app/src/controller/state.rs:826:            AppEvent::MountStopping => self.mount_stopping = true,
crates/rommfs-app/src/controller/state.rs:828:                self.mount_stopping = false;
crates/rommfs-app/src/window/actions.rs:37:        if self.state.mount != MountState::Mounted || self.state.mount_stopping {
crates/rommfs-app/src/window/actions.rs:40:        self.state.mount_stopping = true;
crates/rommfs-app/src/window/pages/setup.rs:83:                    .when(mounting, |this| match self.state.mount_progress.clone() {
crates/rommfs-app/src/window/pages/activity.rs:467:            let detail = match &state.mount_progress {
crates/rommfs-app/src/window/pages/activity.rs:503:    if state.mount_stopping {
crates/rommfs-app/src/window/pages/activity.rs:510:    if let Some((stage, completed, total, elapsed)) = &state.catalogue_progress {
crates/rommfs-app/src/window/pages/settings.rs:111:                if self.state.mount_stopping {
crates/rommfs-app/src/window/pages/settings.rs:125:                    .label(if self.state.mount_stopping {
crates/rommfs-app/src/window/pages/settings.rs:130:                    .disabled(self.state.mount_stopping || self.state.mount == MountState::Mounting)
crates/rommfs-app/src/window/pages/overview.rs:236:                        .when(self.state.mount == MountState::Mounted && !self.state.mount_stopping, |this| {
crates/rommfs-app/src/window/folder_controls.rs:280:                        .disabled(self.state.mount_stopping)
crates/rommfs-app/src/window/folder_controls.rs:311:                                .label(if self.state.mount_stopping { "Stopping…" } else { "Stop" })
crates/rommfs-app/src/window/folder_controls.rs:313:                                .disabled(self.state.mount_stopping)
crates/rommfs-app/src/window/folder_controls.rs:321:                                .label(if self.state.catalogue_progress.is_some() { "Checking library…" } else { "Check for library changes" })
crates/rommfs-app/src/window/folder_controls.rs:322:                                .disabled(self.state.catalogue_progress.is_some() || summary == FolderSummary::Starting || self.state.mount_stopping)
crates/rommfs-app/src/window/folder_controls.rs:325:                                    view.state.catalogue_progress = Some(("checking".into(), 0, None, 0));
crates/rommfs-app/src/window/folder_controls.rs:338:                .when_some(self.state.catalogue_progress.clone(), |this, (_, completed, total, elapsed)| {
crates/rommfs-app/src/window/folder_controls.rs:342:                    match self.state.mount_progress.clone() {
crates/rommfs-app/src/window/folder_controls.rs:347:                .when(self.state.catalogue_pending_remount, |this| {
crates/rommfs-app/src/window/folder_controls.rs:355:                            .disabled(self.state.mount_stopping || summary == FolderSummary::Starting)
]]></tool_result>
<tool_result tool_call_id="toolu_011BrRFbAPTGTgVgsip372zb" source="tool_result" timestamp="2026-10-27T09:05:55.715Z"><![CDATA[
crates/rommfs-app/src/controller/state.rs:259:    pub mount_stopping: bool,
crates/rommfs-app/src/controller/state.rs:272:    pub catalogue_progress: Option<(String, u64, Option<u64>, u64)>,
crates/rommfs-app/src/controller/state.rs:273:    pub mount_progress: Option<(String, u64, Option<u64>, u64)>,
crates/rommfs-app/src/controller/state.rs:275:    pub catalogue_pending_remount: bool,
crates/rommfs-app/src/controller/state.rs:362:            mount_stopping: false,
crates/rommfs-app/src/controller/state.rs:373:            catalogue_progress: None,
crates/rommfs-app/src/controller/state.rs:374:            mount_progress: None,
crates/rommfs-app/src/controller/state.rs:376:            catalogue_pending_remount: false,
crates/rommfs-app/src/controller/state.rs:650:                self.mount_stopping = false;
crates/rommfs-app/src/controller/state.rs:655:                self.catalogue_pending_remount = false;
crates/rommfs-app/src/controller/state.rs:656:                self.catalogue_progress = None;
crates/rommfs-app/src/controller/state.rs:666:                self.mount_progress = None;
crates/rommfs-app/src/controller/state.rs:696:                self.catalogue_progress = None;
crates/rommfs-app/src/controller/state.rs:698:                self.catalogue_pending_remount = false;
crates/rommfs-app/src/controller/state.rs:765:                    self.catalogue_progress =
crates/rommfs-app/src/controller/state.rs:775:                self.catalogue_pending_remount = *pending_remount;
crates/rommfs-app/src/controller/state.rs:783:                self.mount_progress = Some((stage.clone(), *completed, *total, *elapsed_ms));
crates/rommfs-app/src/controller/state.rs:795:                self.catalogue_progress = None;
crates/rommfs-app/src/controller/state.rs:803:                self.catalogue_progress = None;
crates/rommfs-app/src/controller/state.rs:815:                self.mount_stopping = false;
crates/rommfs-app/src/controller/state.rs:817:                self.catalogue_pending_remount = false;
crates/rommfs-app/src/controller/state.rs:818:                self.mount_progress = None;
crates/rommfs-app/src/controller/state.rs:826:            AppEvent::MountStopping => self.mount_stopping = true,
crates/rommfs-app/src/controller/state.rs:828:                self.mount_stopping = false;
crates/rommfs-app/src/window/actions.rs:37:        if self.state.mount != MountState::Mounted || self.state.mount_stopping {
crates/rommfs-app/src/window/actions.rs:40:        self.state.mount_stopping = true;
crates/rommfs-app/src/window/pages/setup.rs:83:                    .when(mounting, |this| match self.state.mount_progress.clone() {
crates/rommfs-app/src/window/pages/activity.rs:467:            let detail = match &state.mount_progress {
crates/rommfs-app/src/window/pages/activity.rs:503:    if state.mount_stopping {
crates/rommfs-app/src/window/pages/activity.rs:510:    if let Some((stage, completed, total, elapsed)) = &state.catalogue_progress {
crates/rommfs-app/src/window/pages/settings.rs:111:                if self.state.mount_stopping {
crates/rommfs-app/src/window/pages/settings.rs:125:                    .label(if self.state.mount_stopping {
crates/rommfs-app/src/window/pages/settings.rs:130:                    .disabled(self.state.mount_stopping || self.state.mount == MountState::Mounting)
crates/rommfs-app/src/window/pages/overview.rs:236:                        .when(self.state.mount == MountState::Mounted && !self.state.mount_stopping, |this| {
crates/rommfs-app/src/window/folder_controls.rs:280:                        .disabled(self.state.mount_stopping)
crates/rommfs-app/src/window/folder_controls.rs:311:                                .label(if self.state.mount_stopping { "Stopping…" } else { "Stop" })
crates/rommfs-app/src/window/folder_controls.rs:313:                                .disabled(self.state.mount_stopping)
crates/rommfs-app/src/window/folder_controls.rs:321:                                .label(if self.state.catalogue_progress.is_some() { "Checking library…" } else { "Check for library changes" })
crates/rommfs-app/src/window/folder_controls.rs:322:                                .disabled(self.state.catalogue_progress.is_some() || summary == FolderSummary::Starting || self.state.mount_stopping)
crates/rommfs-app/src/window/folder_controls.rs:325:                                    view.state.catalogue_progress = Some(("checking".into(), 0, None, 0));
crates/rommfs-app/src/window/folder_controls.rs:338:                .when_some(self.state.catalogue_progress.clone(), |this, (_, completed, total, elapsed)| {
crates/rommfs-app/src/window/folder_controls.rs:342:                    match self.state.mount_progress.clone() {
crates/rommfs-app/src/window/folder_controls.rs:347:                .when(self.state.catalogue_pending_remount, |this| {
crates/rommfs-app/src/window/folder_controls.rs:355:                            .disabled(self.state.mount_stopping || summary == FolderSummary::Starting)
]]></tool_result>
</message>
<message role="assistant" ordinal="52" timestamp="2026-10-27T09:06:02.622Z" has_thinking="true" has_tool_use="true">
<thinking><![CDATA[
I'm also noticing the window mutates state directly in places like setting mount_stopping or catalogue_progress—optimistic UI writes that bypass the event-derived state and risk conflicting with incoming events. There's also a gap where FolderSummary lacks a Stopping variant, so an "Available" pill could end up next to a "Stopping…" button, and I'm now wondering whether the window even owns its own copy of UiState.
]]></thinking>
<tool_call id="toolu_019VBrRKo5AD9cDqCwXscWZo" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"rg -n 'state: UiState|apply_event|fn drain|poll|Timer|timer\\(|from_millis' crates/rommfs-app/src/window.rs | head -20","description":"Check how window receives state updates"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ rg -n 'state: UiState|apply_event|fn drain|poll|Timer|timer\(|from_millis' crates/rommfs-app/src/window.rs | head -20
]]></tool_body>
<tool_result><![CDATA[
62:const POLL_INTERVAL: Duration = Duration::from_millis(200);
111:    state: UiState,
371:        // Worker events arrive on the channel; poll it on the UI executor —
375:                cx.background_executor().timer(POLL_INTERVAL).await;
391:            state: UiState::new(LOG_CAP),
431:    fn drain_events(&mut self, window: &mut Window, cx: &mut Context<Self>) {
515:            self.state.apply_event(&event);
1060:                                cx.background_executor().timer(POLL_INTERVAL).await;
]]></tool_result>
<tool_result tool_call_id="toolu_019VBrRKo5AD9cDqCwXscWZo" source="tool_result" timestamp="2026-10-27T09:06:03.399Z"><![CDATA[
62:const POLL_INTERVAL: Duration = Duration::from_millis(200);
111:    state: UiState,
371:        // Worker events arrive on the channel; poll it on the UI executor —
375:                cx.background_executor().timer(POLL_INTERVAL).await;
391:            state: UiState::new(LOG_CAP),
431:    fn drain_events(&mut self, window: &mut Window, cx: &mut Context<Self>) {
515:            self.state.apply_event(&event);
1060:                                cx.background_executor().timer(POLL_INTERVAL).await;
]]></tool_result>
</message>
<message role="assistant" ordinal="54" timestamp="2026-10-27T09:06:05.255Z" has_tool_use="true">
<tool_call id="toolu_01Y1KB3dQ5mQU6oAEYHHd3Qo" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"sed -n 365,390p crates/rommfs-app/src/window.rs; sed -n 431,530p crates/rommfs-app/src/window.rs","description":"Read window event polling and drain"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ sed -n 365,390p crates/rommfs-app/src/window.rs; sed -n 431,530p crates/rommfs-app/src/window.rs
]]></tool_body>
<tool_result><![CDATA[
                if matches!(event, InputEvent::PressEnter { .. }) {
                    view.apply_save_sync_debounce(cx);
                }
            },
        ));

        // Worker events arrive on the channel; poll it on the UI executor —
        // every applied event is a real fact, nothing fabricated (R5).
        cx.spawn_in(window, async move |this: WeakEntity<RommfsWindow>, cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;
                if this
                    .update_in(cx, |view, window, cx| view.drain_events(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        let view = Self {
            #[cfg(all(feature = "mcp", debug_assertions))]
            _mcp_bridge: mcp_bridge,
            controller,
            session,
    fn drain_events(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut new_logs = false;
        let mut save_sync_updated = false;
        let mut changed = false;
        let events = self.session.borrow_mut().drain();
        for event in events {
            if matches!(event, AppEvent::Log(_)) {
                new_logs = true;
            }
            match &event {
                AppEvent::SaveFileMappings {
                    session_id, games, ..
                } if *session_id == self.state.save_sync_session_id => {
                    let items = games
                        .iter()
                        .map(|(id, name)| SharedString::from(format!("{id} · {name}")))
                        .collect::<Vec<_>>();
                    self.save_game_select.update(cx, |state, cx| {
                        state.set_items(SearchableVec::from(items), window, cx)
                    });
                }
                AppEvent::SaveSyncUpdated { .. } | AppEvent::SaveSyncSessionChanged { .. } => {
                    save_sync_updated = true;
                }
                _ => {}
            }
            match &event {
                AppEvent::IntegrationUpdated { mode, .. } => {
                    self.integration_choice = usize::from(*mode == IntegrationMode::RetroBat);
                    save_sync_updated = true;
                }
                AppEvent::PairingReady { url, .. } => cx.open_url(url),
                AppEvent::Connected => self.active_view = 0,
                // SignedOut with no remembered account is the unconfigured
                // state — guided setup, not the full settings page.
                AppEvent::SignedOut => self.active_view = 4,
                // A remembered account that needs re-auth IS configured —
                // send it to Overview where the sign-in action lives
                // (existing config must never replay onboarding).
                AppEvent::SignInRequired { .. } => self.active_view = 0,
                // A remembered account whose server is temporarily down is
                // still configured — Overview, never guided setup again
                // (UI-PLAN §4).
                AppEvent::ConnectFailed { .. } if self.state.connected_server.is_some() => {
                    self.active_view = 0;
                }
                AppEvent::ConnectionRestored { server, mount_root } => {
                    self.url_input
                        .update(cx, |input, cx| input.set_value(server.clone(), window, cx));
                    if let Some(root) = mount_root {
                        self.mount_input
                            .update(cx, |input, cx| input.set_value(root.clone(), window, cx));
                    }
                }
                // An applied folder normalizes the field it was typed in.
                AppEvent::MountRootConfigured { mount_root, .. } => {
                    self.mount_input.update(cx, |input, cx| {
                        input.set_value(mount_root.clone(), window, cx)
                    });
                }
                AppEvent::SaveSyncSessionChanged { .. } => {
                    if self.debounce_input.read(cx).value() == self.debounce_input_applied {
                        let default = rommfs_core::save_sync::DEFAULT_DEBOUNCE_SECS.to_string();
                        self.debounce_input
                            .update(cx, |input, cx| input.set_value(default.clone(), window, cx));
                        self.debounce_input_applied = default;
                    }
                    self.debounce_error = None;
                }
                AppEvent::SaveSyncUpdated {
                    session_id,
                    debounce_secs,
                    ..
                } if *session_id == self.state.save_sync_session_id => {
                    let previous = self.debounce_input_applied.clone();
                    let next = debounce_secs.to_string();
                    if self.debounce_input.read(cx).value() == previous {
                        self.debounce_input
                            .update(cx, |input, cx| input.set_value(next.clone(), window, cx));
                    }
                    self.debounce_input_applied = next;
                }
                _ => {}
            }
            self.state.apply_event(&event);
            changed = true;
        }
        if save_sync_updated {
            self.sync_install_select(window, cx);
            self.sync_settings_install_select(window, cx);
        }
        if new_logs && self.activity_follow_log {
            let max = self.log_scroll.max_offset();
            if self.log_scroll.offset().y <= -max.y + px(2.) {
                self.log_scroll.scroll_to_bottom();
            } else {
                self.activity_follow_log = false;
            }
        }
        if changed {
]]></tool_result>
<tool_result tool_call_id="toolu_01Y1KB3dQ5mQU6oAEYHHd3Qo" source="tool_result" timestamp="2026-10-27T09:06:06.062Z"><![CDATA[
                if matches!(event, InputEvent::PressEnter { .. }) {
                    view.apply_save_sync_debounce(cx);
                }
            },
        ));

        // Worker events arrive on the channel; poll it on the UI executor —
        // every applied event is a real fact, nothing fabricated (R5).
        cx.spawn_in(window, async move |this: WeakEntity<RommfsWindow>, cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;
                if this
                    .update_in(cx, |view, window, cx| view.drain_events(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        let view = Self {
            #[cfg(all(feature = "mcp", debug_assertions))]
            _mcp_bridge: mcp_bridge,
            controller,
            session,
    fn drain_events(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut new_logs = false;
        let mut save_sync_updated = false;
        let mut changed = false;
        let events = self.session.borrow_mut().drain();
        for event in events {
            if matches!(event, AppEvent::Log(_)) {
                new_logs = true;
            }
            match &event {
                AppEvent::SaveFileMappings {
                    session_id, games, ..
                } if *session_id == self.state.save_sync_session_id => {
                    let items = games
                        .iter()
                        .map(|(id, name)| SharedString::from(format!("{id} · {name}")))
                        .collect::<Vec<_>>();
                    self.save_game_select.update(cx, |state, cx| {
                        state.set_items(SearchableVec::from(items), window, cx)
                    });
                }
                AppEvent::SaveSyncUpdated { .. } | AppEvent::SaveSyncSessionChanged { .. } => {
                    save_sync_updated = true;
                }
                _ => {}
            }
            match &event {
                AppEvent::IntegrationUpdated { mode, .. } => {
                    self.integration_choice = usize::from(*mode == IntegrationMode::RetroBat);
                    save_sync_updated = true;
                }
                AppEvent::PairingReady { url, .. } => cx.open_url(url),
                AppEvent::Connected => self.active_view = 0,
                // SignedOut with no remembered account is the unconfigured
                // state — guided setup, not the full settings page.
                AppEvent::SignedOut => self.active_view = 4,
                // A remembered account that needs re-auth IS configured —
                // send it to Overview where the sign-in action lives
                // (existing config must never replay onboarding).
                AppEvent::SignInRequired { .. } => self.active_view = 0,
                // A remembered account whose server is temporarily down is
                // still configured — Overview, never guided setup again
                // (UI-PLAN §4).
                AppEvent::ConnectFailed { .. } if self.state.connected_server.is_some() => {
                    self.active_view = 0;
                }
                AppEvent::ConnectionRestored { server, mount_root } => {
                    self.url_input
                        .update(cx, |input, cx| input.set_value(server.clone(), window, cx));
                    if let Some(root) = mount_root {
                        self.mount_input
                            .update(cx, |input, cx| input.set_value(root.clone(), window, cx));
                    }
                }
                // An applied folder normalizes the field it was typed in.
                AppEvent::MountRootConfigured { mount_root, .. } => {
                    self.mount_input.update(cx, |input, cx| {
                        input.set_value(mount_root.clone(), window, cx)
                    });
                }
                AppEvent::SaveSyncSessionChanged { .. } => {
                    if self.debounce_input.read(cx).value() == self.debounce_input_applied {
                        let default = rommfs_core::save_sync::DEFAULT_DEBOUNCE_SECS.to_string();
                        self.debounce_input
                            .update(cx, |input, cx| input.set_value(default.clone(), window, cx));
                        self.debounce_input_applied = default;
                    }
                    self.debounce_error = None;
                }
                AppEvent::SaveSyncUpdated {
                    session_id,
                    debounce_secs,
                    ..
                } if *session_id == self.state.save_sync_session_id => {
                    let previous = self.debounce_input_applied.clone();
                    let next = debounce_secs.to_string();
                    if self.debounce_input.read(cx).value() == previous {
                        self.debounce_input
                            .update(cx, |input, cx| input.set_value(next.clone(), window, cx));
                    }
                    self.debounce_input_applied = next;
                }
                _ => {}
            }
            self.state.apply_event(&event);
            changed = true;
        }
        if save_sync_updated {
            self.sync_install_select(window, cx);
            self.sync_settings_install_select(window, cx);
        }
        if new_logs && self.activity_follow_log {
            let max = self.log_scroll.max_offset();
            if self.log_scroll.offset().y <= -max.y + px(2.) {
                self.log_scroll.scroll_to_bottom();
            } else {
                self.activity_follow_log = false;
            }
        }
        if changed {
]]></tool_result>
</message>
<message role="assistant" ordinal="56" timestamp="2026-10-27T09:06:15.632Z" has_thinking="true" has_tool_use="true">
<thinking><![CDATA[
I notice events get polled every 200ms, and some events trigger jarring navigation side effects — like forcing the user back to Overview on reconnect or to a different view on sign-out, which would be disorienting if they're mid-task elsewhere. The polling interval adds some latency but isn't the main slowness culprit; meanwhile the save activity scan shows most of the 1,864 files as "not linked," which seems worth digging into further.

Looking at the full feature surface — pairing, mounting, RetroBat install/linking, auxiliary files, artwork sync, favourites, save sync, memory cards, tray/startup settings, Linux FUSE testing — it's a lot to stabilize in two days. The core loop the user actually cares about is just connect → pick folder → set up RetroBat → play, with saves being secondary, so I should freeze everything else and focus fixes there. I also want to check whether the `retrobat_setup_ok` flag gets re-verified at startup, since there's a reported bug where removing the RomM folder from RetroBat's roms directory doesn't get noticed by romm-fs.
]]></thinking>
<tool_call id="toolu_01QiQ4EK9wFgYYKGPMMpcYZu" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"rg -n 'retrobat_setup_ok =' crates/rommfs-app/src | head; rg -n 'RetroBatSetupFinished|RetroBatSetupStatus|RetroBatLinked' crates/rommfs-core/src/events.rs | head","description":"Find how RetroBat setup-ok flag is set"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ rg -n 'retrobat_setup_ok =' crates/rommfs-app/src | head; rg -n 'RetroBatSetupFinished|RetroBatSetupStatus|RetroBatLinked' crates/rommfs-core/src/events.rs | head
]]></tool_body>
<tool_result><![CDATA[
crates/rommfs-app/src/controller/state.rs:572:                    self.retrobat_setup_ok = false;
crates/rommfs-app/src/controller/state.rs:581:                    self.retrobat_setup_ok = true;
crates/rommfs-app/src/controller/state.rs:602:                self.retrobat_setup_ok = !*setup_migration
]]></tool_result>
<tool_result tool_call_id="toolu_01QiQ4EK9wFgYYKGPMMpcYZu" source="tool_result" timestamp="2026-10-27T09:06:16.432Z"><![CDATA[
crates/rommfs-app/src/controller/state.rs:572:                    self.retrobat_setup_ok = false;
crates/rommfs-app/src/controller/state.rs:581:                    self.retrobat_setup_ok = true;
crates/rommfs-app/src/controller/state.rs:602:                self.retrobat_setup_ok = !*setup_migration
]]></tool_result>
</message>
<message role="assistant" ordinal="58" timestamp="2026-10-27T09:06:18.753Z" has_tool_use="true">
<tool_call id="toolu_01GLNBHUd5hc28XDMgY3qgQE" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"sed -n 560,610p crates/rommfs-app/src/controller/state.rs; rg -n 'setup_complete|setup_migration' crates/rommfs-app/src/controller/retrobat_setup.rs crates/rommfs-app/src/controller/connection.rs | head -20","description":"Read RetroBat setup state transitions"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ sed -n 560,610p crates/rommfs-app/src/controller/state.rs; rg -n 'setup_complete|setup_migration' crates/rommfs-app/src/controller/retrobat_setup.rs crates/rommfs-app/src/controller/connection.rs | head -20
]]></tool_body>
<tool_result><![CDATA[
            }
            AppEvent::RetroBatSetup {
                running,
                detail,
                skipped,
                error,
            } => {
                self.retrobat_setup_running = *running;
                self.retrobat_setup_detail = Some(detail.clone());
                self.retrobat_setup_error = error.clone();
                self.retrobat_setup_skipped = skipped.clone();
                if *running || error.is_some() {
                    self.retrobat_setup_ok = false;
                }
                if *running {
                    // A new setup attempt supersedes earlier messages.
                    self.installer_error = None;
                    self.foreign_root = None;
                    self.foreign_entries.clear();
                    self.foreign_removal = None;
                } else if error.is_none() {
                    self.retrobat_setup_ok = true;
                    self.integration_notice = None;
                }
            }
            AppEvent::IntegrationUpdated {
                mode,
                retrobat_root,
                setup_complete,
                setup_migration,
                error,
            } => {
                let changed = self.integration != *mode;
                let root_changed = self.retrobat_root != *retrobat_root;
                self.integration = *mode;
                self.retrobat_root = retrobat_root.clone();
                self.retrobat_setup_migration = *setup_migration;
                if error.is_none() && retrobat_root.is_some() {
                    // A usable installation is selected: the installer's
                    // download/launch problems no longer apply.
                    self.installer_error = None;
                }
                self.retrobat_setup_ok = !*setup_migration
                    && *mode == IntegrationMode::RetroBat
                    && retrobat_root.is_some()
                    && (*setup_complete || (!root_changed && self.retrobat_setup_ok));
                if root_changed {
                    // Results belong to the installation that produced them.
                    self.retrobat_setup_error = None;
                    self.retrobat_setup_detail = None;
                    self.auxiliary_error = None;
crates/rommfs-app/src/controller/retrobat_setup.rs:7:        let setup_complete = preferences.is_some_and(|p| {
crates/rommfs-app/src/controller/retrobat_setup.rs:24:        let setup_migration = preferences
crates/rommfs-app/src/controller/retrobat_setup.rs:29:            setup_complete,
crates/rommfs-app/src/controller/retrobat_setup.rs:30:            setup_migration,
]]></tool_result>
<tool_result tool_call_id="toolu_01GLNBHUd5hc28XDMgY3qgQE" source="tool_result" timestamp="2026-10-27T09:06:19.547Z"><![CDATA[
            }
            AppEvent::RetroBatSetup {
                running,
                detail,
                skipped,
                error,
            } => {
                self.retrobat_setup_running = *running;
                self.retrobat_setup_detail = Some(detail.clone());
                self.retrobat_setup_error = error.clone();
                self.retrobat_setup_skipped = skipped.clone();
                if *running || error.is_some() {
                    self.retrobat_setup_ok = false;
                }
                if *running {
                    // A new setup attempt supersedes earlier messages.
                    self.installer_error = None;
                    self.foreign_root = None;
                    self.foreign_entries.clear();
                    self.foreign_removal = None;
                } else if error.is_none() {
                    self.retrobat_setup_ok = true;
                    self.integration_notice = None;
                }
            }
            AppEvent::IntegrationUpdated {
                mode,
                retrobat_root,
                setup_complete,
                setup_migration,
                error,
            } => {
                let changed = self.integration != *mode;
                let root_changed = self.retrobat_root != *retrobat_root;
                self.integration = *mode;
                self.retrobat_root = retrobat_root.clone();
                self.retrobat_setup_migration = *setup_migration;
                if error.is_none() && retrobat_root.is_some() {
                    // A usable installation is selected: the installer's
                    // download/launch problems no longer apply.
                    self.installer_error = None;
                }
                self.retrobat_setup_ok = !*setup_migration
                    && *mode == IntegrationMode::RetroBat
                    && retrobat_root.is_some()
                    && (*setup_complete || (!root_changed && self.retrobat_setup_ok));
                if root_changed {
                    // Results belong to the installation that produced them.
                    self.retrobat_setup_error = None;
                    self.retrobat_setup_detail = None;
                    self.auxiliary_error = None;
crates/rommfs-app/src/controller/retrobat_setup.rs:7:        let setup_complete = preferences.is_some_and(|p| {
crates/rommfs-app/src/controller/retrobat_setup.rs:24:        let setup_migration = preferences
crates/rommfs-app/src/controller/retrobat_setup.rs:29:            setup_complete,
crates/rommfs-app/src/controller/retrobat_setup.rs:30:            setup_migration,
]]></tool_result>
</message>
<message role="assistant" ordinal="60" timestamp="2026-10-27T09:06:21.246Z" has_tool_use="true">
<tool_call id="toolu_01WKz1qFxoGHa572d8yvh2TV" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"sed -n 1,40p crates/rommfs-app/src/controller/retrobat_setup.rs","description":"Read RetroBat setup-complete derivation"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ sed -n 1,40p crates/rommfs-app/src/controller/retrobat_setup.rs
]]></tool_body>
<tool_result><![CDATA[
//! RetroBat integration setup and its metadata publication lifecycle.
use super::*;

impl Worker {
    pub(super) fn emit_integration(&self, error: Option<String>) {
        let preferences = self.preferences.as_ref();
        let setup_complete = preferences.is_some_and(|p| {
            match (
                p.integration == IntegrationMode::RetroBat,
                p.retrobat_root.as_deref(),
                p.mount_root.as_deref(),
            ) {
                (true, Some(install), Some(managed)) => rommfs_core::retrobat::verify_setup_link(
                    install,
                    managed,
                    &server_id_of(&p.server),
                )
                .is_ok(),
                _ => false,
            }
        });
        // Only the RetroBat layout needs a migration; a leftover root from a
        // previous selection must not raise the prompt.
        let setup_migration = preferences
            .filter(|p| p.integration == IntegrationMode::RetroBat)
            .and_then(|p| p.retrobat_root.as_deref())
            .is_some_and(rommfs_core::retrobat::setup_needs_migration);
        self.sink.emit(AppEvent::IntegrationUpdated {
            setup_complete,
            setup_migration,
            mode: preferences.map(|p| p.integration).unwrap_or_default(),
            retrobat_root: preferences
                .and_then(|p| p.retrobat_root.as_ref())
                .map(|p| p.display().to_string()),
            error,
        });
    }

    pub(super) fn metadata_status(
        &self,
]]></tool_result>
<tool_result tool_call_id="toolu_01WKz1qFxoGHa572d8yvh2TV" source="tool_result" timestamp="2026-10-27T09:06:22.093Z"><![CDATA[
//! RetroBat integration setup and its metadata publication lifecycle.
use super::*;

impl Worker {
    pub(super) fn emit_integration(&self, error: Option<String>) {
        let preferences = self.preferences.as_ref();
        let setup_complete = preferences.is_some_and(|p| {
            match (
                p.integration == IntegrationMode::RetroBat,
                p.retrobat_root.as_deref(),
                p.mount_root.as_deref(),
            ) {
                (true, Some(install), Some(managed)) => rommfs_core::retrobat::verify_setup_link(
                    install,
                    managed,
                    &server_id_of(&p.server),
                )
                .is_ok(),
                _ => false,
            }
        });
        // Only the RetroBat layout needs a migration; a leftover root from a
        // previous selection must not raise the prompt.
        let setup_migration = preferences
            .filter(|p| p.integration == IntegrationMode::RetroBat)
            .and_then(|p| p.retrobat_root.as_deref())
            .is_some_and(rommfs_core::retrobat::setup_needs_migration);
        self.sink.emit(AppEvent::IntegrationUpdated {
            setup_complete,
            setup_migration,
            mode: preferences.map(|p| p.integration).unwrap_or_default(),
            retrobat_root: preferences
                .and_then(|p| p.retrobat_root.as_ref())
                .map(|p| p.display().to_string()),
            error,
        });
    }

    pub(super) fn metadata_status(
        &self,
]]></tool_result>
</message>
<message role="assistant" ordinal="62" timestamp="2026-10-27T09:06:26.611Z" has_thinking="true" has_tool_use="true">
<thinking><![CDATA[
I'm noting that setup_complete is only checked at emit_integration time, and the retrobat_setup_ok flag stays sticky once true unless the root changes, so there's no periodic re-verification—removing the link wouldn't get noticed this way. Let me check the test output file to confirm.
]]></thinking>
Nearly done reading. I&#39;m waiting on the test baseline, then I&#39;ll write up the plan.
<tool_call id="toolu_019jeotPhSh78N1dRFA2vFDP" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"cat /tmp/claude-1000/-home-aaron-Code-romm-fs/9d37bbd8-bbbc-4344-ab8a-4a7faabe6c42/tasks/ba6v4ztu3.output; cd /home/aaron/Code/romm-fs && rg -n 'Settings tabs|tab' crates/rommfs-app/src/window/pages/settings.rs | head -8; rg -c 'Technical details|details_disclosure' crates/rommfs-app/src/window -g '*.rs' | head","description":"Check test output and technical-details usage"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ cat /tmp/claude-1000/-home-aaron-Code-romm-fs/9d37bbd8-bbbc-4344-ab8a-4a7faabe6c42/tasks/ba6v4ztu3.output; cd /home/aaron/Code/romm-fs && rg -n 'Settings tabs|tab' crates/rommfs-app/src/window/pages/settings.rs | head -8; rg -c 'Technical details|details_disclosure' crates/rommfs-app/src/window -g '*.rs' | head
]]></tool_body>
<tool_result><![CDATA[
5:/// The Settings tabs (phase 2c). The selected index lives in view state.
14:/// Settings page: a tab bar with one scroll region per tab (phase 2c).
24:    /// Settings owns its own tab bar and per-tab scrolling, so the window
27:        let tab = self.settings_tab.min(SETTINGS_TABS.len() - 1);
28:        let handle = self.settings_tab_scrolls[tab].clone();
29:        let body: AnyElement = match tab {
30:            0 => self.render_account_tab(window, cx).into_any_element(),
31:            1 => self.render_games_folder_tab(window, cx).into_any_element(),
crates/rommfs-app/src/window/pages/activity.rs:2
crates/rommfs-app/src/window/pages/save_review.rs:5
crates/rommfs-app/src/window/pages/overview.rs:3
crates/rommfs-app/src/window/components.rs:5
]]></tool_result>
<tool_result tool_call_id="toolu_019jeotPhSh78N1dRFA2vFDP" source="tool_result" timestamp="2026-10-27T09:06:27.447Z"><![CDATA[
5:/// The Settings tabs (phase 2c). The selected index lives in view state.
14:/// Settings page: a tab bar with one scroll region per tab (phase 2c).
24:    /// Settings owns its own tab bar and per-tab scrolling, so the window
27:        let tab = self.settings_tab.min(SETTINGS_TABS.len() - 1);
28:        let handle = self.settings_tab_scrolls[tab].clone();
29:        let body: AnyElement = match tab {
30:            0 => self.render_account_tab(window, cx).into_any_element(),
31:            1 => self.render_games_folder_tab(window, cx).into_any_element(),
crates/rommfs-app/src/window/pages/activity.rs:2
crates/rommfs-app/src/window/pages/save_review.rs:5
crates/rommfs-app/src/window/pages/overview.rs:3
crates/rommfs-app/src/window/components.rs:5
]]></tool_result>
</message>
<message role="user" ordinal="64" timestamp="2026-10-27T09:06:29.59Z" is_system="true">
&lt;task-notification&gt;
&lt;task-id&gt;ba6v4ztu3&lt;/task-id&gt;
&lt;tool-use-id&gt;toolu_01DkQx1X9h2aTdEepFnPg58Y&lt;/tool-use-id&gt;
&lt;output-file&gt;/tmp/claude-1000/-home-aaron-Code-romm-fs/9d37bbd8-bbbc-4344-ab8a-4a7faabe6c42/tasks/ba6v4ztu3.output&lt;/output-file&gt;
&lt;status&gt;completed&lt;/status&gt;
&lt;summary&gt;Background command &#34;Run portable test suite for baseline&#34; completed (exit code 0)&lt;/summary&gt;
&lt;/task-notification&gt;
</message>
<message role="assistant" ordinal="65" timestamp="2026-10-27T09:06:30.634Z" has_thinking="true" has_tool_use="true">
<thinking><![CDATA[
The tests are still running, so I should wait on the background process using the appropriate tool, which I need to look up first.
]]></thinking>
<tool_call id="toolu_01KcJHdCNdpYVBYxsPNp7KHH" name="ToolSearch" category="Other">
<arguments><![CDATA[
{"query":"select:mcp__t3-code__t3_background_wait","max_results":1}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
max_results: 1
query: select:mcp__t3-code__t3_background_wait
]]></tool_body>
<tool_result tool_call_id="toolu_01KcJHdCNdpYVBYxsPNp7KHH" source="tool_result" timestamp="2026-10-27T09:06:30.95Z"><![CDATA[

]]></tool_result>
</message>
<message role="assistant" ordinal="67" timestamp="2026-10-27T09:06:34.894Z" has_tool_use="true">
<tool_call id="toolu_01JJpLd2tk9Krp5a7MysZMim" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"cat /tmp/claude-1000/-home-aaron-Code-romm-fs/9d37bbd8-bbbc-4344-ab8a-4a7faabe6c42/tasks/ba6v4ztu3.output | awk '/^test result/ {split($0,a,\" \"); p+=a[4]; f+=a[6]; i+=a[8]} END {print \"passed\",p,\"failed\",f,\"ignored\",i}'; grep -E 'FAILED|panicked' /tmp/claude-1000/-home-aaron-Code-romm-fs/9d37bbd8-bbbc-4344-ab8a-4a7faabe6c42/tasks/ba6v4ztu3.output | head","description":"Sum test results from baseline run"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ cat /tmp/claude-1000/-home-aaron-Code-romm-fs/9d37bbd8-bbbc-4344-ab8a-4a7faabe6c42/tasks/ba6v4ztu3.output | awk '/^test result/ {split($0,a," "); p+=a[4]; f+=a[6]; i+=a[8]} END {print "passed",p,"failed",f,"ignored",i}'; grep -E 'FAILED|panicked' /tmp/claude-1000/-home-aaron-Code-romm-fs/9d37bbd8-bbbc-4344-ab8a-4a7faabe6c42/tasks/ba6v4ztu3.output | head
]]></tool_body>
<tool_result><![CDATA[
passed 385 failed 0 ignored 0
]]></tool_result>
<tool_result tool_call_id="toolu_01JJpLd2tk9Krp5a7MysZMim" source="tool_result" timestamp="2026-10-27T09:06:35.79Z"><![CDATA[
passed 385 failed 0 ignored 0
]]></tool_result>
</message>
<message role="assistant" ordinal="69" timestamp="2026-10-27T09:07:13.431Z" has_thinking="true" has_tool_use="true">
<thinking><![CDATA[
The baseline run shows 385 passed and 0 failed, with no changes made yet. I'm diagnosing why the process feels slow and frustrating to use: every start/apply cycle deletes and rebuilds all roughly 11,500 placeholder files and directories, each requiring multiple opens, ACL changes, and verification passes, plus redundant full verification runs.

Each file operation is also getting filtered through Defender, consuming extra CPU, and placeholder creation happens sequentially rather than in batches — though I should flag that this diagnosis comes from reading the code rather than measuring on actual Windows hardware. I'm also tracing the confusing "exclusive access to retrobat.exe" error: the guard logic opens retrobat.exe and four emulator executables with exclusive read+write access, so any competing handle from Defender, Explorer's thumbnailer, or the search indexer causes it to fail, and these write handles likely trigger additional Defender rescans too. The fix would be switching to a less intrusive check, like enumerating running processes by image path instead of opening exclusive handles to the executables.

Looking at the broader architectural issues now: the gamelist writes require ES to not be running since it overwrites gamelists on exit, so a process check plus the existing retry logic should suffice there, though I'll note the tradeoff of losing the launch-blocking behavior during operations. I'm also noticing that UiState is a sprawling flat structure of around 110 fields with a single 680-line apply_event function, and the window reads 81 raw fields directly while also writing some optimistically — this creates conflicting state where booleans like mount_stopping can disagree with the main state enum, retrobat_setup_ok goes stale after startup verification, and artwork progress gets collapsed into a single "complete" signal even when still running. On top of that, events are forcing navigation transitions that yank the user around unexpectedly.

Beyond the state issues, the UI itself is overwhelming: the save list shows 1,864 rows where 1,857 aren't linked, there are six "Remote version N" entries each dumping technical details, and the Overview constantly warns about thousands of hidden items. Given the two-day timeframe and the stated goals of RomM feature parity while never touching user ROMs, I think the right move is to recommend hiding or freezing features rather than deleting anything, and to propose a definition of "finished" for the user to confirm rather than making that scope call myself. I'll lay out a two-day plan, starting with day one's focus on making the core loop fast and truthful — beginning with eliminating folder rebuilding on every startup in favor of reconciliation.

Instead of wiping and recreating everything, I want to diff the manifest against existing placeholders and only touch what changed, batching the creation calls and skipping unnecessary ACL churn — keeping the read-only ACL invariant intact but checking whether inherited permissions from the protected root already satisfy it. Reconciliation should make a second startup nearly instant since unchanged entries skip the delete-create-ACL cycle entirely, and I want to measure this with existing timing logs before and after, then also replace the exclusive-exe lock with a check for the running process instead.

Then there's the state.rs refactor — collapsing folder, RetroBat link, and artwork status into clean enums, routing writes through request_* methods instead of direct window-side mutation, and trimming event-driven navigation down to just setup→overview. That's a sizable day-one task focused on three surfaces, with day two spent simplifying the UI: defaulting the Saves page filter to tracked/needs-review, hiding unlinked items and remote version clutter behind filters, consolidating technical details into one Activity log location, and feature-freezing lower-priority items like memory cards, auxiliary files, the RetroBat installer, play data, and the legacy GB profile.

I decide not to hide the frozen features from the code but to avoid advertising or building new UI for them, marking them experimental where needed, then closing with a Windows acceptance run using the test wizard before release. I define "finished" as: sign in, pick a folder, connect RetroBat, browse with artwork, play, and have saves sync reliably with one truthful status per feature — everything else pushed past 1.0, including the RomM parity goal which conflicts with the two-day timeline. The real risk is that the reconcile change touches CFAPI ownership/deletion logic, an invariant-heavy area that can only be verified on an actual Windows machine, so I'll need to account for that machine time as the biggest scheduling constraint.

I'm also noting reassuring context: all 385 portable tests pass, so the code itself is healthy — the real problem is product shape, not broken logic, meaning the user is over-built and under-simplified rather than far from done. The core complaint, even if the user can't name it, is that the app forces them to manage internal sync-engine states instead of just answering "can I play?" — five status cards with inconsistent vocabulary, buttons that keep changing roles, required restarts, and warnings like "4,563 items not shown" that give no actionable next step. I'm keeping this diagnosis tight and now shaping the final plan into a moderate-length response (roughly 500-700 words) with headers, short bullets, and file references.

I'm pinning down the exact line number for the "ponytail" comment in windows.rs, counting from the `mount` function definition to locate it precisely for citation.
]]></thinking>
<tool_call id="toolu_01GTi59vxv55Z7Q9sJrAfPnd" name="Bash" category="Bash">
<arguments><![CDATA[
{"command":"rg -n 'ponytail|for \\(index, entry\\) in mount.manifest' crates/rommfs-cfapi/src/windows.rs; rg -n 'fn stopped_retrobat_guard' crates/rommfs-app/src/controller.rs; rg -n 'pub fn apply_event|pub struct UiState' crates/rommfs-app/src/controller/state.rs; rg -n 'AppEvent::Connected => self.active_view' crates/rommfs-app/src/window.rs; rg -n 'self.state.mount_stopping = true' crates/rommfs-app/src/window/actions.rs; rg -n 'view.state.catalogue_progress = ' crates/rommfs-app/src/window/folder_controls.rs","description":"Get line numbers for cited code locations"}
]]></arguments>
</tool_call>
<tool_body><![CDATA[
$ rg -n 'ponytail|for \(index, entry\) in mount.manifest' crates/rommfs-cfapi/src/windows.rs; rg -n 'fn stopped_retrobat_guard' crates/rommfs-app/src/controller.rs; rg -n 'pub fn apply_event|pub struct UiState' crates/rommfs-app/src/controller/state.rs; rg -n 'AppEvent::Connected => self.active_view' crates/rommfs-app/src/window.rs; rg -n 'self.state.mount_stopping = true' crates/rommfs-app/src/window/actions.rs; rg -n 'view.state.catalogue_progress = ' crates/rommfs-app/src/window/folder_controls.rs
]]></tool_body>
<tool_result><![CDATA[
440:        // ponytail: rebuilds owned placeholders on start; reconcile unchanged entries
500:        for (index, entry) in mount.manifest.entries.iter().enumerate() {
1073:pub(crate) fn stopped_retrobat_guard(installation: &Path) -> anyhow::Result<Vec<std::fs::File>> {
206:pub struct UiState {
418:    pub fn apply_event(&mut self, event: &AppEvent) {
463:                AppEvent::Connected => self.active_view = 0,
40:        self.state.mount_stopping = true;
325:                                    view.state.catalogue_progress = Some(("checking".into(), 0, None, 0));
]]></tool_result>
<tool_result tool_call_id="toolu_01GTi59vxv55Z7Q9sJrAfPnd" source="tool_result" timestamp="2026-10-27T09:07:14.346Z"><![CDATA[
440:        // ponytail: rebuilds owned placeholders on start; reconcile unchanged entries
500:        for (index, entry) in mount.manifest.entries.iter().enumerate() {
1073:pub(crate) fn stopped_retrobat_guard(installation: &Path) -> anyhow::Result<Vec<std::fs::File>> {
206:pub struct UiState {
418:    pub fn apply_event(&mut self, event: &AppEvent) {
463:                AppEvent::Connected => self.active_view = 0,
40:        self.state.mount_stopping = true;
325:                                    view.state.catalogue_progress = Some(("checking".into(), 0, None, 0));
]]></tool_result>
</message>
<message role="assistant" ordinal="71" timestamp="2026-10-27T09:07:35.396Z" has_thinking="true">
<thinking><![CDATA[
Now I should pull this together into a final answer: a short verdict, the three causes behind why it feels unfinished with evidence for each, a clear definition of "done," a concrete Day 1/Day 2 plan, the key risks, and the decision that needs to be made.
]]></thinking>
The project isn&#39;t broken; it&#39;s overbuilt and under-simplified. All 385 portable tests pass and the core features work. What you&#39;re reacting to is three specific problems, and you can fix all three in two days if you stop adding features now.

## Why it feels bad

**1. It&#39;s slow because every start rebuilds the whole games folder.**
Every time you start the folder or click &#34;Apply library changes&#34;, the code deletes and recreates every file stub in it, one at a time. With 11,531 files that&#39;s hundreds of thousands of file operations: checking each file, changing its permissions, deleting it, creating it again and setting permissions again. There are also two full checks of the folder before that. Windows Defender inspects every one of those operations, which fits what you saw with its CPU and RAM use. The code already flags this as a known shortcut at `crates/rommfs-cfapi/src/windows.rs:440`. I worked this out from reading the code; I haven&#39;t timed it on Windows.

**2. The &#34;exclusive access to retrobat.exe&#34; error isn&#39;t caused by RetroBat running.**
To check that RetroBat is closed, `stopped_retrobat_guard` (`crates/rommfs-app/src/controller.rs:1073`) opens `retrobat.exe` and four emulator programs for writing, with no sharing allowed. If anything else has one of those files open (Defender, the search indexer, Explorer), the check fails. Opening program files for writing also makes Defender scan them again.

**3. The conflicting status comes from how the app stores state.**
- **Too many separate flags:** `UiState` has about 110 separate fields, updated by one 680-line function (`controller/state.rs:418`). Most features are tracked as separate running / ok / error / detail flags that can contradict each other.
- **Pages assemble their own status:** the pages read 81 of those fields directly and combine them in their own way. For example, &#34;stopping&#34; is a separate flag checked in 12 places, so &#34;Available&#34; can show next to &#34;Stopping…&#34;.
- **The UI writes state itself:** some buttons set state directly before anything has happened (`window/actions.rs:40`, `window/folder_controls.rs:325`), and later events can disagree.
- **&#34;RetroBat set up&#34; is only checked at startup:** once it&#39;s marked done it stays done, which is why deleting the RomM folder from RetroBat went unnoticed.
- **The app moves you between pages on its own:** a reconnect sends you to Overview (`window.rs:463`).

**Why it&#39;s confusing:** the app shows how it works inside rather than answering &#34;can I play?&#34; Examples are 1,857 &#34;Not linked&#34; save rows, six &#34;Remote version&#34; entries each with its own technical details, and a &#34;4,563 items aren&#39;t shown&#34; warning with nothing you can do about it.

## Proposed definition of &#34;finished&#34;

You can sign in, pick a folder, connect RetroBat, start, browse with artwork, play, and have saves sync. Starting is fast, each feature shows one truthful status, and nothing tells you to look at technical details unless something actually failed.

Everything else stays as it is with no new UI work: memory cards, BIOS/firmware/cheats, the RetroBat installer, play history, and the old Game Boy save format. Your earlier goal of matching everything RomM supports becomes a goal for after 1.0.

## Day 1: fast and truthful

1. **Measure it.** The mount progress events already record elapsed time. Get start and apply timings on your Windows machine before and after.
2. **Stop rebuilding the folder.** Compare the new file list with the stubs already on disk, and only create or remove what changed. Batch the creation calls instead of making one per file. A restart with nothing changed should do almost no file work.
3. **Replace the RetroBat lock** with a check of running processes by their program path. The trade-off: RetroBat could then be launched mid-operation, but the existing retry already handles RetroBat files being busy.
4. **Collapse the status for the three features in your notes into one state each:**
   - **Games folder:** Stopped, Starting, Available, Stopping or Failed.
   - **RetroBat:** whether the link is checked again on every start and library refresh.
   - **Artwork:** one stage at a time, so it can&#39;t say &#34;complete&#34; and &#34;in progress&#34; together.

   Pages read only these. Button clicks go through `request_*` methods, as `request_mount` already does. Remove the automatic page switching except for setup going to Overview.

## Day 2: simplify, then test on Windows

5. **Saves page:** open on Tracked and Needs review. Put &#34;Not linked&#34; behind its filter. Collapse remote versions into one summary with an export option.
6. **Technical details:** show them inline only on a failure. Everything else goes to Activity → Technical log. Move the &#34;items not shown&#34; count into Library details.
7. **Windows test:** run the main journey with `test-wizard.cmd`, fix whatever breaks, then tag the release.

## Risks

- **Step 2 is the most important and the riskiest.** It touches the code that decides which files the app may delete, and it can only be proven on real Windows. Schedule Windows machine time on both days, not just at the end.
- **The process check in step 3** is a Windows-only change and needs the same real-machine test.
- **The Defender link is a strong guess, not a measurement.** Step 1 confirms or rules it out.
- **The artwork-from-a-RomM-URL idea** from your notes is a new feature. I&#39;d leave it until after release.

Before anyone starts, decide one thing: do you accept this definition of &#34;finished&#34;, including freezing the extra features? Everything above depends on that cut.
</message>
</session>
