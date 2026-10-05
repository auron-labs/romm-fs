# Test WinFsp and RetroBat on Windows

Use this guide to test the current checkout on a native Windows host. It is
written for an LLM with PowerShell, filesystem access, and, for manual checks,
desktop interaction tools. The filesystem backend is **WinFsp**.

Run the available sections and produce the report below. Do not treat source
inspection, compilation, or a test command that runs zero tests as a runtime pass.
If desktop tools, dependencies, or test installations are missing, complete the
independent checks and report the affected cases as blocked. Do not modify the
application to make tests pass; report reproducible defects separately.

## 1. Prepare the host and evidence

Use native Windows PowerShell in the repository root, rather than WSL. Open a
Visual Studio Developer PowerShell if the compiler tools are absent from PATH.

Prerequisites:

- Rust 1.89 or later with an MSVC target.
- Visual Studio C++ build tools, Windows SDK, and Clang/libclang for bindgen.
- WinFsp 2.1 or later, with runtime and Developer components installed.
- An interactive desktop for app, RetroBat, and native picker checks.
- A disposable RomM server/account for live tests. ROM access requires
  `platforms.read` and `roms.read`; save sync also requires `me.read`,
  `assets.read`, and `assets.write`.
- Two disposable RetroBat **8.2.1** installations for the save-sync checks.
  Use RetroArch/Gambatte for a single-file Game Boy `.gb` game.

Record the environment before testing:

```powershell
git status --short
git rev-parse HEAD
Get-CimInstance Win32_OperatingSystem | Select-Object Caption, Version, OSArchitecture
rustc -Vv
cargo -V
rustup show active-toolchain
Get-Command cl.exe, clang.exe -ErrorAction SilentlyContinue
```

Confirm that Rust's host target ends in `windows-msvc`. Record the installed
WinFsp version and installation location, RetroBat version, and actual RomM
version. A missing tool or driver is an environment blocker; retain the error.
Do not change drivers or system installations without host authorization.

Create an evidence folder outside the mount and saves folders:

```powershell
$Evidence = Join-Path $env:TEMP ('rommfs-test-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $Evidence | Out-Null
```

Save command output and exit codes there. For example:

```powershell
cargo fmt --all -- --check 2>&1 | Tee-Object -FilePath (Join-Path $Evidence 'fmt.txt')
$LASTEXITCODE
```

Record `$LASTEXITCODE` immediately after each native command. Keep credentials,
tokens, and cookies out of reports and screenshots. Preserve pre-existing
workspace changes. Use a dedicated Windows user profile if the host already has
RomMFS consent/settings, so a prior opt-in cannot affect default-state checks.

## 2. Run automated checks

Run each command from the repository root and record its result independently:

```powershell
cargo fmt --all -- --check
cargo test --locked -p rommfs-core -p rommfs-fixture -p rommfs-app --no-default-features -- --test-threads=1
cargo test --locked --workspace -- --test-threads=1
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --locked -p rommfs-app
```

The headless suite covers catalogue/cache behavior, fixture HTTP contracts,
discovery, profile validation, consent, durable upload/reconciliation, and
controller behavior. The workspace suite also builds the desktop feature and
runs native WinFsp integration. The live-RomM test is ignored by default.
Record executed, failed, and ignored test counts; do not copy historical counts
from the acceptance checklist.

Run these native checks explicitly, even if the workspace suite passed, to
make Windows evidence easy to identify:

```powershell
cargo test --locked -p rommfs-winfsp --test winfsp_native -- --nocapture --test-threads=1
cargo test --locked -p rommfs-core --lib save_sync::path::tests::windows_save_read_handle_denies_concurrent_write_and_delete -- --exact --nocapture
cargo test --locked -p rommfs-core --lib save_sync::path::tests::windows_directory_guard_blocks_rename_until_released -- --exact --nocapture
cargo test --locked -p rommfs-app --no-default-features --lib save_sync_agent::files::tests::windows_publication_does_not_replace_a_file_created_at_the_boundary -- --exact --nocapture
cargo test --locked -p rommfs-app --no-default-features --lib save_sync_agent::tests::real_filesystem_notification_drives_the_agent_without_waiting_for_fallback_scan -- --exact --nocapture
```

