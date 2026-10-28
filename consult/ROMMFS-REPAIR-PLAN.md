# RomMFS repair task

Implement this task in the existing checkout. The outcome is a usable Windows app: connect, configure RetroBat, start the games folder, play, sync supported saves, and reopen without repeating setup. Complete the repairs below; do not return another proposed plan.

## Scope and rules

- Start from the supplied source, logs, screenshots, and reported failures. Do not require Aaron to reproduce them, run a clean baseline, reset configuration, reinstall, or supply a new test session before implementation.
- Keep Rust, GPUI, CFAPI, the four existing pages, and existing features, including RetroBat, saves/save states, favourites, play history, and support files. Do not declare success by removing or disabling features.
- Make focused changes in the existing owners. No replacement architecture, generic state/retry/validation framework, new workflow system, speculative hardening, or unrelated cleanup. Existing roadmaps do not enlarge this task.
- Preserve checks that prevent actual data loss: ownership before deletion, save/path boundaries, atomic replacement, local-change detection, account/session isolation, and opt-in sync. Remove the particular excessive checks named below. Do not change Defender or system-wide permissions.
- The screenshots/logs predate some source fixes. Keep the existing catalogue cache/concurrent paging, eight artwork workers, throttled progress, aggregated warnings, and shared metadata summaries. Do not implement them again. If a listed repair is already complete in your checkout, verify that behavior and continue.

Paths below are repository-relative. `app` means `crates/rommfs-app/src`; `core` means `crates/rommfs-core/src`; `cfapi` means `crates/rommfs-cfapi/src`.

## 1. Remove the executable-lock blocker

**Targets:** `app/controller.rs::stopped_retrobat_guard`, its setup/metadata/support-file callers, and `app/controller/retrobat_setup.rs::start_metadata_publication`.

Remove exclusive writable handles on RetroBat/emulator executables. They establish file access, not whether the application is running. Where replacing a gamelist/config actually requires its application to be closed, use one read-only process check scoped to that application in the selected installation. Do not prevent application launch or lock every emulator.

Defer only the affected write; games and artwork downloading continue. Replace the 100 ms retry loop with one pending timer that rechecks every two seconds while deferred, cancelled on stop/sign-out; explicit Retry checks immediately. Preserve real destination-write errors and atomic publication. Do not expand this into a memory-card rewrite.

**Done:** A reader holding `retrobat.exe` does not block setup. A running frontend delays its gamelist update, which resumes after it closes. Errors say what failed and offer Retry; they do not assert RetroBat is running merely because Windows returned a sharing error.

## 2. Reuse the games folder across normal stop/start

**Targets:** `cfapi/windows.rs::WindowsMount::mount`, `Drop for WindowsMount`, `manifest_for`, and the existing native CFAPI test.

Change ordinary Stop/Quit to disconnect and drain callbacks while retaining registration, manifest, placeholders, and hydrated bytes. On the next start, reconcile against the existing identity (server, ROM/file, version, size, path): reuse unchanged entries; create missing/new ones; replace or remove only changed/obsolete entries verified as owned. Do not rewrite unchanged entries' permissions or read ROM bodies to compare them.

Extract the destructive cleanup currently inside `Drop` and invoke it explicitly when replacing a root/account; never adopt another server's tree. Update the native test that currently requires Stop to empty the ROM tree. This is an intentional behavior change: stopping leaves game entries on disk; downloads require RomMFS to be running. Preserve cache eviction.

**Done:** An unchanged stop/start creates and deletes zero placeholders and preserves their file identities. A changed ROM affects only its owned entries. Listing downloads no ROM bytes; cold and cached reads still work; user files survive.

## 3. Remove avoidable waiting and filesystem work

**Targets:** `app/controller/{connection,catalogue,mount,retrobat_setup}.rs`, `cfapi/root.rs::{read_frontend_metadata,publish_frontend_metadata,publish_local}`.

Move cold catalogue loading and mount preparation off the controller command loop using the existing job/result-polling pattern. Keep state updates on the controller; reuse existing cancellation/session checks. Process cancellation without waiting for the job. Disconnect/dispose of a mount returned after cancellation; merely ignoring its event is insufficient. Late results cannot reactivate the old session.

Skip publication of already-owned, unchanged artwork at its existing versioned target. Validate the managed tree once per metadata publication and pass that operation's manifest to subordinate helpers; retain destination checks immediately before writes. Do not cache a permanent “trusted” flag.

Keep favourites/history reconciliation: those can change while the ROM catalogue stays identical. Retain the existing unchanged-XML check.

**Done:** A delayed fixture response does not block cancellation/sign-out. Repeating unchanged artwork publication causes no downloads or replacements; deleting one owned image republishes only that image. One publication performs one full-tree validation.

## 4. Correct the facts behind the statuses

**Targets:** `app/controller/{state,catalogue,retrobat_setup}.rs`, `core/retrobat/setup.rs`, existing UI summary helpers.

