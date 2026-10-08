# Approved-scope implementation status

Updated 2026-10-08. Scope authority: [approved plan](../.scratch/PLAN.md).
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

## Still unfinished

- Official installer discovery/download/integrity checks/execution/cancellation.
  Selecting and setting up an existing fresh installation is implemented.
- Generalized save-file mapping/identity, streamed large/extensionless/companion
  saves, remote original-path representation, clean-client reconstruction,
  journal migration and baseline/conflict-aware replacement. Existing GB-only
  consent and transfer restrictions remain; new scopes are not silently enabled.
- External BIOS/keys/firmware/cheat inventory and safe materialization contracts.
  Generic ROM-file auxiliaries and RetroBat Switch `.nsp` projection are not a
  claim of dedicated BIOS/cheat support.
- Application-session/tray lifetime, icons, single-instance enforcement,
  start-at-login preference and ordered reconnect/mount/sync restoration.
- Final full-scope review. The reported live HTTP 500 cause is still unknown.

## Reproducible source contracts

RomM source: `95599dadbe93c8f7b8a8647148fafbe293df3167`; see
[API contract](API-CONTRACT.md). New extensions are source/fixture evidence,
not live RomM 5.3.1 validation.

RetroBat launcher source: `af2dc472fecfaadf851ecf0e92d1e4eb7c519b31`:

- [Eden generator](https://github.com/RetroBat-Official/emulatorlauncher/blob/af2dc472fecfaadf851ecf0e92d1e4eb7c519b31/emulatorLauncher/Generators/Eden.Generator.cs):
  `emulators/eden/user/config/qt-config.ini`, section `[Data%20Storage]`,
  `save_directory\default=false`, `save_directory=<saves>/switch/`.
- [Citron generator](https://github.com/RetroBat-Official/emulatorlauncher/blob/af2dc472fecfaadf851ecf0e92d1e4eb7c519b31/emulatorLauncher/Generators/Citron.Generator.cs):
  its own `user/config/qt-config.ini`, section `[Data%20Storage]`,
  `global_custom_save_path_enabled\default=false`,
  `global_custom_save_path_enabled=true`,
  `global_custom_save_path\default=false`,
  `global_custom_save_path=<saves>/switch/`.
- Both use `yuzu_mutualize`. [Program.cs](https://github.com/RetroBat-Official/emulatorlauncher/blob/af2dc472fecfaadf851ecf0e92d1e4eb7c519b31/emulatorLauncher/Program.cs)
  applies global, system and per-game settings, then final command-line
  overrides. Direct invocation can still override the configured destination.

Installer-source research: `retrobat-setup` revision
`4eb2ed0c4b40a47a077471580bb67b3c1153bbc2` contains both
[Inno Setup](https://github.com/RetroBat-Official/retrobat-setup/blob/4eb2ed0c4b40a47a077471580bb67b3c1153bbc2/installer/installer.iss)
and legacy NSIS scripts. Do not infer installer switches from `.exe` alone.
The reference package is `RetroBat-v8.2.1-stable-win64-setup.exe`,
1,901,457,836 bytes, SHA-256
`afbb060de637fdb1de9b7465276cafb30c8d32f40cb2271b256e52c0de257b2c`.
Its version is not an installation compatibility gate. Installer execution and
signature/runtime acceptance remain unverified.

## Validation boundary

Linux portable tests, formatting, Clippy and GPUI compilation are available.
A 25-platform/2,448-ROM HTTP fixture checks 13 pages fetched once, safe
projection and zero ROM-body requests; it reports local HTTP/projection timings,
not Windows mount latency.
One local run measured 450 ms for fixture HTTP and 40 ms for projection. Those
measurements include neither a real RomM server nor CFAPI setup/hydration.
A MinGW toolchain was extracted under `/tmp/opencode/rommfs-mingw` for
`x86_64-pc-windows-gnu` type checking without installing system packages.
Cross-target checks do not link/run the desktop or prove native permissions.

Verification commands used on Linux:

```sh
cargo fmt --all --check
cargo clippy -p rommfs-core -p rommfs-fixture -p rommfs-app -p rommfs-cfapi --no-default-features --all-targets -- -D warnings
cargo test -p rommfs-core -p rommfs-fixture -p rommfs-app -p rommfs-cfapi --no-default-features -- --test-threads=1
cargo check -p rommfs-app
```

Cross-target checking used the temporary compiler through target-specific
`CC`, `AR` and Cargo linker environment variables, with
`cargo check -p rommfs-core -p rommfs-cfapi -p rommfs-app --no-default-features --target x86_64-pc-windows-gnu --all-targets`.

Watcher tests exhausted this shared Linux host's 128 inotify-instance
limit (`Too many open files`), causing timeouts, including in a one-thread
combined run. A probe found 125 visible descriptors already used by unrelated
processes. The failed logs are retained in
`/tmp/opencode/rommfs-portable-metadata-concurrent-failed.log` and
`/tmp/opencode/rommfs-watcher-suite-diagnostics.log`. Subsequent verification uses
one fresh process per app unit test; results are in
`/tmp/opencode/rommfs-app-isolated-unit-tests.log`. Host limits and unrelated
processes were left untouched; watcher tests were not skipped. The
independently reproduced scheduler race above is not presented as proof that it
caused that resource-exhaustion failure.

Latest outcomes:

| Check | Result |
| --- | --- |
| Core, fixture and portable CFAPI tests | 145 passed |
| App controller integration tests | 14 passed |
| Isolated app unit tests | 61 passed; 2 watcher-dependent assertions failed under inotify exhaustion |
| Formatting and diff whitespace | Passed |
| Clippy, including fixture tests | Passed with warnings denied |
| Linux GPUI app compilation | Passed |
| Windows GNU cross-target, headless crates/tests | Passed; no native execution |
| Local documentation links | Passed |

The two environment-blocked app cases are
`local_write_while_first_upload_response_is_blocked_is_captured_and_uploaded_next`
and `real_filesystem_notification_drives_the_agent_without_waiting_for_fallback_scan`.
They remain unverified in the latest full verification; no test was removed or
ignored, and the portable suite is **not claimed wholly green** on this host.

All real Windows and human cases remain pending, including CFAPI ACLs,
delete/rename protection, link privilege/cleanup, frontend/emulator activity
guards, installation, browser approval, Credential Manager, actual save paths,
watchers, tray and Windows login. Procedures are in
[Windows testing](../testing/WINDOWS-TESTING.md); none are prerequisites for
continuing Linux implementation.
