# Test CFAPI and RetroBat on Windows

Use this guide to test the current checkout on a native Windows host. It is
written for an operator with PowerShell, filesystem access, and, for manual
checks, an interactive desktop. The filesystem backend is **CFAPI**. Windows
10/11 is the client acceptance environment. Windows Server (including Server
2022) is diagnostic evidence only; it cannot replace a Windows 10/11 client run.

Run the available sections and produce the report below. Keep source/build/test
evidence distinct from native runtime acceptance. A command that runs zero tests
is not a pass. If a prerequisite blocks a case, continue independent cases and
record the blocked case rather than silently skipping it. Do not modify the
application or a real RetroBat installation to make acceptance pass; report
reproducible defects separately.

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
- Two disposable RetroBat **8.2.1** installations for the save-sync checks.
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
`8.2.1-stable-win64` marker as well as legacy bare `8.2.1`; record what the real
file says and never relabel it to force profile acceptance. Record
EmulationStation's version as corroborating evidence, not as a replacement for
`system/version.info`. If the package or version metadata is unavailable, say
so rather than infer a version. On the test host, use the real install root:

```powershell
$RunId = '{0}-{1}' -f (Get-Date -Format 'yyyyMMdd-HHmmss'), ([guid]::NewGuid().ToString('N').Substring(0, 8))
$Evidence = Join-Path $env:TEMP "rommfs-test-$RunId"
New-Item -ItemType Directory -Path $Evidence | Out-Null
@('commands', 'native', 'harness', 'desktop', 'saves', 'screenshots') | ForEach-Object {
    New-Item -ItemType Directory -Path (Join-Path $Evidence $_) | Out-Null
}
```

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

Capture every native command's combined output and exit code in its own file.
Define this helper once in the PowerShell session and use it for native commands
throughout the run; the exit code is written immediately after each command:

```powershell
function Invoke-LoggedCommand {
    param([Parameter(Mandatory)][string]$Id, [Parameter(Mandatory)][scriptblock]$Command)
    $log = Join-Path (Join-Path $Evidence 'commands') "$Id.txt"
    "COMMAND: $Command" | Set-Content -LiteralPath $log
    & $Command 2>&1 | Tee-Object -FilePath $log -Append
    $exitCode = $LASTEXITCODE
    "EXIT_CODE: $exitCode" | Tee-Object -FilePath $log -Append
    return $exitCode
}
```

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

Run each command from the repository root and record its result independently:

