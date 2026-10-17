#requires -Version 5.1
# Dependency-free self-checks for the Windows test wizard's pure logic:
# status transitions, the native-command runner, manifest diffing/classification,
# redaction, state round-trips/atomic writes, result transitions with draft
# persistence, and the finish phase (report/ZIP, staging and failure). Dot-sources
# the wizard (its interactive entry is guarded) and never prompts. Exit code is
# non-zero on any failure.
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$script:Checks = 0
$script:Failures = 0

function Assert-True {
    param([bool]$Condition, [string]$Name)
    $script:Checks++
    if (-not $Condition) { $script:Failures++; Write-Host "FAIL: $Name" }
}

function Assert-Equal {
    param($Expected, $Actual, [string]$Name)
    $script:Checks++
    if ($Expected -ne $Actual) { $script:Failures++; Write-Host "FAIL: $Name (expected '$Expected', got '$Actual')" }
}

. (Join-Path $PSScriptRoot 'windows-test-wizard.ps1')

$scratch = Join-Path ([System.IO.Path]::GetTempPath()) ("rommfs-wizard-test-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch -Force | Out-Null

try {
    # Status transitions.
    Assert-Equal 'PASS' (Get-OverallStatus -Statuses @('PASS', 'PASS')) 'all pass is PASS'
    Assert-Equal 'UNVERIFIED' (Get-OverallStatus -Statuses @('PASS', 'UNVERIFIED')) 'unverified beats pass'
    Assert-Equal 'NOT RUN' (Get-OverallStatus -Statuses @('PASS', 'NOT RUN')) 'skip is not a pass'
    Assert-Equal 'BLOCKED' (Get-OverallStatus -Statuses @('BLOCKED', 'PARTIAL')) 'blocked beats partial'
    Assert-Equal 'FAIL' (Get-OverallStatus -Statuses @('PASS', 'FAIL', 'BLOCKED')) 'fail dominates'

    # Auto-command status: error -> BLOCKED, no exit -> UNVERIFIED, zero tests -> UNVERIFIED.
    $passCounts = Get-CargoTestCounts 'test result: ok. 3 passed; 0 failed; 0 ignored'
    $noneCounts = Get-CargoTestCounts 'Running unittests (target/debug/deps/x)'
    Assert-Equal 'PASS' (Get-AutoCommandStatus -ExitCode 0 -Error $null -Kind 'Test' -Counts $passCounts) 'exit 0 with tests is PASS'
    Assert-Equal 'UNVERIFIED' (Get-AutoCommandStatus -ExitCode 0 -Error $null -Kind 'Test' -Counts $noneCounts) 'zero tests is UNVERIFIED'
    Assert-Equal 'UNVERIFIED' (Get-AutoCommandStatus -ExitCode $null -Error $null -Kind 'Check' -Counts $noneCounts) 'no exit code is UNVERIFIED'
    Assert-Equal 'FAIL' (Get-AutoCommandStatus -ExitCode 1 -Error $null -Kind 'Test' -Counts $passCounts) 'non-zero exit is FAIL'
    Assert-Equal 'BLOCKED' (Get-AutoCommandStatus -ExitCode $null -Error 'not found' -Kind 'Check' -Counts $noneCounts) 'missing command is BLOCKED'
    Assert-Equal 'PASS' (Get-AutoCommandStatus -ExitCode 0 -Error $null -Kind 'Check' -Counts $noneCounts) 'check needs no tests'

    # Test-count parsing across multiple binaries.
    $counts = Get-CargoTestCounts "test result: ok. 5 passed; 0 failed; 1 ignored`ntest result: ok. 2 passed; 1 failed; 0 ignored"
    Assert-Equal 7 $counts.Executed 'sums executed'
    Assert-Equal 1 $counts.Failed 'sums failed'
    Assert-Equal 1 $counts.Ignored 'sums ignored'
    Assert-Equal $true $counts.Matched 'matched when a result line exists'
    Assert-Equal $false $noneCounts.Matched 'not matched without a result line'

    # Redaction.
    $redacted = Get-RedactedText "Authorization: Bearer abc.def.ghi`npassword=hunter2`ntoken=zzz`neyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.sig"
    Assert-True ($redacted -notmatch 'hunter2') 'redacts password'
    Assert-True ($redacted -notmatch 'abc\.def\.ghi') 'redacts bearer token'
    Assert-True ($redacted -notmatch 'zzz') 'redacts token value'
    Assert-True ($redacted -notmatch 'eyJhbGciOiJIUzI1NiJ9') 'redacts jwt'
    Assert-True ($redacted -match 'REDACTED') 'marks redaction'
    Assert-Equal 'ordinary log line' (Get-RedactedText 'ordinary log line') 'leaves ordinary text alone'

    # Deep JSON round-trip.
    $hashtable = ConvertTo-HashtableDeep ('{"a":{"b":[1,2]},"c":true}' | ConvertFrom-Json)
    Assert-Equal 1 $hashtable.a.b[0] 'deep conversion keeps nested array'
    Assert-Equal $true $hashtable.c 'deep conversion keeps scalars'

    # Native runner: exactly one dictionary result, 0/1/missing, stderr-with-success.
    $runnerDir = Join-Path $scratch 'runner'
    New-Item -ItemType Directory -Path (Join-Path $runnerDir 'commands') -Force | Out-Null
    $runnerState = New-WizardState -RunId 'r' -RepoRoot $scratch -EvidencePath $runnerDir -ZipPath (Join-Path $scratch 'r.zip')
    if ($env:OS -eq 'Windows_NT') {
        $exit0 = { & cmd.exe /c 'exit 0' }
        $exit3 = { & cmd.exe /c 'echo hello & exit 3' }
        $stderr0 = { & cmd.exe /c 'echo warn 1>&2 & exit 0' }
    } else {
        $exit0 = { & /bin/sh -c 'exit 0' }
        $exit3 = { & /bin/sh -c 'echo hello; exit 3' }
        $stderr0 = { & /bin/sh -c 'echo warn 1>&2; exit 0' }
    }
    $r0 = Invoke-WizardNativeCommand -State $runnerState -Id 'RUN-0' -Command $exit0
    Assert-True ($r0 -is [System.Collections.IDictionary]) 'runner returns exactly one dictionary'
    Assert-Equal 1 @($r0).Count 'runner returns a single object'
    Assert-True ($r0.ExitCode -is [int]) 'exit code is an int, not an array'
    Assert-Equal 0 $r0.ExitCode 'runner captures exit 0'
    Assert-Equal $null $r0.Error 'runner exit 0 has no error'
    $r3 = Invoke-WizardNativeCommand -State $runnerState -Id 'RUN-3' -Command $exit3
    Assert-True ($r3 -is [System.Collections.IDictionary]) 'runner exit 3 is one dictionary'
    Assert-Equal 3 $r3.ExitCode 'runner captures exit 3'
    $runLog = Get-Content -Raw -LiteralPath (Join-Path $runnerDir 'commands/RUN-3.txt')
    Assert-True ($runLog -match 'hello') 'runner logs live output'
    Assert-True ($runLog -match 'EXIT_CODE: 3') 'runner logs the exit code'
    $missing = Invoke-WizardNativeCommand -State $runnerState -Id 'RUN-MISSING' -Command { definitely-not-a-command-xyz }
    Assert-True ([bool]$missing.Error) 'missing command returns an error (BLOCKED)'
    Assert-Equal $null $missing.ExitCode 'missing command has no stale exit code'
    # Native stderr with a success exit must not be treated as a failure.
    $rw = Invoke-WizardNativeCommand -State $runnerState -Id 'RUN-STDERR' -Command $stderr0
    Assert-Equal 0 $rw.ExitCode 'stderr with success keeps exit 0'
    Assert-Equal $null $rw.Error 'stderr with success is not an error'
    Assert-True ((Get-Content -Raw -LiteralPath (Join-Path $runnerDir 'commands/RUN-STDERR.txt')) -match 'warn') 'stderr is captured in the log'

    # Hashing does not depend on Get-FileHash and all evidence paths use the
    # streaming helper.
    function Get-FileHash { throw 'Get-FileHash must not be called' }
    $known = Join-Path $scratch 'known.bin'
    [System.IO.File]::WriteAllBytes($known, [System.Text.Encoding]::ASCII.GetBytes('abc'))
    $expectedHash = 'BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD'
    Assert-Equal $expectedHash (Get-WizardFileSha256 -Path $known) 'streaming helper returns standard uppercase SHA256'
    $missingHashThrew = $false
    try { $null = Get-WizardFileSha256 -Path (Join-Path $scratch 'missing.bin') } catch { $missingHashThrew = $true }
    Assert-True $missingHashThrew 'missing file does not produce a successful hash'

    $hashEvidence = Join-Path $scratch 'hash-evidence'
    New-Item -ItemType Directory -Path (Join-Path $hashEvidence 'commands') -Force | Out-Null
    $hashState = New-WizardState -RunId 'hash' -RepoRoot $scratch -EvidencePath $hashEvidence -ZipPath (Join-Path $scratch 'hash.zip')
    $hashState.Paths.AppExe = $known
    $appEvidence = Write-AppEvidence -State $hashState
    Assert-Equal 'PASS' $appEvidence.Status 'app evidence hashes without Get-FileHash'
    Assert-True ((Get-Content -Raw -LiteralPath (Join-Path $hashEvidence 'commands/ENV-APP.txt')) -match $expectedHash) 'app evidence records expected SHA256'
    $locked = [System.IO.File]::Open($known, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::None)
    try {
        $unreadableApp = Write-AppEvidence -State $hashState
        Assert-Equal 'PARTIAL' $unreadableApp.Status 'unreadable app hash is not a pass'
        Assert-True ((Get-Content -Raw -LiteralPath (Join-Path $hashEvidence 'commands/ENV-APP.txt')) -match 'AppSha256=unavailable') 'unreadable app hash is recorded unavailable'
    } finally { $locked.Dispose() }

    $retro = Join-Path $scratch 'RetroBat'
    New-Item -ItemType Directory -Path (Join-Path $retro 'system'), (Join-Path $retro 'emulationstation') -Force | Out-Null
    foreach ($path in @((Join-Path $retro 'system/version.info'), (Join-Path $retro 'RetroBat.exe'), (Join-Path $retro 'emulationstation/emulationstation.exe'))) {
        [System.IO.File]::WriteAllBytes($path, [System.Text.Encoding]::ASCII.GetBytes('abc'))
    }
    $hashState.Paths.RetroBatRoot = $retro
    $retroEvidence = Write-RetroBatEvidence -State $hashState
    Assert-Equal 'PASS' $retroEvidence.Status 'RetroBat evidence hashes without Get-FileHash'
    Assert-Equal 3 ([regex]::Matches((Get-Content -Raw -LiteralPath (Join-Path $hashEvidence 'commands/ENV-RETROBAT.txt')), $expectedHash).Count) 'RetroBat evidence records all expected hashes'
    Remove-Item -LiteralPath (Join-Path $retro 'RetroBat.exe')
    $retroMissing = Write-RetroBatEvidence -State $hashState
    Assert-Equal 'PARTIAL' $retroMissing.Status 'missing RetroBat hash is not a pass'
    Assert-True ($retroMissing.Notes -match 'RetroBat\.exe missing') 'missing RetroBat hash has a precise limitation'
    $hashState.Results['ENV-HOST'] = @{ Status = 'PASS' }
    $hashState.Results['ENV-APP'] = @{ Status = 'PASS'; Notes = 'stale' }
    $hashState.Results['ENV-RETROBAT'] = @{ Status = 'PASS'; Notes = 'stale empty hashes' }
    Invoke-EnvironmentPhase -State $hashState
    Assert-Equal 'PARTIAL' $hashState.Results['ENV-RETROBAT'].Status 'resume refreshes stale RetroBat hash evidence'
    Assert-True ($hashState.Results['ENV-RETROBAT'].Notes -match 'RetroBat\.exe missing') 'resume retains the refreshed hash limitation'

    # Manifest hashing is bounded and skips the RomM link.
    $tree = Join-Path $scratch 'tree'
    New-Item -ItemType Directory -Path (Join-Path $tree 'roms/gb/RomM') -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $tree 'roms/gb/stock.gb') -Value 'stock'
    Set-Content -LiteralPath (Join-Path $tree 'roms/gb/RomM/owned.gb') -Value 'owned'
    $manifest = @(Get-WizardManifest -Root $tree -SubPaths @('roms'))
    Assert-True ($manifest[0] -match 'hashed=1') 'manifest hashes only stock files'
    Assert-True (($manifest -join "`n") -match '[0-9A-F]{64}') 'manifest uses the shared hash helper'
    Assert-True (($manifest -join "`n") -notmatch 'owned\.gb') 'manifest excludes the RomM folder'

    # A symlinked roms root is never traversed.
    $linkTree = Join-Path $scratch 'linktree'
    New-Item -ItemType Directory -Path (Join-Path $linkTree 'realroms/gb') -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $linkTree 'realroms/gb/stock.gb') -Value 'stock'
    $linkOk = $false
    try {
        New-Item -ItemType SymbolicLink -Path (Join-Path $linkTree 'roms') -Target (Join-Path $linkTree 'realroms') -ErrorAction Stop | Out-Null
        $linkOk = $true
    } catch { Write-Host 'SKIP: symbolic links unavailable for the base-root test' }
    if ($linkOk) {
        # @() keeps the single-element header array from being unwrapped to a scalar.
        $linkManifest = @(Get-WizardManifest -Root $linkTree -SubPaths @('roms'))
        Assert-True ($linkManifest[0] -match 'hashed=0') 'a symlinked roms root is not traversed'
        Assert-True ($linkManifest[0] -match 'skipped=1') 'the symlinked root is counted skipped'
    }

    # Manifest completeness and findings classification.
    Assert-Equal $false (Test-WizardManifestComplete -Manifest @('# hashed=0 skipped=0 truncated=False')) 'empty manifest is incomplete'
    Assert-Equal $false (Test-WizardManifestComplete -Manifest @('# hashed=3 skipped=1 truncated=False')) 'skipped manifest is incomplete'
    Assert-Equal $false (Test-WizardManifestComplete -Manifest @('# hashed=3 skipped=0 truncated=True')) 'truncated manifest is incomplete'
    Assert-Equal $true (Test-WizardManifestComplete -Manifest @('# hashed=3 skipped=0 truncated=False')) 'complete manifest is evidence'

    $hA = 'A' * 64; $hB = 'B' * 64; $hC = 'C' * 64
    $before = @('# hashed=4 skipped=0 truncated=False', "$hA  10  a.gb", "$hB  20  b.gb", "$hB  5  gone.gb")
    $after = @('# hashed=4 skipped=0 truncated=False', "$hA  10  a.gb", "$hC  21  b.gb", "$hC  30  new.gb")
    $diff = Compare-WizardManifest -Before $before -After $after
    Assert-Equal 1 $diff.Added.Count 'manifest counts added'
    Assert-Equal 1 $diff.Removed.Count 'manifest counts removed'
    Assert-Equal 1 $diff.Changed.Count 'manifest counts changed'
    Assert-True ($diff.Changed -contains 'b.gb') 'manifest reports the changed path'

    $diffWithGamelist = @{ Added = @(); Removed = @('roms\gb\gamelist.xml'); Changed = @('roms\gb\stock.gb', 'saves\gb\gamelist.xml') }
    $findings = Get-WizardManifestFindings -Diff $diffWithGamelist
    Assert-Equal 1 $findings.Contradicting.Count 'only the non-gamelist change contradicts'
    Assert-True ($findings.Contradicting -contains 'roms\gb\stock.gb') 'a stock change is contradicting'
    Assert-Equal 2 $findings.Gamelist.Count 'gamelist changes are expected, not contradicting'

    # Status adjustment: only a human PASS is downgraded, and gamelist merges are safe.
    $completeBefore = @('# hashed=2 skipped=0 truncated=False', "$hA  10  a.gb")
    $sameDiff = Compare-WizardManifest -Before $completeBefore -After $completeBefore
    Assert-Equal 'PASS' (Get-AdjustedManifestStatus -Status 'PASS' -Before $completeBefore -After $completeBefore -Diff $sameDiff).Status 'complete manifest keeps PASS'
    $changedAfter = @('# hashed=2 skipped=0 truncated=False', "$hC  10  a.gb")
    $changedDiff = Compare-WizardManifest -Before $completeBefore -After $changedAfter
    Assert-Equal 'FAIL' (Get-AdjustedManifestStatus -Status 'PASS' -Before $completeBefore -After $changedAfter -Diff $changedDiff).Status 'a changed original fails PASS'
    $glBefore = @('# hashed=2 skipped=0 truncated=False', "$hA  10  gamelist.xml")
    $glAfter = @('# hashed=2 skipped=0 truncated=False', "$hC  10  gamelist.xml")
    $glDiff = Compare-WizardManifest -Before $glBefore -After $glAfter
    $glAdj = Get-AdjustedManifestStatus -Status 'PASS' -Before $glBefore -After $glAfter -Diff $glDiff
    Assert-Equal 'PASS' $glAdj.Status 'a gamelist change keeps PASS'
    Assert-True (($glAdj.Notes -join ' ') -match 'gamelist') 'the gamelist merge is noted'
    $truncBefore = @('# hashed=2 skipped=0 truncated=True', "$hA  10  a.gb")
    $truncDiff = Compare-WizardManifest -Before $truncBefore -After $truncBefore
    Assert-Equal 'PARTIAL' (Get-AdjustedManifestStatus -Status 'PASS' -Before $truncBefore -After $truncBefore -Diff $truncDiff).Status 'a truncated manifest downgrades PASS'
    Assert-Equal 'PARTIAL' (Get-AdjustedManifestStatus -Status 'PASS' -Before $null -After $null -Diff $null).Status 'no manifest downgrades PASS'
    Assert-Equal 'NOT RUN' (Get-AdjustedManifestStatus -Status 'NOT RUN' -Before $completeBefore -After $completeBefore -Diff $sameDiff).Status 'a skip is not upgraded'

    # Result transitions from the interactive prompt, with Read-Host scripted.
    $script:scriptedKeys = @()
    $script:keyIndex = 0
    function Read-Host {
        param([string]$Prompt)
        $value = $script:scriptedKeys[$script:keyIndex]
        $script:keyIndex++
        return $value
    }
    function Get-StepResultFor {
        param([string[]]$Keys)
        $script:scriptedKeys = $Keys
        $script:keyIndex = 0
        $dir = Join-Path $scratch ("step-" + [guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
        $state = New-WizardState -RunId 'r' -RepoRoot $scratch -EvidencePath $dir -ZipPath (Join-Path $dir 'x.zip')
        return Read-StepResult -State $state -Step @{ Id = 'X' }
    }

    Assert-Equal 'PASS' (Get-StepResultFor @('')).Status 'Enter is Pass'
    Assert-Equal 'FAIL' (Get-StepResultFor @('f')).Status 'F is Fail'
    Assert-Equal 'BLOCKED' (Get-StepResultFor @('b')).Status 'B is Blocked'
    Assert-Equal 'NOT RUN' (Get-StepResultFor @('s')).Status 'S is a skip, not a pass'
    Assert-Equal 'PARTIAL' (Get-StepResultFor @('p')).Status 'P is Partial'
    Assert-Equal 'UNVERIFIED' (Get-StepResultFor @('u')).Status 'U is Unverified'
    Assert-Equal $null (Get-StepResultFor @('q')) 'Q returns null to save and quit'
    $noted = Get-StepResultFor @('n', 'a note', '')
    Assert-Equal 'a note' $noted.Notes 'note is kept'
    Assert-Equal 'PASS' $noted.Status 'note then Pass'

    # A note is persisted as a draft before the step is completed, and the draft is
    # cleared once a result is chosen.
    $draftDir = Join-Path $scratch 'draft'
    New-Item -ItemType Directory -Path $draftDir -Force | Out-Null
    $draftState = New-WizardState -RunId 'r' -RepoRoot $scratch -EvidencePath $draftDir -ZipPath (Join-Path $draftDir 'x.zip')
    $script:scriptedKeys = @('n', 'draft note', 'q')
    $script:keyIndex = 0
    Assert-Equal $null (Read-StepResult -State $draftState -Step @{ Id = 'X' }) 'Q after a note returns null'
    Assert-True ($draftState.Drafts.Contains('X')) 'draft note persisted before quit'
    Assert-True ((Get-Content -Raw -LiteralPath (Join-Path $draftDir 'wizard-state.json')) -match 'draft note') 'draft note written to disk'
    $script:scriptedKeys = @('')
    $script:keyIndex = 0
    $resumed = Read-StepResult -State $draftState -Step @{ Id = 'X' }
    Assert-Equal 'draft note' $resumed.Notes 'draft note preloaded on resume'
    Assert-True (-not $draftState.Drafts.Contains('X')) 'draft removed once completed'

    # State file is written atomically (no leftover temp file).
    $stateDir = Join-Path $scratch 'state'
    New-Item -ItemType Directory -Path $stateDir -Force | Out-Null
    $state = New-WizardState -RunId 'test-run' -RepoRoot $scratch -EvidencePath $stateDir -ZipPath (Join-Path $scratch 'out.zip')
    $state.Paths.AppExe = 'C:\path\rommfs-app.exe'
    $state.Results['SETUP-OWNERSHIP'] = @{ Status = 'PASS'; Notes = 'ok'; Evidence = @('a.txt'); AtUtc = 'now' }
    Assert-True ($state.Results.Contains('SETUP-OWNERSHIP')) 'in-memory results supports Contains'
    Save-WizardState -State $state
    Assert-True (Test-Path -LiteralPath (Join-Path $stateDir 'wizard-state.json')) 'state file written'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $stateDir 'wizard-state.json.tmp'))) 'no temp state file left'

    $loaded = Read-WizardState -Path (Join-Path $stateDir 'wizard-state.json')
    Assert-Equal 'C:\path\rommfs-app.exe' $loaded.Paths.AppExe 'state keeps path'
    Assert-Equal 'PASS' $loaded.Results['SETUP-OWNERSHIP'].Status 'state keeps result'
    Assert-True ($loaded.Results.Contains('SETUP-OWNERSHIP')) 'reloaded results supports Contains'

    # Unrecorded steps stay NOT RUN and cannot give an overall PASS.
    $overall = Write-WizardReport -State $loaded -Path (Join-Path $stateDir 'report.md')
    Assert-True ($overall -ne 'PASS') 'overall is not PASS with unrecorded steps'
    $report = Get-Content -Raw -LiteralPath (Join-Path $stateDir 'report.md')
    Assert-True ($report -match 'NOT RUN') 'report shows NOT RUN'
    Assert-True ($report -match 'never uploaded automatically') 'report warns about sharing'

    # Finish phase: no logs -> NOT RUN; packaging is PASS in the report and inside
    # the ZIP (the ZIP is not falsely reported as NOT RUN).
    $env:LOCALAPPDATA = Join-Path $scratch 'appdata'
    New-Item -ItemType Directory -Path $env:LOCALAPPDATA -Force | Out-Null
    $finishDir = Join-Path $scratch 'finish'
    New-Item -ItemType Directory -Path (Join-Path $finishDir 'commands') -Force | Out-Null
    $finishState = New-WizardState -RunId 'finish' -RepoRoot $scratch -EvidencePath $finishDir -ZipPath (Join-Path $scratch 'finish.zip')
    Assert-Equal 'PASS' (Invoke-FinishPhase -State $finishState) 'packaging returns PASS'
    Assert-Equal 'NOT RUN' $finishState.Results['LOGS-COLLECT'].Status 'no logs is NOT RUN, not PASS'
    Assert-True ($finishState.Results.Contains('ENV-FINAL-STATUS')) 'finish records the final git status'
    Assert-Equal 'FAIL' $finishState.Results['ENV-FINAL-STATUS'].Status 'nonzero final git status command is FAIL'
    $finishReport = Get-Content -Raw -LiteralPath (Join-Path $finishDir 'report.md')
    Assert-True ($finishReport -match '\| EVIDENCE-ZIP \| PASS') 'report shows ZIP PASS, not NOT RUN'
    Assert-True (Test-Path -LiteralPath (Join-Path $scratch 'finish.zip')) 'zip created'
    $extract = Join-Path $scratch 'finish-extract'
    Expand-Archive -LiteralPath (Join-Path $scratch 'finish.zip') -DestinationPath $extract
    $innerReport = Get-Content -Raw -LiteralPath (Join-Path $extract 'report.md')
    Assert-True ($innerReport -match '\| EVIDENCE-ZIP \| PASS') 'zip contains a report that shows ZIP PASS'

    # Archive failure must not report success and must keep the previous ZIP.
    $oldZip = Join-Path $scratch 'old.zip'
    Compress-Archive -Path (Join-Path $finishDir '*') -DestinationPath $oldZip -ErrorAction Stop
    function Compress-Archive {
        [CmdletBinding()]
        param($Path, $DestinationPath, $CompressionLevel)
        throw 'archive boom'
    }
    $threw = $false
    try { New-EvidenceZip -EvidencePath $finishDir -ZipPath $oldZip } catch { $threw = $true }
    Assert-True $threw 'archive failure throws'
    Assert-True (Test-Path -LiteralPath $oldZip) 'previous zip is preserved on failure'
    Assert-True (-not (Test-Path -LiteralPath "$oldZip.tmp.zip")) 'no temp zip left on failure'

    $failDir = Join-Path $scratch 'finishfail'
    New-Item -ItemType Directory -Path (Join-Path $failDir 'commands') -Force | Out-Null
    $failState = New-WizardState -RunId 'fail' -RepoRoot $scratch -EvidencePath $failDir -ZipPath (Join-Path $scratch 'finishfail.zip')
    Assert-Equal 'FAIL' (Invoke-FinishPhase -State $failState) 'packaging failure returns FAIL'
    Assert-Equal 'FAIL' $failState.Results['EVIDENCE-ZIP'].Status 'zip step is FAIL on failure'
    Assert-Equal $null $failState.Finish.ZipPath 'no zip path on failure'
    Assert-True ((Get-Content -Raw -LiteralPath (Join-Path $failDir 'report.md')) -match '\| EVIDENCE-ZIP \| FAIL') 'report shows packaging FAIL'
} finally {
    Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue
}

if ($script:Failures -gt 0) {
    Write-Host "$($script:Failures) of $($script:Checks) checks failed."
    exit 1
}
Write-Host "All $($script:Checks) checks passed."
exit 0
