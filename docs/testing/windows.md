# Test RomMFS on Windows

Use this guide to test the current checkout on a native Windows host. It is
written for an operator with PowerShell, filesystem access, and, for manual
checks, an interactive desktop. The filesystem backend is **CFAPI**. Windows
10/11 is the client acceptance environment. Windows Server (including Server
2022) is diagnostic evidence only; it cannot replace a Windows 10/11 client run.

## Guided path

For the easiest operator path, clone the repository on a Windows 10/11 desktop
and double-click `test-wizard.cmd` in the repository root. The wizard guides the
remaining human checks and stores resumable evidence under
`%LOCALAPPDATA%\rommfs\test-wizard`. Rust is needed only if you opt into the
automated checks/build. This is a guided evidence collector, not automation or
proof of the complete acceptance procedure below. It has not yet been run under
Windows PowerShell 5.1 on a Windows desktop.

At a result prompt, `S` records a skip and `Q` packages the partial run; launch
the wizard again to resume. `C` first asks permission to capture the **full
desktop on all monitors**, then waits four seconds for you to switch to the app.
Nothing is uploaded. Review the ZIP's screenshots and best-effort-redacted logs
for private information, then send the ZIP shown by the wizard. Missing,
skipped, blocked, partial, and unverified checks are not passes.

The wizard's before/after manifests are deliberately bounded: they hash at most
500 eligible ordinary, non-placeholder files under the selected `roms`/`saves`
areas, do not traverse RomM links, and treat `gamelist.xml` merges separately.
They are not blanket proof that every file is byte-identical. This document
remains the full source testing and native acceptance procedure. Public usage is
only `test-wizard.cmd` or
`powershell -NoProfile -File .\scripts\windows-test-wizard.ps1 -Help`.

Run the available sections and produce the report below. Keep source/build/test
evidence distinct from native runtime acceptance. A command that runs zero tests
is not a pass. If a prerequisite blocks a case, continue independent cases and
record the blocked case rather than silently skipping it. Do not modify the
application or a real RetroBat installation to make acceptance pass; report
reproducible defects separately.

**Current validation status:** the Linux portable suite and a Windows Server 2022 diagnostic run are recorded in the [dated validation record](../validation/2026-10-12.md). Windows 10/11 client acceptance and the specific manual cases listed in that record remain open. The sections below are the repeatable procedure and evidence template. The dated record identifies executed cases; update it when new evidence is collected. Do not infer a pass from a command that ran zero tests.
Portable ordinary-file fixtures and the UI compilation check are not evidence
of emulator, tray, installer or native save-lock behavior.

## Start a local demo server