```powershell
$null = Invoke-LoggedCommand 'AUTO-FMT' { cargo fmt --all -- --check }
$null = Invoke-LoggedCommand 'AUTO-HEADLESS' { cargo test --locked -p rommfs-core -p rommfs-fixture -p rommfs-app --no-default-features -- --test-threads=1 }
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
$null = Invoke-LoggedCommand 'NATIVE-CANONICAL-EXPORT' { cargo test --locked -p rommfs-app --no-default-features --lib save_sync_agent::tests::canonicalized_export_parent_accepts_save_export -- --exact --nocapture --test-threads=1 }
$null = Invoke-LoggedCommand 'NATIVE-NO-CLOBBER-BOUNDARY' { cargo test --locked -p rommfs-app --no-default-features --lib save_sync_agent::files::tests::windows_publication_does_not_replace_a_file_created_at_the_boundary -- --exact --nocapture --test-threads=1 }
$null = Invoke-LoggedCommand 'NATIVE-FILESYSTEM-WATCHER' { cargo test --locked -p rommfs-app --no-default-features --lib save_sync_agent::tests::real_filesystem_notification_drives_the_agent_without_waiting_for_fallback_scan -- --exact --nocapture --test-threads=1 }
$null = Invoke-LoggedCommand 'PROFILE-VERSION-MARKERS' { cargo test --locked -p rommfs-core --lib save_sync::profile::tests::accepts_only_verified_retrobat_version_markers -- --exact --nocapture --test-threads=1 }
$null = Invoke-LoggedCommand 'PROFILE-STOCK-821' { cargo test --locked -p rommfs-core --lib save_sync::profile::tests::verifies_stock_821_gambatte_and_resolves_uncreated_saves_root -- --exact --nocapture --test-threads=1 }
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
- Write, append, delete, rename, hard-link, and file/directory creation fail.
- Open handles defer eviction; closing them permits dehydration and a new download.
- Unmount restores an empty folder; remount reuses the private cache.
- A nonempty mount root is refused without changing its local data.

The remaining tests exercise canonicalized paths, ancestor-junction rejection,
Windows sharing/rename protection, no-clobber save publication/export, and
actual filesystem notifications. The profile marker test accepts both
`8.2.1-stable-win64` (the shipped marker) and legacy `8.2.1`, and rejects
unsupported markers. These tests use disposable fixtures and do not verify a
live RomM server or a real RetroBat installation.

If a failure occurs, retain the first log. Re-run the failing test alone to
investigate; record both outcomes if it is intermittent. A build failure blocks
downstream runtime checks and must not be described as a failed runtime assertion.

## 3. Test a real RomM through CFAPI

Use the [local harness](romm-harness/README.md) with Docker Desktop running Linux
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
app-only request-log window immediately before the first **Connect**. Use this
connection for both `UI-MOUNT-CONNECT` and `UI-PROFILE-DEFAULT`; exclude the
earlier password grant and GET preflight window from all app request counts.

| Case | Action | Expected result |
| --- | --- | --- |
| `UI-MOUNT-CONNECT` | Enter the test URL and credentials; press **Connect**. | Connected state and catalogue appear; credentials are absent from logs. |
| `UI-MOUNT-START` | Enter the empty folder; press **Start**. | Platform folders and ROM filenames appear in Explorer and PowerShell. |
| `UI-MOUNT-LIST` | Run `Get-ChildItem -LiteralPath '<mount>' -Recurse` before opening a ROM. | Metadata appears. Use native fixture counters from section 2 as proof of no content downloads; Explorer may have other readers. |
| `UI-MOUNT-READ` | Run `Get-FileHash -LiteralPath '<mounted ROM>' -Algorithm SHA256`; compare with its known source. Repeat. | Hashes match; warm reads do not create a new ROM transfer. Hashing performs a content read. |
| `UI-MOUNT-STOP` | Press **Stop**, inspect the folder, then **Start** again. | Folder is restored empty; sibling `<folder>.rommfs-root` marker remains; cached bytes are reused. |
| `UI-MOUNT-CLOSE` | Close the app while mounted, then inspect the folder. | Mount stops and the empty folder returns; no tray/background app remains. |
| `UI-MOUNT-LEGACY-ROOT` | Put a sentinel file in a separate disposable folder; record its hash; attempt to mount there. | Mount is refused; sentinel and hash remain unchanged. Never test against a real old ROM/save library. |
| `UI-MOUNT-OWNERSHIP` | Claim a test folder for server A, stop, then try it for a different server identity. | Ownership mismatch is refused. A configured URL base path is part of server identity. |

Replace the quoted placeholders with actual paths. Record errors, screenshots,
and hashes. Do not remove ownership markers to bypass a rejection. The CFAPI
backend stores a private copy in `%LOCALAPPDATA%\rommfs\cache\<server>` and an
NTFS hydrated copy while mounted. Eviction must reclaim both. Clean Stop restores
an empty root and its original ACL; busy/changed entries defer cleanup. Never
remove ownership markers or run recursive cleanup to bypass a refusal.

Configure the disposable RetroBat installation's Game Boy ROM path to the mounted
`gb` folder using its supported configuration workflow. Record and restore any
tester-made configuration changes. RomMFS does not configure RetroBat itself.
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
before testing and compare after each case. RomMFS must not edit emulator
configuration. Keep backups untouched and restore only tester-made changes.

| Case | Action | Expected result |
| --- | --- | --- |
| `UI-PROFILE-DEFAULT` | Use the first connection from `UI-MOUNT-CONNECT`, captured in its clean-profile app-only request window, without enabling save sync. | Save sync is off; no app save API requests occur before enabling. Authentication/account verification may still occur. Exclude the separate `API-PREFLIGHT` window. |
| `UI-PROFILE-DISCOVERY` | Press **Refresh** with a real installation at fixed `X:\RetroBat`, and with an installation elsewhere while the actual RetroBat/EmulationStation is running. Test removable media only if a real removable volume is available. | Valid installations appear once per path; choose explicitly. Record unavailable removable media as `BLOCKED`, not as covered by a fixed-drive or stand-in process test. |
| `UI-PROFILE-BROWSE` | Use **Browse…** for an installation outside discovered locations. | Native picker selects the intended installation; cancelling preserves selection. |
| `UI-PROFILE-PREVIEW` | Select A without enabling. | Server, authenticated account, 8.2.1 profile, effective saves folder, mapped targets, supported/skipped existing files, and target paths are shown. |
| `UI-PROFILE-READONLY` | Refresh with a missing saves directory in a disposable install. | No saves folder is created. No save contents are opened/hashed by preview and no save API requests occur. Use fixture tests for proof beyond visible effects. |
| `UI-PROFILE-UNSUPPORTED` | Select an unsupported version or settings/core layout in a disposable install. | Explanation is shown; enabling is refused. Do not relabel a real installation to fake version compatibility. |
| `UI-PROFILE-SCOPE` | Change account, server, selected installation, or effective saves root. | Prior consent cannot authorize the changed scope. Confirm preview before any new opt-in. |
| `UI-PROFILE-MISSING` | Close the app; temporarily move the selected disposable install; restart and refresh. | Prior selection remains visible and paused; another installation is not silently selected. Restore it afterward. |
| `UI-PROFILE-DEBOUNCE` | Apply `0`, `3601`, `1.5`, and nonnumeric text; then apply `5` and restart. | Invalid values are rejected and not saved. Whole seconds 1–3600 are accepted; valid value persists for its scope. |
| `UI-KEYBOARD` | Follow the keyboard walkthrough below with the real app. | Record visual evidence for each required focus and activation behavior; source inspection is not a keyboard pass. |

### Keyboard walkthrough (`UI-KEYBOARD`)

Use screenshots or a short screen recording where focus is visibly indicated;
save them under `$Evidence\screenshots` and note the action and focus target.
Start with the initial app state and check the URL field has the expected initial
focus. Tab and Shift+Tab through URL/credential inputs and the enabled **Connect**,
**Start**, **Stop**, **Copy**, and save-sync controls/candidates, recording the
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

Only visible catalogue `.gb` games map to `gb/<visible ROM stem>.srm` under the
effective saves root. `.gbc`, `.rtc`, save states, memory cards, ambiguous names,
multi-file ROMs, and other profiles are excluded. Check exclusions in the automated
mapping/profile tests and any matching disposable entries available on this host.
Record skipped counts separately from mapped game counts. A bounded or unavailable
preview scan must be labelled partial/unavailable, not a complete zero-file scan.

## 6. Run a live save-sync smoke test

Use one disposable RomM account and the same playable Game Boy catalogue entry
for both installations. Record account ID, ROM ID, slot
`rommfs-retrobat-gb-srm-v1`, effective saves root, mapped `.srm` paths, and
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
| UI-MOUNT-* / UI-PROFILE-* | | | | | |
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

Use [SAVE-SYNC-CHECKLIST.md](../SAVE-SYNC-CHECKLIST.md) for acceptance contracts
and [README.md](../README.md) for supported behavior. Preserve historical
verification statements unless this run supplies evidence to update them.
