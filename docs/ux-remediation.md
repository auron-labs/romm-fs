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