Each command must execute one test. If it executes zero, inspect `-- --list`
and resolve the selector before reporting a result.

The WinFsp test verifies these behaviors through real Windows filesystem calls:

- Enumeration/stat request no ROM bodies, including a large directory listing.
- First read downloads exactly once and returns exact fixture bytes.
- Reads with different path casing and seek offsets work; warm reads stay cached.
- Write, append, delete, rename, hard-link, and file/directory creation fail.
- Open handles prevent eviction; closing them permits eviction and a new download.
- Unmount restores an empty folder; remount reuses the private cache.
- A nonempty mount root is refused without changing its local data.

The remaining tests exercise Windows sharing/rename protection, publication
without replacing a concurrently created save, and actual filesystem notifications.
These use disposable fixtures and do not verify a live RomM server.

If a failure occurs, retain the first log. Re-run the failing test alone to
investigate; record both outcomes if it is intermittent. A build failure blocks
downstream runtime checks and must not be described as a failed runtime assertion.

## 3. Test a real RomM through WinFsp

Use the [local harness](romm-harness/README.md) with Docker Desktop running Linux
containers. From the repository root:

```powershell
Push-Location testing/romm-harness
docker compose up -d
docker compose ps
Pop-Location
```

Wait for `http://127.0.0.1:8080` to respond. Complete initial user setup and
trigger a library scan through the RomM web UI. Verify that its catalogue
contains the three committed placeholder files under `library/roms/{nes,snes,gb}`.
Use the harness's local-only `admin` / `admin123` test account if following its
documented setup. Do not reuse those credentials elsewhere.

The compose file uses `latest` image tags. Record the actual server version and
image IDs; the fixture contract models RomM 5.3.1, but compose does not pin it.
If port 8080 or the container names are already in use, report the collision
without stopping unrelated services.

Run from the repository root:

```powershell
$env:ROMM_URL = 'http://127.0.0.1:8080'
cargo test --locked -p rommfs-winfsp --test live_romm -- --ignored --nocapture --test-threads=1
```

For different disposable credentials, supply `ROMM_USER` and `ROMM_PASS` privately
in the process environment. Do not print them. Restore any environment variables
you changed when finished.

Expected: exactly one live test runs, at least three ROMs are verified, and cold
and warm reads match the committed sources byte for byte. This test expects a
harness-only catalogue: every projected file must have a matching committed
source. Do not point it at an arbitrary library.

The placeholder ROMs prove filesystem bytes, not emulator compatibility. Use a
separate, legally available playable Game Boy ROM with battery saves for the
RetroBat launch and save-sync checks.

## 4. Check the desktop mount and migration

Start `target\debug\rommfs-app.exe` on the interactive desktop. Use the test
server and a fresh, empty mount folder under a disposable local directory.
Keep the mount outside RetroBat saves and the evidence folder.

| Case | Action | Expected result |
| --- | --- | --- |
| Connect | Enter the test URL and credentials; press **Connect**. | Connected state and catalogue appear; credentials are absent from logs. |
| Mount | Enter the empty folder; press **Start**. | Platform folders and ROM filenames appear in Explorer and PowerShell. |
| List | Run `Get-ChildItem -LiteralPath '<mount>' -Recurse` before opening a ROM. | Metadata appears. Use native fixture counters from section 2 as proof of no content downloads; Explorer may have other readers. |
| Read | Run `Get-FileHash -LiteralPath '<mounted ROM>' -Algorithm SHA256`; compare with its known source. Repeat. | Hashes match; warm reads do not create a new ROM transfer. Hashing performs a content read. |
| Stop/remount | Press **Stop**, inspect the folder, then **Start** again. | Folder is restored empty; sibling `<folder>.rommfs-root` marker remains; cached bytes are reused. |
| Close | Close the app while mounted, then inspect the folder. | Mount stops and the empty folder returns; no tray/background app remains. |
| Old backend root | Put a sentinel file in a separate disposable folder; record its hash; attempt to mount there. | Mount is refused; sentinel and hash remain unchanged. Never test against a real old ROM/save library. |
| Server ownership | Claim a test folder for server A, stop, then try it for a different server identity. | Ownership mismatch is refused. A configured URL base path is part of server identity. |