For a quick desktop test, use the root [compose.dev.yaml](../../compose.dev.yaml).
It pins RomM 5.3.1 and MariaDB 11.8, matching the existing harness versions.
RomM rejects SQLite as a database backend; see its
[database configuration](https://github.com/rommapp/romm/blob/5.3.1/backend/config/config_manager.py).
RomMFS's own local SQLite databases are separate from the server database.

Install Docker Desktop with Linux containers and Python 3.10+ on the test host.
Use native Windows PowerShell in the repository root for the commands below.
The Python launcher is `py`; on Linux/macOS use `python3` for the same script.
The generator uses only Python's standard library.

```powershell
py -3 scripts/create-demo-roms.py
docker compose -f compose.dev.yaml config --quiet
docker compose -f compose.dev.yaml up -d
docker compose -f compose.dev.yaml ps
```

Open `http://127.0.0.1:8081`. If the page is not ready, inspect
`docker compose -f compose.dev.yaml logs --tail 100 romm romm-db` and retry once
startup completes. Finish RomM's first-run setup with a disposable admin account.
Metadata-provider credentials are unnecessary for these synthetic files; skip
provider setup and scan the library through the web UI with metadata sources
disabled. Confirm six platforms and 18 single-file games appear before opening
RomMFS. Metadata lookup misses do not make these files playable.

The Compose project is `rommfs-dev`, with no fixed container names. Only RomM's
HTTP port is published, on loopback; MariaDB has no host port. Database passwords
and the signing key in this file are public development values. Use this stack
only for disposable local testing. The web account you create is separate from
the database account. Browser pairing and password sign-in remain enabled;
download authentication and CSRF protection retain their normal settings.

The generated layout is:

```text
romm/
  roms/
    gb/Demo GB 001.gb
    gbc/Demo GBC 001.gbc
    nes/Demo NES 001.nes
    snes/Demo SNES 001.sfc
    gba/Demo GBA 001.gba
    n64/Demo N64 001.z64
    ...002 and 003 for each platform
```

Every byte is zero. Sizes are representative test values, not exact dumps of
named games or valid cartridge headers. These files test scanning, listing,
downloads, caching, and byte equality; they cannot test emulator gameplay.

| Platform | Extension | Bytes per file |
| --- | --- | ---: |
| Game Boy | `.gb` | 32,768 |
| Game Boy Color | `.gbc` | 1,048,576 |
| NES | `.nes` | 262,144 |
| SNES | `.sfc` | 2,097,152 |
| Game Boy Advance | `.gba` | 8,388,608 |
| Nintendo 64 | `.z64` | 33,554,432 |

The default set occupies about 130 MiB. The script writes zero bytes in bounded
chunks. Rerunning verifies and retains matching files. It refuses existing files
with a different size or nonzero content and does not delete unrelated files.
After an interrupted write, inspect the reported partial file and remove only
that generated file before retrying. Changing size options requires a fresh
output directory or deliberate removal of the old generated files.

Use `--help` for options. For example, a fresh 72-file, 72-MiB catalogue:

```powershell
py -3 scripts/create-demo-roms.py --root romm-small --games-per-platform 12 --size-mib 1
```

`--root` names the parent of `roms`; its default is this checkout's `romm`
directory, regardless of the shell's working directory. Compose always mounts
`./romm/roms` at `/romm/library/roms`, read-only. A custom output such as
`romm-small` needs a matching bind-source edit before RomM can serve it.
The default generated `/romm/` tree is ignored by Git. Custom output paths are
not automatically ignored. Config, database, resources, Redis data, and uploaded
saves live in separate named volumes. ROM uploads through the web UI cannot
write to the read-only demo library; add source files on the host, then rescan.

Build and open the desktop app after completing section 1's prerequisites:

```powershell
cargo build --locked -p rommfs-app
.\target\debug\rommfs-app.exe
```

Connect RomMFS to `http://127.0.0.1:8081`, choose an empty local NTFS games folder,
select **Generic folders**, and press **Start games folder**. Keep this folder
separate from the generated source tree. Confirm **Games folder ready**, then
enumerate without hashing first. Hashing deliberately downloads the selected ROM:

```powershell
$DemoMount = 'C:\RomMFS-Test\Games' # The folder selected in RomMFS.
Get-ChildItem -LiteralPath $DemoMount -Recurse
$SourceRom = Join-Path $PWD 'romm\roms\gb\Demo GB 001.gb'
$MountedRom = Join-Path $DemoMount 'gb\Demo GB 001.gb'
$Expected = (Get-FileHash -LiteralPath $SourceRom -Algorithm SHA256).Hash
$Cold = (Get-FileHash -LiteralPath $MountedRom -Algorithm SHA256).Hash
$Warm = (Get-FileHash -LiteralPath $MountedRom -Algorithm SHA256).Hash
if ($Expected -ne $Cold -or $Expected -ne $Warm) { throw 'ROM byte mismatch' }
```

Check Activity for the first transfer and cache reuse on the repeated read.
Explorer, antivirus, and other applications can also read placeholders; use
the native fixture test in section 2 for isolated no-download-on-listing proof.

Use port 8080's committed harness for `ROM-PLACEHOLDERS` in section 3. That test
compares every ROM against `testing/romm-harness/library`; **do not point it at
the generated demo library**. Both stacks can run together, with separate
accounts and state. When switching between them, stop the games folder and use
a fresh mount root for the different server identity. Never remove an ownership
marker to reuse a root for another server.

If 8081 is occupied, set `$env:ROMM_DEV_PORT = '8082'` before starting the demo
stack and use that URL throughout its tests. For offline/recovery cases, use
`docker compose -f compose.dev.yaml stop romm`, then `start romm`.
Finish with **Quit** from the RomMFS tray and:

```powershell
docker compose -f compose.dev.yaml down
```

This retains the demo ROMs and all server state. For an intentional full reset
of this disposable server, `docker compose -f compose.dev.yaml down --volumes`
deletes its accounts, database, uploaded saves, config, and resources together;
the host ROM files remain. A new server may assign different account/game IDs,
so recheck identity and consent before reusing a client test scope.

## 1. Prepare the host and evidence

Use native Windows PowerShell in the repository root, rather than WSL. Open a
Visual Studio Developer PowerShell if compiler tools are absent from PATH.
Record whether the shell is elevated; do not elevate or install drivers unless
the host owner authorizes it. No WinFsp, ProjFS, or other driver installation is
a test prerequisite or an acceptable workaround.

Prerequisites:

- Rust 1.89 or later with an MSVC target.
- Visual Studio C++ build tools and Windows SDK.
- Windows 10 version 1709+ or Windows 11, with the built-in Cloud Files platform.
- A local NTFS mount root, outside any OneDrive/Dropbox/other sync root.
- Driver-free installation acceptance requires an approved clean host/profile
  with WinFsp not installed and ProjFS not enabled. Record existing drivers as
  found; do not install, remove, or disable them on the user's host. Their
  presence is a deviation/blocker for the driver-free claim, not for independent
  functional diagnostics; use an approved clean host instead.
- An interactive desktop for app, RetroBat, and native picker checks.
- A disposable RomM server/account for live tests. ROM access requires
  `platforms.read` and `roms.read`; save sync also requires `me.read`,
  `assets.read`, and `assets.write`.
- Two disposable compatible RetroBat installations for the save-sync checks.
  **8.2.1** is the package/source reference, not an exact-version gate.
  Use RetroArch/Gambatte for a single-file Game Boy `.gb` game.

Record the environment before testing using the logged commands below; retain
errors from missing tools.

Confirm that Rust's host target ends in `windows-msvc`. Record the Windows
edition/build, whether PowerShell is elevated, Cloud Files service state, and
the mount volume's filesystem. For repeatability, choose a fixed local NTFS
mount root outside any cloud-sync root and confirm it is empty. The backend
rejects network shares and requires NTFS; a removable volume is not excluded if
it is local NTFS and outside a sync root. Missing tools/platform support are
environment blockers; retain errors. Do not change drivers or system
installations without host authorization.

Compare the chosen mount path against the host's configured OneDrive/Dropbox or
other sync roots (including custom locations); `CldFlt` being present does not
by itself prove the selected root is local NTFS or outside a sync provider.
Record both the check and the actual `$MountRoot` used.

Record the actual RetroBat installation, not a renamed or synthetic fixture:
the unmodified `system/version.info` text, its hash, the release/package hash
if the original package is available, and the file version/hash of the installed
`RetroBat.exe` and EmulationStation executable. The code accepts the official
compatible layout regardless of version string; record what the real
file says and never relabel it to force profile acceptance. Record
EmulationStation's version as corroborating evidence, not as a replacement for
`system/version.info`. If the package or version metadata is unavailable, say
so rather than infer a version. On the test host, use the real install root:

```powershell
. ./scripts/windows-test-support.ps1
$RunId = '{0}-{1}' -f (Get-Date -Format 'yyyyMMdd-HHmmss'), ([guid]::NewGuid().ToString('N').Substring(0, 8))
$EvidencePath = Join-Path $env:TEMP "rommfs-test-$RunId"
$Evidence = Initialize-RommFsEvidence -EvidencePath $EvidencePath
```

The dot-sourced [Windows test helper](../../scripts/windows-test-support.ps1)
creates only the requested evidence directory and its case subdirectories.
Choose a new path for each run; it refuses to reuse an existing evidence path.

```powershell
$RetroBatRoot = 'C:\RetroBat' # Replace with the real, unmodified installation root.
$VersionInfo = Join-Path $RetroBatRoot 'system\version.info'
& {
    Get-Content -LiteralPath $VersionInfo
    Get-FileHash -LiteralPath $VersionInfo -Algorithm SHA256
    Get-FileHash -LiteralPath (Join-Path $RetroBatRoot 'RetroBat.exe') -Algorithm SHA256
    Get-ChildItem -LiteralPath $RetroBatRoot -Filter 'emulationstation.exe' -File -Recurse |
        ForEach-Object { $_.FullName; $_.VersionInfo.FileVersion; Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256 }
    $RetroBatPackage = $null # Set this to the retained original package/archive path, if available.
    if ($RetroBatPackage -and (Test-Path -LiteralPath $RetroBatPackage -PathType Leaf)) {
        Get-FileHash -LiteralPath $RetroBatPackage -Algorithm SHA256
    }
} 2>&1 | Tee-Object -FilePath (Join-Path $Evidence 'ENV-HOST.txt') -Append
```

The evidence folder is outside the mount and saves folders. Keep all per-run
output there and retain the test-created mount root separately. Use
`commands/<case-id>.txt` for native output/exit codes, `harness/` for scan and
image/version observations, `desktop/<case-id>.md` plus `screenshots/` for UI
walkthrough evidence, and `saves/api-preflight.txt` plus `saves/SAVE-01` through
`SAVE-07` for scoped requests, hashes, deadlines, and outcomes.

Use stable case IDs throughout the guide and report. `PASS` means the stated
observable contract was demonstrated; `FAIL` means it was contradicted;
`BLOCKED` means a prerequisite prevented the test; `NOT RUN` means it was not
attempted; `PARTIAL` means only part of the contract was exercised; and
`UNVERIFIED` means evidence (often request tracing) was insufficient to decide.
Never turn a blocked or unverified assertion into a pass by relying on source
inspection, a screenshot alone, or a test with zero executions.

Capture every native command's combined output and exit code in its own file
with `Invoke-LoggedCommand`, defined by the dot-sourced helper above. It reads
the caller's `$Evidence` variable, so keep that variable in the current
PowerShell session. The helper records `$LASTEXITCODE` immediately after the
scriptblock and is intended for native commands.

Create the mount root and capture environment details after defining the helper.
PowerShell cmdlet results/errors go to `ENV-HOST.txt`; native commands each get
their own file and immediate exit code. Choose a writable local test path and
verify it is empty before mounting.

```powershell
$MountRoot = Join-Path $env:TEMP "rommfs-mount-$RunId"
New-Item -ItemType Directory -Path $MountRoot | Out-Null
& {
    Get-CimInstance Win32_OperatingSystem | Select-Object Caption, Version, BuildNumber, OSArchitecture
    Get-Command cl.exe, clang.exe -ErrorAction SilentlyContinue
    Get-Service CldFlt -ErrorAction Continue
    Get-Volume -DriveLetter ($MountRoot.Substring(0, 1)) | Select-Object DriveLetter, FileSystem, FileSystemLabel
    $principal = [System.Security.Principal.WindowsPrincipal]::new([System.Security.Principal.WindowsIdentity]::GetCurrent())
    "MountRoot=$MountRoot"
    "PowerShell elevated=$($principal.IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator))"
} 2>&1 | Tee-Object -FilePath (Join-Path $Evidence 'ENV-HOST.txt') -Append
$null = Invoke-LoggedCommand 'ENV-GIT-STATUS' { git status --short }
$null = Invoke-LoggedCommand 'ENV-COMMIT' { git rev-parse HEAD }
$null = Invoke-LoggedCommand 'ENV-WHOAMI' { whoami }
$null = Invoke-LoggedCommand 'ENV-RUSTC' { rustc -Vv }
$null = Invoke-LoggedCommand 'ENV-CARGO' { cargo -V }
$null = Invoke-LoggedCommand 'ENV-TOOLCHAIN' { rustup show active-toolchain }
```

Record the actual RetroBat installation metadata in `ENV-HOST.txt`; the real
`system/version.info`, executable hashes, and EmulationStation file version
come from the inspection commands above, not a synthetic fixture.

Never capture or print a bearer token, password, token response, JWT, cookie,
authorization header, or environment dump. Preserve pre-existing workspace
changes (record `git status --short` at start and finish; do not reset/clean the
checkout). Use a dedicated Windows user profile if the host already has RomMFS
consent/settings, so a prior opt-in cannot affect default-state checks.

### Run order and gates

Use these case IDs in the evidence folder and report. The order avoids attributing
the tester's API probes to the app or treating the harness's placeholder ROMs as
playable games.

| Gate / IDs | Acceptance gate | Blocked dependents | Independent work to continue |
| --- | --- | --- | --- |
| `ENV-*` | Windows 10/11, toolchain, Cloud Files, local NTFS root, and interactive desktop are recorded. | Native mount and UI cases if missing. | Source-level cargo checks that the available host supports. |
| `AUTO-*`, `NATIVE-*` | Commands pass and each targeted test executes exactly once. | A failed build blocks launching this checkout; a failed native case blocks only its contract. | Profile tests and unrelated native tests may still run and must be reported. |
| `HARNESS-*`, `ROM-PLACEHOLDERS` | Isolated RomM catalogue and placeholder live-read test pass before adding any extra ROM. | Live catalogue reads if harness/scan unavailable. | Desktop profile preview can run independently of mounting. |
| `ROM-PLAYABLE` | After placeholder pass, import a legal playable `.gb`, rescan, and record catalogue ID/filename/source hash. | RetroBat game launch and save sync if no playable mapped ROM exists. | Mount, profile preview, and native checks. |
| `API-PREFLIGHT` | With a scanned ROM ID, verify the password-grant scopes, `/api/users/me` identity/scopes, and ROM/slot inventory. If live fixture/mount is blocked, a placeholder GB ID is permission-only; rerun with `ROM-PLAYABLE` before `SAVE-*`. | Live save-sync cases if scoped access fails or the playable-ROM check has not passed. | Desktop mount and profile checks can continue. |
| `UI-MOUNT-*` | Actual app mounts, reads, stops, and cleans up on the client OS. | RetroBat-through-mount launch. | `UI-PROFILE-*` preview/discovery checks do not depend on a successful CFAPI mount. |
| `UI-PROFILE-*` | Real RetroBat installation is discovered/selected; preview and accessible controls behave as stated. | Consent and save-sync cases if selection/profile is invalid. | Native tests and ROM-only checks. |
| `RB-LAUNCH-*` | Genuine EmulationStation remains available through actual process discovery and launches the legal playable `.gb` through the mount. | Live save-sync cases requiring a real mapped save. | Remaining UI cases and API preflight. |
| `SAVE-*` | Full `API-PREFLIGHT` with the playable ROM ID succeeds before the app-only baseline; each save scenario has its own evidence window. | Live save-sync acceptance if scoped inventory is unauthorized or playable-ROM inventory is missing. | Mount and profile checks; a `403` is not a reason to stop them. |

Complete independent cases after a blocker. In particular, a failed or blocked
mount does not block profile preview, and a RomM save API `401`/`403` is not
evidence that the RetroBat version marker is unsupported.

## 2. Run automated checks

Run each command from the repository root and record its result independently.
The optional mise aliases `mise run test:portable` and `mise run check:ui` map to
the portable/headless suite and host-native UI compilation. On a native Windows
host, `mise run test:native` maps to the workspace suite; it fails with an
explicit message on other operating systems. Mise is optional, and the direct
commands below remain the evidence commands for this procedure:

```powershell
$null = Invoke-LoggedCommand 'AUTO-FMT' { cargo fmt --all -- --check }
$null = Invoke-LoggedCommand 'AUTO-HEADLESS' { cargo test --locked -p rommfs-core -p rommfs-fixture -p rommfs-cfapi -p rommfs-app --no-default-features -- --test-threads=1 }
$null = Invoke-LoggedCommand 'AUTO-WORKSPACE' { cargo test --locked --workspace -- --test-threads=1 }
$null = Invoke-LoggedCommand 'AUTO-CLIPPY' { cargo clippy --locked --workspace --all-targets -- -D warnings }
$null = Invoke-LoggedCommand 'AUTO-BUILD' { cargo build --locked -p rommfs-app }
```

The headless suite covers catalogue/cache behavior, fixture HTTP contracts,
discovery, profile validation, consent, durable upload/reconciliation, and
controller behavior. The workspace suite also builds the desktop feature and
runs native CFAPI integration. The live-RomM test is ignored by default.
Record executed, failed, and ignored test counts; do not copy historical counts
from the acceptance checklist.

Run these native checks explicitly, even if the workspace suite passed, to make
Windows evidence easy to identify. Keep each exact selector as a separate
command/log so its run count is unambiguous:

```powershell
$null = Invoke-LoggedCommand 'NATIVE-CFAPI' { cargo test --locked -p rommfs-cfapi --test cfapi_native cfapi_mount_lists_reads_once_and_stays_read_only -- --exact --nocapture --test-threads=1 }
$null = Invoke-LoggedCommand 'NATIVE-READ-HANDLE' { cargo test --locked -p rommfs-core --lib save_sync::path::tests::windows_save_read_handle_denies_concurrent_write_and_delete -- --exact --nocapture --test-threads=1 }
$null = Invoke-LoggedCommand 'NATIVE-DIRECTORY-GUARD' { cargo test --locked -p rommfs-core --lib save_sync::path::tests::windows_directory_guard_blocks_rename_until_released -- --exact --nocapture --test-threads=1 }
$null = Invoke-LoggedCommand 'NATIVE-CANONICAL-PATHS' { cargo test --locked -p rommfs-core --lib save_sync::path::tests::windows_canonicalized_paths_accept_existing_and_missing_children -- --exact --nocapture --test-threads=1 }
$null = Invoke-LoggedCommand 'NATIVE-ANCESTOR-JUNCTION' { cargo test --locked -p rommfs-core --lib save_sync::path::tests::windows_canonicalized_ancestor_junction_is_rejected -- --exact --nocapture --test-threads=1 }
$null = Invoke-LoggedCommand 'NATIVE-CANONICAL-PUBLISH' { cargo test --locked -p rommfs-app --no-default-features --lib save_sync_agent::files::tests::windows_canonicalized_save_paths_publish_without_replacing -- --exact --nocapture --test-threads=1 }
$null = Invoke-LoggedCommand 'NATIVE-CANONICAL-EXPORT' { cargo test --locked -p rommfs-app --no-default-features --lib save_sync_agent::tests::exports::canonicalized_export_parent_accepts_save_export -- --exact --nocapture --test-threads=1 }
$null = Invoke-LoggedCommand 'NATIVE-NO-CLOBBER-BOUNDARY' { cargo test --locked -p rommfs-app --no-default-features --lib save_sync_agent::files::tests::windows_publication_does_not_replace_a_file_created_at_the_boundary -- --exact --nocapture --test-threads=1 }
$null = Invoke-LoggedCommand 'NATIVE-FILESYSTEM-WATCHER' { cargo test --locked -p rommfs-app --no-default-features --lib save_sync_agent::tests::lifecycle::real_filesystem_notification_drives_the_agent_without_waiting_for_fallback_scan -- --exact --nocapture --test-threads=1 }
$null = Invoke-LoggedCommand 'PROFILE-VERSION-MARKERS' { cargo test --locked -p rommfs-core --lib save_sync::legacy_gb_profile::tests::version_markers_are_diagnostics_not_compatibility_gates -- --exact --nocapture --test-threads=1 }
$null = Invoke-LoggedCommand 'PROFILE-STOCK-821' { cargo test --locked -p rommfs-core --lib save_sync::legacy_gb_profile::tests::verifies_stock_821_gambatte_and_resolves_uncreated_saves_root -- --exact --nocapture --test-threads=1 }
```

Each `NATIVE-*` and `PROFILE-*` command must execute exactly one test. If it
executes zero, inspect `-- --list` and resolve the selector before reporting a
result; zero is not a pass. The ancestor-junction test creates a disposable
directory junction with `cmd /C mklink /J` and asserts that setup succeeded. A
junction-creation failure is a visible test failure/setup blocker to report,
never a silently skipped test. The directory-guard test checks both the saves
directory and an ancestor cannot be renamed while held, then checks both can be
renamed after release.

The CFAPI test verifies these behaviors through real Windows filesystem calls:

- Enumeration/stat request no ROM bodies, including a large directory listing.
- First read downloads exactly once and returns exact fixture bytes.
- Reads with different path casing and seek offsets work; warm reads stay cached.
- A child process opens warm NTFS bytes and refreshes last-use without a download.
- A truncated download fails without exposing partial bytes; a later read retries.
- ROM write, append, delete, rename and hard-link attempts fail. Ordinary
  frontend files may be created; unrelated unowned files still prevent remount.
- Open handles defer eviction; closing them permits dehydration and a new download.
- Unmount removes ROM placeholders but preserves owned frontend metadata;
  remount reuses the private cache and retains favourites/history.
- A nonempty mount root is refused without changing its local data.

The remaining tests exercise canonicalized paths, ancestor-junction rejection,
Windows sharing/rename protection, no-clobber save publication/export, and
actual filesystem notifications. Compatible version markers are diagnostic;
missing required layout/settings must produce capability-specific errors.
These tests use disposable fixtures and do not verify a
live RomM server or a real RetroBat installation.

If a failure occurs, retain the first log. Re-run the failing test alone to
investigate; record both outcomes if it is intermittent. A build failure blocks
downstream runtime checks and must not be described as a failed runtime assertion.

### Setup and metadata acceptance cases

Use disposable installs and roots, never a real game/save library. Run the
native CFAPI regression above as a standard user; it now publishes writable
metadata beside protected ROMs, checks no ROM request during publication, and
retains frontend state across a fresh-session remount. Record any elevated-token
exceptions rather than treating skipped protection checks as passes.

| Case | Procedure | Required evidence |
| --- | --- | --- |
| `SETUP-OWNERSHIP` | Try setup with stock/user files in `roms`; then with an empty directory tree. Add a user-owned `roms\\<system>` link, a file in place of a system folder, and an existing `roms\\<system>\\RomM`. Repeat setup. | Every system folder stays a real folder with its files byte-identical; each gets a `RomM` link to the managed folder. The user-owned cases are skipped with a reason while every other system is linked. Repeat setup reuses links. |
| `SETUP-RECOVERY` | Without Developer Mode, run setup and decline the UAC prompt; rerun and approve. Interrupt after some links; rerun. Upgrade an install set up by an earlier version (whole-system links and `.rommfs-original-roms`). | Declining changes nothing. Approving links every system. Rerun completes the rest. Earlier layouts become real folders with `RomM` links, and moved folders return to `roms`. |
| `SETUP-SWITCH` | Back up settings, enable setup, inspect unrelated values and per-game disabling overrides. Launch each installed emulator through RetroBat. | Config backup, unrelated XML unchanged, override rejection, and actual distinct Eden/Citron serialized keys plus effective save paths. Missing emulator is separate from configuration failure. |
| `META-ACL` | Publish XML/artwork; edit and atomically replace a gamelist as the frontend does. Try ordinary overwrite/delete/rename of ROMs, including via the same parent and another process. | Local metadata writes work; parent `FILE_DELETE_CHILD` cannot bypass ROM protection. Cross-target/portable checks do not count. |
| `META-LIFETIME` | Change favourites/play counts in the frontend, exit, refresh, stop/remount, then exercise interrupted publication and projected-name changes. | Frontend values survive; launch paths remain actual filenames; removed games disappear without losing archived history. |
| `META-ACTIVITY` | Refresh while EmulationStation or a Switch emulator runs, then exit. Also exercise unavailable access permissions. | Ready metadata waits, does not clobber in-memory history, publishes after safe exclusive access, and reports access/capability failures. |
| `META-NETWORK` | Trace listing, XML generation and artwork requests separately. Include missing artwork and a rejected session. | No ROM bodies/detail requests; missing artwork nonfatal, authentication rejection requires sign-in, local references load in RetroBat. |

### Installer, ordinary files, auxiliaries and lifecycle acceptance cases

| Case | Procedure | Required evidence |
| --- | --- | --- |
| `INSTALLER` | Download the official stable package, cancel during download, retry, launch, and cancel inside its wizard. Select a completed fresh install for setup. | Digest/size verified before execution; no executable partial download; no guessed silent flags or process killing; cancelled/partial installs left untouched. A zero exit code alone does not prove installation. |
| `INSTALLER-ELEVATION` | On a standard-user account, start Install RetroBat. Approve the UAC prompt once; repeat and cancel the prompt. | The UAC prompt appears for the installer only (RomMFS itself stays unelevated). Cancelling shows the plain "cancelled" message, leaves nothing half-installed, and offers retry. |
| `RETROBAT-PRISTINE` | Install a stock RetroBat 8.3.0 into an empty folder, then run Set up RetroBat against it and mount. Compare the stock `roms\` and `saves\` trees before and after. | Setup succeeds on the stock install. Stock files are byte-identical; each system has a `RomM` link. RomM games appear in RetroBat next to stock games (e.g. ports), and the stock `ports\gamelist.xml` entries remain. Favourite a RomM game, play it, remount: favourite and play count survive. |
| `AUXILIARY-NOTES` | Run Install support files straight after the pristine setup (and once with Set up RetroBat skipped or failed). | The result reads as success with notes, not as an error; the step is available whenever RetroBat is installed and connected, and warnings are listed without a red failure. |
| `SAVE-SYNC-QUIET` | On a clean profile, never enable save sync; browse Overview, Save sync and Activity. Then select the stock RetroBat install. | Save sync shows a neutral Off state with a one-line hint and an "Open Settings" button, with no "Needs attention" or "unsupported:" text. The 1,800+ stock RetroBat save files collapse into one "files don't belong to games in your library - ignored" line with a Show toggle, and nothing is uploaded. |
| `LOG-SIZE` | After a full clean-install session (download RetroBat, setup, support files, a catalogue refresh of a large library), inspect `rommfs.log`. | No per-tick progress lines: installer and support-file progress appear only at start, finish/failure and at most every 10% and 10 seconds. Catalogue warnings are one counted line per reason with up to five example ids. The log stays well under one 5 MiB file (5 archives kept). |
| `FILES-CLEAN` | Use two disposable save roots and the same game/account. Transfer companions, nested extensionless files, empty files and a >1 MiB file; give the second client no first-client journal. | Byte hashes and relative paths match; remote slots reconstruct paths despite filename rewriting; watcher notices new files. |
| `FILES-CONFLICT` | Establish a synchronized baseline; change only remote, then change both sides, then delete local. Hold a local file open without sharing writes/deletion. | Remote-only update safely replaces unchanged baseline and keeps backup; simultaneous/active changes stay pending; deliberate deletion does not restore or delete remote files. |
| `FILES-MIGRATION` | Upgrade a copied legacy journal with queued work and old enabled SRAM consent. | Baselines/snapshots/incoming/removal state retained; broader scope off until explicitly enabled. |
| `AUXILIARY` | Install lawful BIOS, unpacked Switch firmware/keys and RetroArch .cht files. Retry, modify a local file, supply an unsupported archive, and change the server version. | Correct effective destinations and ownership index; no game hydration/list entries; unrelated/modified/changed-version files preserved and actionable issues shown; auxiliaries not watched as saves. |
| `TRAY` | Hide a mounted window, reopen from tray, start a second instance, then Quit during activity. | Mount/sync survive hiding; one process/mount; status/error accessible; Quit drains workers. Taskbar and tray icons visible. |
| `LOGIN` | Enable using remembered browser sign-in, log in again, then repeat with rejected auth or a failed mount. | Hidden startup authenticates, mounts, then resumes only the valid prior consent scope. Failure accessible from tray and no sync starts after a failed startup mount. Logout removes login registration. |

## 3. Test a real RomM through CFAPI

Use the [local harness](../../testing/romm-harness/README.md) with Docker Desktop running Linux
containers. From the repository root:

```powershell
Push-Location testing/romm-harness
$null = Invoke-LoggedCommand 'HARNESS-UP' { docker compose up -d }
$null = Invoke-LoggedCommand 'HARNESS-PS' { docker compose ps }
Pop-Location
```

Wait for `http://127.0.0.1:8080` to respond. Complete initial user setup and
trigger a library scan through the RomM web UI. Verify that its catalogue
contains only the three committed placeholder files under
`library/roms/{nes,snes,gb}`. Use the harness's local-only `admin` / `admin123`
test account if following its documented setup. Do not reuse those credentials
elsewhere.

The current harness compose pins RomM `5.3.1` and MariaDB `11.8`, but still
record the actual RomM version reported by the running server and the image
reference plus immutable image ID for both containers. Capture only those
image fields with this safe query; do not print `docker compose config` or
inspect environment/configuration fields, which can expose secrets:

```powershell
$null = Invoke-LoggedCommand 'HARNESS-IMAGES' { docker inspect --format '{{.Config.Image}} {{.Image}}' romm romm-db }
```

Do not edit the compose file for an acceptance run. If port 8080 or the
container names are already in use, report the collision without stopping
unrelated services.

Run from the repository root:

```powershell
$env:ROMM_URL = 'http://127.0.0.1:8080'
$null = Invoke-LoggedCommand 'ROM-PLACEHOLDERS' { cargo test --locked -p rommfs-cfapi --test live_romm -- --ignored --nocapture --test-threads=1 }
```

For different disposable credentials, supply `ROMM_USER` and `ROMM_PASS` privately
in the process environment. Do not print them. Restore any environment variables
you changed when finished.

Expected: exactly one live test runs, at least the three placeholder ROMs are
verified, and cold and warm reads match committed sources byte for byte. The
live test requires every catalogue entry to match a committed source. It must
run **before** importing a playable ROM; do not run it after adding a ROM that
is not committed in the repository, and do not point it at an arbitrary library.

If `ROM-PLACEHOLDERS` is blocked (or a later live mount is blocked) but the
harness scan still exposes the committed GB placeholder, keep the catalogue
source-matched and record that placeholder's RomM ROM ID. You may use that ID
for `API-PREFLIGHT` as a permission-only diagnostic; this does not pass the live
ROM test, a playable-game/battery-save check, or any live save-sync scenario.
Do not add an uncommitted ROM while `ROM-PLACEHOLDERS` remains unpassed.

After `ROM-PLACEHOLDERS` passes, import a legally available, playable single-file
Game Boy `.gb` ROM with battery saves (`ROM-PLAYABLE`), then trigger a supported
library rescan through the RomM UI. Verify it appears in the catalogue and record
its title, RomM ROM ID, catalogue filename, source SHA256, and the actual
RetroArch/Gambatte configuration. The placeholder files prove filesystem bytes
only; they are not playable-game evidence. If the ROM is temporarily copied into
this checkout's bind-mounted `library`, record that single test-created file and
remove only that file during cleanup. Do not alter the committed placeholders or
rerun `ROM-PLACEHOLDERS` with the extra catalogue entry present.

### Save API access preflight (`API-PREFLIGHT`)

Run this with a scanned ROM ID **before opening RomMFS**. Normally use the
`ROM-PLAYABLE` ID. If the placeholder live test or live mount is blocked, use
the recorded GB placeholder ID only for the permission-only diagnostic above;
record the mode and ID. After the placeholder test passes and the playable game
is imported/rescanned, repeat the scoped inventory with the actual playable ROM
ID before any `SAVE-*` case. Each run makes one password-grant request, uses the
returned bearer to probe `/api/users/me`, and requests the test-ROM/slot
inventory only if that response verifies a real account ID and all required
scopes. Request scopes are exactly `platforms.read`,
`roms.read`, `me.read`, `assets.read`, and `assets.write`, matching the verified
client contract. The ROM-only `ROM-PLACEHOLDERS` test requests only
`platforms.read roms.read`; it cannot prove save access. Admin role alone also
does not prove granted token scopes.

Run this directly in an interactive PowerShell prompt—not through
`Invoke-LoggedCommand`, a transcript, or debug/request logging. `Get-Credential`
collects the disposable account privately; username/password, form body, token
response, access/refresh tokens, and response bodies stay in memory and are
never printed or written to evidence. The console emits only grant/GET statuses,
the `/api/users/me` ID and granted scopes, and a successfully parsed inventory
count. Copy only those emitted fields into `$Evidence\saves\api-preflight.txt`.

```powershell
$BaseUri = 'http://127.0.0.1:8080'
$RomId = [long](Read-Host 'ROM ID: playable title, or GB placeholder for permission-only diagnosis')
$Slot = 'rommfs-retrobat-gb-srm-v1'
$requiredScopes = @('platforms.read', 'roms.read', 'me.read', 'assets.read', 'assets.write')
$scopeString = $requiredScopes -join ' '
Add-Type -AssemblyName System.Net.Http
$credential = Get-Credential -Message 'Disposable RomM test account; credentials remain private'

$grantStatus = 'NOT_SENT'
$meStatus = 'NOT_REQUESTED'
$savesStatus = 'NOT_REQUESTED'
$userId = $null
$meParsed = $false
$grantedScopes = $null
$inventoryCount = $null
$passwordPtr = [IntPtr]::Zero
$client = $null
$formContent = $null
$grantResponse = $null
$meResponse = $null
$savesResponse = $null
$plainPassword = $null
$accessToken = $null
try {
    if ($credential) {
        $passwordPtr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($credential.Password)
        $plainPassword = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($passwordPtr)
        $formValues = [System.Collections.Generic.Dictionary[string, string]]::new()
        $formValues.Add('grant_type', 'password')
        $formValues.Add('username', $credential.UserName)
        $formValues.Add('password', $plainPassword)
        $formValues.Add('scope', $scopeString)
        $formContent = [System.Net.Http.FormUrlEncodedContent]::new($formValues)
        $client = [System.Net.Http.HttpClient]::new()
        $client.Timeout = [TimeSpan]::FromSeconds(60)

        try {
            $grantResponse = $client.PostAsync("$BaseUri/api/token", $formContent).GetAwaiter().GetResult()
            $grantStatus = [int]$grantResponse.StatusCode
            if ($grantResponse.IsSuccessStatusCode) {
                try {
                    $grantBody = $grantResponse.Content.ReadAsStringAsync().GetAwaiter().GetResult()
                    $grantPayload = ConvertFrom-Json -InputObject $grantBody -ErrorAction Stop
                    $accessToken = [string]$grantPayload.access_token
                } catch {
                    $accessToken = $null
                }
            }
        } catch {
            $grantStatus = 'NO_RESPONSE'
        }

        if (-not [string]::IsNullOrWhiteSpace($accessToken)) {
            $client.DefaultRequestHeaders.Authorization = [System.Net.Http.Headers.AuthenticationHeaderValue]::new('Bearer', $accessToken)
            try {
                $meResponse = $client.GetAsync("$BaseUri/api/users/me").GetAwaiter().GetResult()
                $meStatus = [int]$meResponse.StatusCode
                if ($meResponse.IsSuccessStatusCode) {
                    try {
                        $meBody = $meResponse.Content.ReadAsStringAsync().GetAwaiter().GetResult()
                        $mePayload = ConvertFrom-Json -InputObject $meBody -ErrorAction Stop
                        if ($null -ne $mePayload.id) { $userId = [long]$mePayload.id }
                        $grantedScopes = @($mePayload.oauth_scopes | ForEach-Object { [string]$_ })
                        $meParsed = $true
                    } catch {
                        $userId = $null
                        $meParsed = $false
                        $grantedScopes = $null
                    }
                }
            } catch {
                $meStatus = 'NO_RESPONSE'
            }

            $missingScopes = @($requiredScopes | Where-Object { $grantedScopes -notcontains $_ })
            $identityAndScopesValid = ($null -ne $userId) -and ($userId -gt 0) -and ($missingScopes.Count -eq 0)
            if ($meResponse -and $meResponse.IsSuccessStatusCode -and $identityAndScopesValid) {
                $savesUri = "$BaseUri/api/saves?rom_id=$RomId&slot=$([uri]::EscapeDataString($Slot))"
                try {
                    $savesResponse = $client.GetAsync($savesUri).GetAwaiter().GetResult()
                    $savesStatus = [int]$savesResponse.StatusCode
                    if ($savesResponse.IsSuccessStatusCode) {
                        try {
                            $savesBody = $savesResponse.Content.ReadAsStringAsync().GetAwaiter().GetResult()
                            if ($savesBody.TrimStart().StartsWith('[')) {
                                $inventory = @($savesBody | ConvertFrom-Json -ErrorAction Stop)
                                $inventoryCount = $inventory.Count
                            }
                        } catch {
                            $inventoryCount = $null
                        }
                    }
                } catch {
                    $savesStatus = 'NO_RESPONSE'
                }
            }
        }
    }
} catch {
    if ($grantStatus -eq 'NOT_SENT') { $grantStatus = 'NO_RESPONSE' }
} finally {
    if ($grantResponse) { $grantResponse.Dispose() }
    if ($meResponse) { $meResponse.Dispose() }
    if ($savesResponse) { $savesResponse.Dispose() }
    if ($formContent) { $formContent.Dispose() }
    if ($client) { $client.Dispose() }
    if ($passwordPtr -ne [IntPtr]::Zero) { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($passwordPtr) }
    if ($credential) { $credential.Password.Dispose() }
    Remove-Variable credential, formValues, grantBody, grantPayload, meBody, mePayload, savesBody, inventory, missingScopes, plainPassword, accessToken -ErrorAction SilentlyContinue
}

"GRANT_STATUS=$grantStatus"
"ME_STATUS=$meStatus"
if ($meParsed) {
    if ($null -ne $userId) { "USER_ID=$userId" }
    "GRANTED_SCOPES=$($grantedScopes -join ',')"
}
"SAVES_STATUS=$savesStatus"
if ($null -ne $inventoryCount) { "SAVES_COUNT=$inventoryCount" }
```

Preflight passes only when the password grant succeeds, `/api/users/me` returns
a real account ID and all five required scopes, and `/api/saves?rom_id=<test-id>&slot=rommfs-retrobat-gb-srm-v1`
returns a successful, parsed inventory (an empty inventory is a valid count of
zero). Missing scopes, missing identity, non-success status, malformed response,
or no response is a blocker; do not request a ROM-only fallback and call it
success. For `401`/`403`, record the status and any redacted detail privately
from trusted server diagnostics; never publish raw response bodies, headers,
credentials, or token objects. Record only the disposable-account provenance,
grant time window, requested scopes, returned account ID/granted scopes, statuses,
and inventory count.

This password grant and both GETs are tester traffic, not app traffic. Record
their time window separately; establish the app's default-state request
baseline in a later, non-overlapping window. Filter or timestamp-segment server
logs before counting app requests so these probes cannot be misattributed to
RomMFS or falsify a zero-save-request assertion.

## 4. Check the desktop mount and migration

Start `target\debug\rommfs-app.exe` on the interactive desktop. Use the test
server and a fresh, empty mount folder under a disposable local directory.
Keep the mount outside RetroBat saves and the evidence folder.
After `API-PREFLIGHT`, start the app in a clean test profile and mark a fresh
app-only request-log window immediately before the first sign-in. Use this
connection for both `UI-MOUNT-CONNECT` and `UI-PROFILE-DEFAULT`; exclude the
earlier password grant and GET preflight window from all app request counts.

| Case | Action | Expected result |
| --- | --- | --- |
| `UI-MOUNT-CONNECT` | Enter the test URL and use **Sign in with browser…**, or expand password sign-in and press **Connect with password**. | Connected state and catalogue appear; credentials are absent from logs. |
| `UI-MOUNT-START` | Enter the empty folder; press **Start games folder**. | Platform folders and ROM filenames appear in Explorer and PowerShell. |
| `UI-MOUNT-LIST` | Run `Get-ChildItem -LiteralPath '<mount>' -Recurse` before opening a ROM. | Metadata appears. Use native fixture counters from section 2 as proof of no content downloads; Explorer may have other readers. |
| `UI-MOUNT-READ` | Run `Get-FileHash -LiteralPath '<mounted ROM>' -Algorithm SHA256`; compare with its known source. Repeat. | Hashes match; warm reads do not create a new ROM transfer. Hashing performs a content read. |
| `UI-MOUNT-STOP` | Press **Stop games folder**, inspect the folder, then **Start games folder** again. | Unmodified owned ROM placeholders are removed; owned frontend metadata and sibling ownership markers remain; cached bytes are reused on the next read. Busy or changed files may defer cleanup. |
| `UI-MOUNT-CLOSE` | Close the window while mounted, then inspect the folder; reopen from tray and explicitly Quit. | Hiding retains the mount; Quit stops it and preserves owned metadata. If tray creation fails, closing the visible window quits instead. |
| `UI-MOUNT-LEGACY-ROOT` | Put a sentinel file in a separate disposable folder; record its hash; attempt to mount there. | Mount is refused; sentinel and hash remain unchanged. Never test against a real old ROM/save library. |
| `UI-MOUNT-OWNERSHIP` | Claim a test folder for server A, stop, then try it for a different server identity. | Ownership mismatch is refused. A configured URL base path is part of server identity. |

Replace the quoted placeholders with actual paths. Record errors, screenshots,
and hashes. Do not remove ownership markers to bypass a rejection. The CFAPI
backend stores a private copy in `%LOCALAPPDATA%\rommfs\cache\<server>` and an
NTFS hydrated copy while mounted. Eviction must reclaim both. Clean Stop removes
verified ROM placeholders and restores the original root ACL; frontend metadata
remains, and busy/changed entries defer cleanup. Never
remove ownership markers or run recursive cleanup to bypass a refusal.

For RetroBat layout, use **Settings → RetroBat → Set up RetroBat** with the
disposable installation and a separate managed games folder. Setup links its
`roms` directory and makes backed-up frontend settings edits; complete the
`SETUP-*` cases above. Record any additional tester-made configuration changes.
For **`RB-LAUNCH`**, launch the actual RetroBat installation and a playable game
using its real RetroArch/Gambatte. Observe the genuine EmulationStation frontend
through process discovery and until the selected game actually launches. The
`RetroBat.exe` launcher may exit normally after handing off to the frontend; do
not require an arbitrary minimum lifetime. Record which real processes appear,
their elapsed times, and whether the frontend remains available through launch.
A replacement executable, synthetic process, or other process-discovery
stand-in is never acceptance evidence. If the real frontend exits before
discovery/launch completes, record elapsed time and display/GPU context without
claiming a GPU cause. Create a battery save, exit the emulator cleanly, verify
it lands outside the read-only mount, and record the actual effective save path
and visible ROM stem used in the next section.

## 5. Check RetroBat discovery and consent

Use backed-up, disposable installations A and B. Stop emulators before replacing
test saves. Copy the actual valid saves and relevant RetroBat/EmulationStation
configuration files to a safe location outside both installs; record SHA256
before testing and compare after each case. Save-sync preview and consent must
not edit emulator configuration. Explicit RetroBat setup has separate,
documented settings changes. Keep backups untouched and restore only tester-made changes.

| Case | Action | Expected result |
| --- | --- | --- |
| `UI-PROFILE-DEFAULT` | Use the first connection from `UI-MOUNT-CONNECT`, captured in its clean-profile app-only request window, without enabling save sync. | Save sync is off; no app save API requests occur before enabling. Authentication/account verification may still occur. Exclude the separate `API-PREFLIGHT` window. |
| `UI-PROFILE-DISCOVERY` | Press **Refresh** with a real installation at fixed `X:\RetroBat`, and with an installation elsewhere while the actual RetroBat/EmulationStation is running. Test removable media only if a real removable volume is available. | Valid installations appear once per path; choose explicitly. Record unavailable removable media as `BLOCKED`, not as covered by a fixed-drive or stand-in process test. |
| `UI-PROFILE-BROWSE` | Use **Browse…** for an installation outside discovered locations. | Native picker selects the intended installation; cancelling preserves selection. |
| `UI-PROFILE-PREVIEW` | Select A without enabling. | Server, authenticated account, selected installation, effective saves folder, mapped targets, supported/skipped existing files, and target paths are shown. |
| `UI-PROFILE-READONLY` | Refresh with a missing saves directory in a disposable install. | No saves folder is created. No save contents are opened/hashed by preview and no save API requests occur. Use fixture tests for proof beyond visible effects. |
| `UI-PROFILE-UNSUPPORTED` | Select a disposable install with missing required layout/settings. | Capability-specific explanation is shown; enabling is refused. A version string difference alone must not reject a compatible installation. |
| `UI-PROFILE-SCOPE` | Change account, server, selected installation, or effective saves root. | Prior consent cannot authorize the changed scope. Confirm preview before any new opt-in. |
| `UI-PROFILE-MISSING` | Close the app; temporarily move the selected disposable install; restart and refresh. | Prior selection remains visible and paused; another installation is not silently selected. Restore it afterward. |
| `UI-PROFILE-DEBOUNCE` | Apply `0`, `3601`, `1.5`, and nonnumeric text; then apply `5` and restart. | Invalid values are rejected and not saved. Whole seconds 1–3600 are accepted; valid value persists for its scope. |
| `UI-KEYBOARD` | Follow the keyboard walkthrough below with the real app. | Record visual evidence for each required focus and activation behavior; source inspection is not a keyboard pass. |

### Keyboard walkthrough (`UI-KEYBOARD`)

Use screenshots or a short screen recording where focus is visibly indicated;
save them under `$Evidence\screenshots` and note the action and focus target.
Start with the initial app state and check the URL field has the expected initial
focus. Tab and Shift+Tab through URL/credential inputs and the visible sign-in,
**Start games folder**, **Stop games folder**, **Copy**, and save-sync controls/candidates, recording the
actual order. Do not assume disabled controls should be reachable. In a text
field, Space must insert a space and must not activate a button. For enabled
actions, verify Enter and Space each invoke the same command once (use separate
trials and observe the result/request count). Disabled controls are skipped and
cannot activate. When an action becomes disabled during a transition (for
example, while connecting), verify focus stays on that control until Tab moves
away; the transition must not activate it a second time.

Open **Browse…** and **Export…** when their prerequisites are valid, then cancel
each native picker with Escape. Verify the selected installation and pending
review are unchanged and focus returns sensibly to the initiating control. If
an action is disabled because the selected profile is invalid or unavailable,
record that interaction as `BLOCKED` for activation—not a generic keyboard
`PASS`; separately record visible disabled-state/focus behavior. Never claim
visual focus based only on key bindings or source tests.

Ordinary files of every format, including `.rtc`, extensionless, empty and large
files, can be associated with a visible game through unique stems, server save
targets or an explicit mapping. Shared saves/cards remain excluded. Use only a
save-data root; unmatched files are visible, not guessed. Record mapped file
counts separately from unmatched paths. An unavailable/limit-exceeded scan is
not a complete zero-file result. Legacy GB-profile tests cover migration only.

### Test the merged UI changes

Run these after the basic mount checks. Use either the generated demo server or
the committed harness, and record which URL/account/root each case uses. These
cases cover [UI-PLAN tasks 1–6](../../.planning/archived/UI-PLAN.md#7-implementation-order)
and the final folder/session fixes in the merged branch. They are acceptance
targets, not a claim that the current checkout passes them.

| Case | Procedure | Expected result |
| --- | --- | --- |
| `UI-AUTH-STATES` | In a fresh profile, begin browser sign-in, cancel, retry, approve, then sign out. Separately exercise password fallback and a rejected password. | Only state-appropriate controls appear; cancellation is available while pending; authenticated account/server replace sign-in controls. Errors are visible without reading logs. |
| `UI-RESTORE-OFFLINE` | Complete setup using remembered browser sign-in, Quit, stop only the test RomM container, and restart RomMFS. Restore RomM and reconnect. | Configured users remain on Overview; the saved games folder remains visible. An unreachable server does not replay onboarding or imply a ready folder. |
| `UI-REAUTH-FOLDER` | In RomM, revoke this test client's browser authorization. Restart RomMFS and sign in again to the same account/server. | The remembered folder is republished to Overview/Settings and is used by **Start games folder**. Authentication failure does not erase it. |
| `UI-FOLDER-APPLY` | Stop the games folder; enter a different empty NTFS folder in Settings and press **Use games folder**. Return to Overview, start, stop, Quit, and reopen with remembered sign-in. | The applied path becomes the next mount target and survives restart. Applying a change while mounted is disabled with an adjacent reason. |
| `UI-SESSION-ISOLATION` | After using account/server A, sign out and connect to B; use a separate empty mount root for B. | A's folder/status/consent cannot authorize a mount or save operation in B. No stale ready state or old mount target leaks into the new session. |
| `UI-REFRESH-FAILURE` | With a warm catalogue and working folder, stop the test RomM container and press **Refresh library**. Switch pages, restore the server, then retry. | Refresh failure is visible and scoped to metadata; the existing catalogue remains usable. It does not report successful freshness. Retry does not erase the failure before an outcome, and repeated clicks do not duplicate the job. |
| `UI-SYNC-STATES` | Compare disabled sync, an invalid source, an empty valid source, only unlinked files, linked files with work pending, a failed operation, and linked files after a successful check. | Overview and Save sync agree. Zero linked files never means everything is synchronized. Unlinked files remain visible while eligible linked files can sync. Save problems do not mark a working games folder broken. |
| `UI-REVIEW-SCALE` | Create the review dataset below, select it as a custom save folder, and use Review files search, each status filter, and pagination. Search for the last path, then clear the filter while on a later page. | All results are reachable in pages of 50; counts distinguish groups/items from files; filtering cannot leave an invalid empty page or silently change a selected file. |
| `UI-REVIEW-MAPPING` | Expand one unlinked row, search for a demo game, inspect affected files, and assign it. Repeat for a folder with two files, then a single file in a mixed folder. | The picker actually filters. The chosen game/platform and exact affected paths are clear; only the intended selection is linked. Preview alone does not upload or overwrite. Check mapping effects while sync is disabled, then enable explicitly for transfer testing. |
| `UI-RETROBAT-FAILURE` | Use a disposable install with a nonempty `roms` path, and exercise a link-permission failure if the host lacks the privilege. Fix only the disposable prerequisite and retry. | The local failure shows its path/cause and useful recovery details, including OS error 1314 when that is the actual failure. Existing content is preserved; no blanket requirement to run the whole app as administrator. Support-file problems stay separate from folder readiness. |
| `UI-ACTIVITY` | During refresh, hydration, save sync, and available optional setup/download work, switch to Activity. Search/copy the technical log and toggle follow. | Running operations and outcomes remain visible with real units. Repeated diagnostics are grouped with counts while full logs remain available. A setup operation cannot disappear behind a misleading idle-transfer summary. |
| `UI-RESIZE` | Visit all four pages at 100%, 150%, and 200% display scaling where available, resize the window, and repeat `UI-KEYBOARD`. | Primary actions, errors, paths/details, search, and pagination stay reachable. Focus is visible and returns after pickers/dialogs close. Record actual sizes/scales and any clipping. |

For the large review dataset, use the `$RunId` from section 1. This creates
1,855 small synthetic files in distinct folders, outside both ROM roots:

```powershell
$ReviewSaves = Join-Path $env:TEMP "rommfs-review-$RunId"
New-Item -ItemType Directory -Path $ReviewSaves -ErrorAction Stop | Out-Null
1..1855 | ForEach-Object {
    $directory = Join-Path $ReviewSaves ('unmatched-{0:D4}' -f $_)
    New-Item -ItemType Directory -Path $directory -ErrorAction Stop | Out-Null
    [System.IO.File]::WriteAllBytes((Join-Path $directory 'progress.dat'), [byte[]]::new(128))
}
```

In Settings, expand **Use a different save folder**, enter `$ReviewSaves`, and
press **Use save folder**. Inspect the preview with sync off. Different parent
folders exercise group pagination; adding thousands of files to one group would
not exercise thousands of review rows. Add a second synthetic file to one group
for the affected-file preview case. Keep the test root until any mapping or
count mismatch has been investigated. These files support sync UI and byte-level
transport tests; real emulator save compatibility still requires section 6.

Capture Overview, Save sync, Activity, and an actionable error under
`$Evidence\screenshots`. Label synthetic-data screenshots and record each case
as `PASS`, `FAIL`, `BLOCKED`, or `NOT RUN` with its evidence path.

## 6. Run a live save-sync smoke test

Use one disposable RomM account and the same playable Game Boy catalogue entry
for both installations. Record account ID, ROM ID, slot
`rommfs-file-v2:gb/<visible ROM stem>.srm`, effective saves root, mapped `.srm` paths, and
baseline SHA256 values for the actual valid local save and each relevant remote
save. Begin with no remote saves for that test game/account/profile so old
history cannot obscure the result. Save backups outside both saves roots and
hash saves plus emulator configuration before any mutation. Do not replace
test saves while an emulator is open.

Use one RomMFS app instance at a time. The app persists journal/settings privately
under `%LOCALAPPDATA%\rommfs\settings\`; do not clear them between restart checks.
Use one evidence file per case (`SAVE-01` through `SAVE-07`) with start/end
timestamps, account/ROM/slot identity, local and remote IDs/hashes before and
after, visible queue/review state, and request-log evidence for that case's
non-overlapping traffic window. Use server request logs or equivalent HTTP
evidence for claims about requests. If unavailable, mark request assertions
`UNVERIFIED` rather than inferring from UI.

1. **`SAVE-01` upload.** Select A, verify the preview and API preflight identity,
   then explicitly enable save sync. Create a battery save by playing and
   exiting cleanly. Wait beyond the configured debounce and observe a verified
   upload. Confirm a UUID-named remote save exists for the correct account,
   ROM ID, and slot/profile. Download that specific version and compare SHA256
   with A's stable local save. Check request options
   `overwrite=false&autocleanup=false` where request evidence is available.
2. Close the app. Create a different valid test save at B's mapped path, record
   its hash, then restart, select B, verify preview, and enable. **`SAVE-02`
   conflict:** expect incoming review/conflict; B's bytes/hash stay unchanged
   and the remote A UUID/bytes remain unchanged.
3. Use **Export…** to choose a new absolute path outside the entire effective
   saves root, such as `<evidence folder>\from-A.rommfs-incoming`. Verify the
   exported hash matches A's uploaded version. Review remains pending. **`SAVE-03`
   export:** repeat to that existing destination; it must fail without changing
   its hash (no-clobber). Export inside the active saves root and to a mapped
   target must also be refused without changing the target.
4. Disable sync, close, restart, and reconnect to the same server/account with B
   selected. Keep sync disabled. **`SAVE-04` disabled/restart:** review must
   persist and export to another fresh path must work. Confirm export makes no
   app save API requests in this case's isolated request window; verify B's
   hash still matches its original bytes.
5. Test first incoming install in a fresh disposable installation/path whose
   mapped target has never existed or been tracked by RomMFS. This must be a
   genuinely fresh path/copy, not a tracked save that was deleted and recreated.
   **`SAVE-05` first install:** enable for the same remote game. The incoming save
   may install automatically and must match selected remote bytes. A deleted,
   previously tracked target does not qualify.
6. In a conflict-free disposable case with a verified baseline, make only the
   test server unavailable. Change the save, wait beyond debounce, and capture
   the pending/retry state. **`SAVE-06` offline queue:** close and restart; restore
   the server, reconnect, and enable the same scope if necessary. The queued
   generation must survive and upload with verified bytes. Pending work must
   not appear up-to-date before successful remote verification. Never clear the
   settings/journal database to reset this case.
7. With a tracked test save, disable sync, remove that disposable save, then
   re-enable and observe reconciliation. **`SAVE-07` tracked deletion:** the
   local target must stay absent and the remote save ID/bytes must remain; verify
   no remote-delete request. Do not delete saves from a real installation.

Record time limits before observing asynchronous cases (for example, 60 seconds
after debounce on a responsive local server). Record each deadline and start/end
time in that scenario's evidence file. If the limit expires, report the last
queue/error state and relevant requests; do not wait indefinitely or infer
completion from a transient UI label.

Automated fixtures cover interrupted/ambiguous POST responses, concurrent writes,
401/403 pauses, 429 retries, tied history, and incoming-publication races. A live
smoke pass does not prove these cases against the actual server. Report that
coverage separately. Remote retention may prune history even with the request
flags above; do not promise unlimited versions.

## 7. Clean up and report

Disable save sync and close RomMFS before restoring backups. Close emulator
processes and restore only tester-made configuration changes. Verify restored
save/configuration hashes, released mounts, and preserved sentinel files. Keep
evidence, ownership markers, private cache, settings, and pending journal data
until failures have been reviewed; do not delete them to make a rerun look clean.
Remove only disposable artifacts created for this run. Never recursively delete
a mount or an existing cache. If a process-discovery stand-in was used in an
earlier attempt, restore the real executable/configuration only from a verified
tester backup and mark that attempt invalid; do not replace the shipped
executable as a test method.

If you started the harness, stop it without removing its volumes:

```powershell
Push-Location testing/romm-harness
$null = Invoke-LoggedCommand 'HARNESS-DOWN' { docker compose down }
Pop-Location
$null = Invoke-LoggedCommand 'ENV-FINAL-STATUS' { git status --short }
```

`docker compose down` intentionally retains named database/resources volumes.
Do not add `-v`, delete the bind-mounted `library`, `assets`, or `config`, or
remove files that hold remote-save data while retaining the database volume.
Stop only containers started for this run; never stop a name/port collision
owned by someone else. Preserve the initial staged/unstaged workspace changes.

Write a report in the evidence folder with:

- Commit, initial/final workspace status, Windows/toolchain/elevation/Cloud Files
  and NTFS details, actual RomM and container image IDs, RetroBat release/package
  hashes and actual version metadata, and test scope.
- A table of stable case ID or command, `PASS`/`FAIL`/`BLOCKED`/`NOT RUN`/
  `PARTIAL`/`UNVERIFIED`, exit code or observed result, executed/failed/ignored
  counts, and evidence path.
- Separate conclusions for automated fixtures, native Windows filesystem/watcher,
  desktop/picker, live ROM reads, RetroBat launch, and live save sync.
- Each defect's severity, exact reproduction steps, expected/actual behavior,
  hashes or redacted logs, and whether it reproduced on an isolated retry.
- Cleanup results and all unverified assertions, including unavailable request
  tracing or desktop access. Do not mark the entire Windows/RetroBat integration
  verified while any required runtime section remains blocked or not run.

Use this compact report in `$Evidence\report.md` and link the per-case files:

```text
Run / date:
Checkout commit:
Initial -> final git status (preserve pre-existing changes):
Host / Windows edition+build / PowerShell elevation:
Rust toolchain / MSVC tools:
Cloud Files service / mount volume+filesystem / local NTFS path:
RomM version / RomM image ID / MariaDB image ID:
RetroBat actual root / system/version.info / EmulationStation version:
RetroBat package+installed executable hashes (or unavailable):
Playable title / ROM ID / source SHA256 / effective save root:
Evidence root:

| Case ID | Status | Command or observed result | Exit/count/deadline | Evidence path | Blocker or defect |
| --- | --- | --- | --- | --- | --- |
| ENV-* | | | | | |
| AUTO-* | | | | | |
| NATIVE-* | | | | | |
| HARNESS-*/ROM-* | | | | | |
| UI-MOUNT-* / UI-PROFILE-* / merged UI cases | | | | | |
| RB-LAUNCH-* | | | | | |
| API-PREFLIGHT / SAVE-01..SAVE-07 | | | | | |

Area conclusions: automated / native filesystem+watcher / desktop+picker /
live ROM / real RetroBat launch / live save sync:
Defects: severity, exact steps, expected vs actual, hashes/redacted evidence,
first and isolated-retry result:
Cleanup: restored hashes / released mounts / retained markers-cache-journal /
stopped own containers only / residual artifacts:
Unverified assertions and remaining blockers:
Overall: PASS only if every required runtime gate passed; otherwise BLOCKED,
PARTIAL, or FAIL with the remaining case IDs (never runtime PASS from source alone).
```

### Previously reported failures and retest order

The E2E report for 2026-10-05/06 at `7b1e4e2` recorded the following. The
corresponding D1-D4 code changes are present in this checkout, but this guide
and those source changes are **not** evidence of a native Windows pass. Re-run
the named cases and report what the real host does:

| Finding | Retest / interpretation |
| --- | --- |
| D1: prefix metadata failure stopped every mount. | Retain the first mount log; run `NATIVE-CFAPI`, then each mount/list/read/stop case. A source or compile pass is not mount evidence. |
| D2: an attribute-only directory guard did not deny rename. | Run `NATIVE-DIRECTORY-GUARD`; it now attempts rename of both the saves directory and an ancestor while guarded and after release. |
| D3: the app rejected the real shipped `8.2.1-stable-win64` marker; a synthetic fixture hid the bug. | Run both `PROFILE-*` selectors; read the real, unmodified `system/version.info`. Supported legacy bare `8.2.1` is not a reason to relabel an installation. |
| D4: Tab did nothing. | Complete `UI-KEYBOARD` on the actual desktop and attach visible focus evidence; key binding/source inspection does not close it. |
| `/api/saves` returned 403; token permissions were unknown. | Run `API-PREFLIGHT` with a fresh token and verify `/api/users/me` scopes plus a scoped inventory response. Until inventory succeeds, live save sync is an independent blocker—not an automatic D3/profile failure. |
| The reported real RetroBat process exited in under three seconds; GPU was suspected. | Distinguish the launcher from genuine EmulationStation: the launcher may exit on a normal handoff. Require the real frontend through discovery/game launch, record process/timing/display context, and do not infer a GPU cause. |
| Process discovery used a substitute; removable-media coverage was absent. | A substitute never counts as `PASS`; use the actual process. Mark removable-media discovery `BLOCKED` if no real removable volume is available. |
| Harness games were placeholders, not playable; many live cases were blocked. | Keep the harness catalogue source-matched. If the placeholder test/mount is blocked, a recorded GB placeholder ID supports permission-only `API-PREFLIGHT` (not a playable/live-sync pass). After `ROM-PLACEHOLDERS` passes, import/rescan the lawful playable ROM and repeat inventory with its actual ID before `SAVE-*`; record each remaining blocker. |

Suggested retest order: record the actual host/install and preserve first-failure
logs; run the exact profile and native regression selectors; prove
`ROM-PLACEHOLDERS`; if it or a live mount is blocked, keep the
source-matched harness catalogue and use the recorded GB placeholder ID for
permission-only `API-PREFLIGHT`. Once the live test passes, import/rescan the
playable ROM and repeat preflight with its ID before `SAVE-*`;
run the password-grant/API preflight in its own traffic window before opening
RomMFS; then exercise mount and keyboard independently; finally run genuine
RetroBat discovery/game launch and the seven isolated save scenarios after
backing up valid saves/configuration. Stop at deadlines, report residual
blockers, and do not give an overall `PASS` while a required runtime case is
blocked or not run.

Use [the approved save-sync scope](../../.planning/archived/PLAN.md#step-6--generalize-save-synchronization) for acceptance contracts
and [user guide](../user-guide.md) for supported behavior. Preserve historical
verification statements unless this run supplies evidence to update them.