- Remove `setup_complete || previous_success` from `IntegrationUpdated`. Reuse actual-link inspection with the backend-aware root validation already used by metadata publication; the generic inspection rejects CFAPI reparse points. Recheck at startup, after setup, on app focus, and explicit refresh. Missing links show the affected system and an existing-setup repair action. If it interrupts the mount, label it **Stop and repair** and handle stop/repair/restart in that action. Preserve busy/foreign files.
- Set `pending_remount` by comparing the projected tree with the mounted tree, including content version/size, not by `mount.is_some()`. Identical refreshes and artwork-only changes must not request a remount.
- Extend the existing metadata summary to carry partial failures: Updated, Updating, Waiting, Updated with issues, or Failed. Skipped publication cannot produce unconditional “Up to date.” Waiting/finished states have no running progress bar. Reuse this summary everywhere; do not replace all of `UiState`.
- Show “Needs permission” for favourites/history only after an actual permission failure. Pending discovery is not a permission error, and Reconnect is not the recovery for every failure.

**Done:** Overview, Settings, and Activity agree. Removing a recorded link clears healthy status after recheck. An unchanged refresh produces no “Apply library changes.” A partially failed publication identifies its problem while usable games remain available.

## 5. Make the existing interface follow the user's task

**Targets:** `app/window.rs`, `app/window/{actions,folder_controls}.rs`, `app/window/pages/{setup,overview,settings,save_review}.rs`.

First-run RetroBat order is **Connect → choose folders/RetroBat → connect RetroBat → start games folder**. Never instruct the user to start and then stop just to finish setup. Keep the generic-folder route. Reuse existing controls; no new wizard framework. Returning users retain their configuration and current page; reconnect must not navigate them away.

Overview leads with games-folder status and its primary Start/Open action, with Stop secondary. Below it, keep compact RetroBat, artwork, and saves statuses with the relevant recovery action. Keep configuration in Settings. Preserve the current visual style.

Default Saves to **Tracked**; **Review saves** opens **Needs review**. Keep All/Not linked/Skipped accessible. Show names, useful status, dates, and actions. Put raw IDs, hashes, revision formats, and backend errors in the existing details/log controls. Keep unsupported-library counts in Library details without describing every omitted file as a missing game.

**Done:** Setup requires no backtracking. Routine use does not require reading technical details. Every displayed blocker has a usable next action. No cosmetic/color tests.

## 6. Finish save-conflict recovery

**Targets:** `app/controller/commands.rs`, `app/save_sync_agent/{actor,incoming,incoming_transfer,transfer}.rs`, `core/save_sync/journal/incoming.rs`, Saves UI.

Export alone never clears the current review state. Add two explicit actions for one mapped save:

- **Keep this PC's save:** preserve remote files, record the decision for the currently observed history, resolve matching review records, and queue the existing local snapshot/upload path.
- **Use this RomM copy:** select an existing staged, hash-verified revision; publish through the existing incoming-save/backup helpers, update the baseline, and resolve matching review records. Preserve the replaced local copy.

Carry existing session/mapping identity and the local version reviewed. If that local version changes before application, require a fresh choice. Persist the decision and honor it in BOTH incoming reconciliation and upload checks (`verify_existing_baseline`, `post_upload_inventory`), so the same unchanged history cannot immediately recreate the blockage.

Retain rejected remote record IDs/reasons instead of only `unsupported_count`. Do not blindly remove that guard: these records already passed mapped-slot checks. An explicit choice may acknowledge leaving them untouched; never automatically install unknown content. Offer Use RomM only for an eligible verified copy. No format conversion or general import system.

**Done:** Either supported resolution resumes sync and survives restart; the losing copy is preserved. Unchanged acknowledged history stays resolved, while a new or changed competing record requires review. Other saves continue syncing.

## 7. Verify the repairs and deliver a runnable build

Implement first; verify these outcomes afterward using existing fixtures and temporary data. Extend only behavior tests covering the changes above. Run the repository's required format/build/test checks for the available target; do not create a new harness, benchmark project, platform matrix, or human acceptance programme.

Use the existing native CFAPI test on an available Windows host and inspect the changed UI using existing tooling. Temporary fixture data is sufficient; no production credentials or changes to Aaron's real saves. If native Windows is unavailable, finish the code and available checks, identify exactly which native checks remain unrun, and do not claim those passed. This does not justify another baseline request.

Add concise start/result/failure logs to the changed operations with elapsed time and reuse/change counts. No per-file log floods or new telemetry system.

Deliver the runnable Windows build through the existing build path where available, its revision, and seven short Pass / Fail / Not run results corresponding to these sections. Update only docs made inaccurate by these changes. Do not substitute a checklist, screenshots, or passing portable tests for a working application; do not mark remaining failures complete.

### Native lifecycle reference

Microsoft distinguishes [disconnecting the provider](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfdisconnectsyncroot) from [unregistering its sync root](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfunregistersyncroot). Ordinary shutdown should use the former; the latter traverses and converts/deletes placeholders. Keep callback lifetime handling intact.