Replace the quoted placeholders with actual paths. Record errors, screenshots,
and hashes. Do not remove ownership markers to bypass a rejection. The WinFsp
backend stores ROM bytes in `%LOCALAPPDATA%\rommfs\cache\<server>`, rather than
leaving hydrated files in the mount folder.

Configure the disposable RetroBat installation's Game Boy ROM path to the mounted
`gb` folder using its supported configuration workflow. Record and restore any
tester-made configuration changes. RomMFS does not configure RetroBat itself.
Launch a playable game using RetroArch/Gambatte, create a battery save, then
exit the emulator cleanly. Verify the save lands outside the read-only mount.
Record the actual effective save path and visible ROM stem used in the next section.

## 5. Check RetroBat discovery and consent

Use backed-up, disposable installations A and B. Stop emulators before replacing
test saves. Hash existing saves and emulator configuration files before testing,
and compare after each case. RomMFS must not edit emulator configuration.

| Case | Action | Expected result |
| --- | --- | --- |
| Default | Connect in a clean test profile. | Save sync is off; no save API requests occur before enabling. Authentication/account verification may still occur. |
| Discovery | Press **Refresh** with installations at fixed/removable `X:\RetroBat`; also test an installation elsewhere while RetroBat/EmulationStation is running. | Valid installations appear once per path; choose explicitly. |
| Browse | Use **Browse…** for an installation outside discovered locations. | Native picker selects the intended installation; cancelling preserves selection. |
| Preview | Select A without enabling. | Server, authenticated account, 8.2.1 profile, effective saves folder, mapped targets, supported/skipped existing files, and target paths are shown. |
| Read-only preview | Refresh with a missing saves directory in a disposable install. | No saves folder is created. No save contents are opened/hashed by preview and no save API requests occur. Use fixture tests for proof beyond visible effects. |
| Unsupported profile | Select an unsupported version or settings/core layout in a disposable install. | Explanation is shown; enabling is refused. Do not relabel a real installation to fake version compatibility. |
| Scope | Change account, server, selected installation, or effective saves root. | Prior consent cannot authorize the changed scope. Confirm preview before any new opt-in. |
| Missing selection | Close the app; temporarily move the selected disposable install; restart and refresh. | Prior selection remains visible and paused; another installation is not silently selected. Restore it afterward. |
| Debounce | Apply `0`, `3601`, `1.5`, and nonnumeric text; then apply `5` and restart. | Invalid values are rejected and not saved. Whole seconds 1–3600 are accepted; valid value persists for its scope. |
| Keyboard | Tab through controls; activate selection, refresh, enable/disable, and export with keyboard. | Focus is visible; actions and picker cancellation work. |

Only visible catalogue `.gb` games map to `gb/<visible ROM stem>.srm` under the
effective saves root. `.gbc`, `.rtc`, save states, memory cards, ambiguous names,
multi-file ROMs, and other profiles are excluded. Check exclusions in the automated
mapping/profile tests and any matching disposable entries available on this host.
Record skipped counts separately from mapped game counts. A bounded or unavailable
preview scan must be labelled partial/unavailable, not a complete zero-file scan.

## 6. Run a live save-sync smoke test

Use one disposable RomM account and the same playable Game Boy catalogue entry
for both installations. Record its ROM ID and mapped `.srm` paths. Back up saves
outside both saves roots; record hashes. Begin with no remote saves for that test
game/account/profile so old history cannot obscure the result.

Use one RomMFS app instance at a time. The app persists journal/settings privately
under `%LOCALAPPDATA%\rommfs\settings\`; do not clear them between restart checks.
Use server request logs or equivalent HTTP evidence for claims about requests.
If unavailable, mark those assertions unverified rather than inferring from UI.

1. Select A, verify the preview, and explicitly enable save sync. Create a battery
   save by playing and exiting cleanly. Wait beyond the configured debounce and
   observe a verified upload. Confirm a UUID-named remote save exists for the
   correct account/ROM/profile. Download that specific version and compare its
   SHA256 with A's stable local save. Check request options
   `overwrite=false&autocleanup=false` where request evidence is available.
2. Close the app. Create a different valid test save at B's mapped path, record
   its hash, then restart, select B, verify preview, and enable. Expect incoming
   review/conflict; B's hash stays unchanged and the remote A copy remains.
3. Use **Export…** to choose a new absolute path outside the entire effective
   saves root, such as `<evidence folder>\from-A.rommfs-incoming`. Verify the
   exported hash matches A's uploaded version. Review remains pending. Repeat
   export to that existing path: it must fail without changing the file. An
   export inside the active saves root or to a mapped save must also be refused.
4. Disable sync, close, restart, and reconnect to the same server/account with B
   selected. Keep sync disabled. Review must persist and export to another fresh
   path must work. Confirm export makes no save API requests; verify B's hash
   still matches its original bytes.
5. Test first incoming install in a fresh disposable installation/path whose
   mapped target has never existed or been tracked by RomMFS. Enable for the
   same remote game. The incoming save may install automatically and must match
   the selected remote bytes. A deleted, previously tracked save does not qualify.
6. In a conflict-free disposable case with a verified baseline, make only the
   test server unavailable. Change the save, wait beyond debounce, and capture
   the pending/retry state. Close and restart; restore the server, reconnect,
   and enable the same scope if necessary. The queued generation must survive
   and upload with verified bytes. Pending work must not appear up-to-date.
7. With a tracked test save, disable sync, remove that disposable save, then
   re-enable and observe reconciliation. It must stay absent and the remote save
   must remain. Do not delete saves from a real installation for this check.

Record time limits before observing asynchronous cases (for example, 60 seconds
after debounce on a responsive local server). If the limit expires, report the
last queue/error state and relevant requests; do not wait indefinitely.

Automated fixtures cover interrupted/ambiguous POST responses, concurrent writes,
401/403 pauses, 429 retries, tied history, and incoming-publication races. A live
smoke pass does not prove these cases against the actual server. Report that
coverage separately. Remote retention may prune history even with the request
flags above; do not promise unlimited versions.

## 7. Clean up and report

Disable save sync and close RomMFS before restoring backups. Close emulator
processes and restore tester-made configuration changes. Verify restored save
hashes, released mounts, and preserved sentinel files. Keep evidence and pending
journal data until failures have been reviewed. Remove only disposable artifacts
created for this run; never recursively delete a mount or an existing cache.

If you started the harness, stop it without removing its volumes:

```powershell
Push-Location testing/romm-harness
docker compose down
Pop-Location
git status --short
```

Write a report in the evidence folder with:

- Commit, initial/final workspace status, Windows/toolchain/WinFsp versions,
  actual server version/image, RetroBat version, and test scope.
- A table of case ID or command, PASS/FAIL/BLOCKED/NOT RUN, exit code or observed
  result, executed/ignored counts, and evidence path.
- Separate conclusions for automated fixtures, native Windows filesystem/watcher,
  desktop/picker, live ROM reads, RetroBat launch, and live save sync.
- Each defect's severity, exact reproduction steps, expected/actual behavior,
  hashes or redacted logs, and whether it reproduced on an isolated retry.
- Cleanup results and all unverified assertions, including unavailable request
  tracing or desktop access. Do not mark the entire Windows/RetroBat integration
  verified while any required runtime section remains blocked or not run.

Use [SAVE-SYNC-CHECKLIST.md](../SAVE-SYNC-CHECKLIST.md) for acceptance contracts
and [README.md](../README.md) for supported behavior. Preserve historical
verification statements unless this run supplies evidence to update them.
