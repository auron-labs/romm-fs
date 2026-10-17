<#
.SYNOPSIS
    Windows-native RomMFS human-testing wizard.

.DESCRIPTION
    Walks the remaining human tests from docs/testing/windows.md as focused guided
    steps, gathers evidence automatically where it is a read, and asks the operator
    for a result per step. It never marks an unseen step as a pass and never gives
    an overall PASS while a step is skipped, blocked, partial or unverified.
    Reads are automatic (host metadata, executable hashes, read-only stock-file
    manifests, log copies). Everything that could change your games, saves,
    emulator configuration, drivers, the registry or the checkout is left to you.
    Screenshots need explicit consent because they capture the full desktop.

.EXAMPLE
    .\test-wizard.cmd
    .\scripts\windows-test-wizard.ps1 -Help
#>
#requires -Version 5.1
[CmdletBinding()]
param(
    [switch]$Help
)

# --- Result model -------------------------------------------------------------

# Worst-first rank so an overall status is the most serious step status. Index in
# the array below matches the rank value.
$script:StatusRank = @{
    'PASS' = 0; 'UNVERIFIED' = 1; 'PARTIAL' = 2; 'NOT RUN' = 3; 'BLOCKED' = 4; 'FAIL' = 5
}
$script:StatusByRank = @('PASS', 'UNVERIFIED', 'PARTIAL', 'NOT RUN', 'BLOCKED', 'FAIL')

function Get-OverallStatus {
    param([string[]]$Statuses)
    $worst = 0
    foreach ($status in $Statuses) {
        if ($script:StatusRank.ContainsKey($status) -and $script:StatusRank[$status] -gt $worst) {
            $worst = $script:StatusRank[$status]
        }
    }
    return $script:StatusByRank[$worst]
}

function Get-CargoTestCounts {
    param([string]$Text)
    $executed = 0
    $failed = 0
    $ignored = 0
    $matched = $false
    if ($Text) {
        foreach ($match in [regex]::Matches($Text, 'test result:\s*\w+\.\s*(\d+)\s*passed;\s*(\d+)\s*failed;\s*(\d+)\s*ignored')) {
            $matched = $true
            $executed += [int]$match.Groups[1].Value
            $failed += [int]$match.Groups[2].Value
            $ignored += [int]$match.Groups[3].Value
        }
    }
    return [ordered]@{ Executed = $executed; Failed = $failed; Ignored = $ignored; Matched = $matched }
}

function Get-AutoCommandStatus {
    param($ExitCode, [string]$Error, [string]$Kind, $Counts)
    if ($Error) { return 'BLOCKED' }
    if ($null -eq $ExitCode) { return 'UNVERIFIED' }
    if ($ExitCode -ne 0) { return 'FAIL' }
    if ($Kind -eq 'Test' -and (-not $Counts.Matched -or $Counts.Executed -eq 0)) {
        # A command that ran zero tests is not a pass.
        return 'UNVERIFIED'
    }
    return 'PASS'
}

function Get-RedactedText {
    param([string]$Text)
    if ([string]::IsNullOrEmpty($Text)) { return $Text }
    # Best-effort: obvious credential shapes only. Logs and screenshots can still
    # contain private information, so the report warns the operator to review the
    # ZIP before sharing it.
    $patterns = @(
        '(?im)^(authorization|cookie|set-cookie|x-api-key|proxy-authorization)\s*:.*$',
        '(?i)\bbearer\s+[A-Za-z0-9._\-]+',
        '(?i)\b(access_token|refresh_token|password|passwd|secret|api_key|apikey|token)\b\s*[=:]\s*[^\s"'',;]+',
        '(?i)eyJ[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+'
    )
    foreach ($pattern in $patterns) {
        $Text = [regex]::Replace($Text, $pattern, '[REDACTED]')
    }
    return $Text
}

# --- State --------------------------------------------------------------------

function ConvertTo-HashtableDeep {
    param($InputObject)
    if ($null -eq $InputObject) { return $null }
    if ($InputObject -is [System.Collections.IDictionary]) {
        $result = [ordered]@{}
        foreach ($key in $InputObject.Keys) { $result[$key] = ConvertTo-HashtableDeep $InputObject[$key] }
        return $result
    }
    if ($InputObject -is [System.Management.Automation.PSCustomObject]) {
        $result = [ordered]@{}
        foreach ($property in $InputObject.PSObject.Properties) { $result[$property.Name] = ConvertTo-HashtableDeep $property.Value }
        return $result
    }
    if ($InputObject -is [System.Collections.IEnumerable] -and $InputObject -isnot [string]) {
        return @($InputObject | ForEach-Object { ConvertTo-HashtableDeep $_ })
    }
    return $InputObject
}

function New-WizardState {
    param(
        [string]$RunId,
        [string]$RepoRoot,
        [string]$EvidencePath,
        [string]$ZipPath
    )
    return [ordered]@{
        RunId             = $RunId
        StartedUtc        = (Get-Date).ToUniversalTime().ToString('o')
        RepoRoot          = $RepoRoot
        EvidencePath      = $EvidencePath
        ZipPath           = $ZipPath
        Paths             = [ordered]@{ AppExe = $null; RetroBatRoot = $null }
        ScreenshotConsent = $false
        Launched          = @()
        Results           = [ordered]@{}
        Drafts            = [ordered]@{}
        Finish            = [ordered]@{ ReportPath = $null; ZipPath = $null }
    }
}

function Save-WizardState {
    param($State)
    # Write to a temp file and rename so a Ctrl+C mid-write cannot truncate the
    # previous good state.
    $path = Join-Path $State.EvidencePath 'wizard-state.json'
    $temp = "$path.tmp"
    $State | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $temp -Encoding UTF8
    Move-Item -LiteralPath $temp -Destination $path -Force
}

function Read-WizardState {
    param([string]$Path)
    $state = ConvertTo-HashtableDeep (Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json)
    if (-not $state.Drafts) { $state.Drafts = [ordered]@{} }
    return $state
}

function Find-LastWizardRun {
    param([string]$Root)
    if (-not (Test-Path -LiteralPath $Root)) { return $null }
    $candidate = Get-ChildItem -LiteralPath $Root -Directory -ErrorAction SilentlyContinue |
        Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'wizard-state.json') } |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1
    if ($candidate) { return $candidate.FullName }
    return $null
}

function Write-EvidenceFile {
    param($State, [string]$Name, [string]$Text)
    $path = Join-Path $State.EvidencePath $Name
    [System.IO.File]::WriteAllText($path, (Get-RedactedText $Text))
}

function Get-WizardFileSha256 {
    param([string]$Path)
    $stream = $null
    $sha256 = $null
    try {
        $stream = [System.IO.File]::OpenRead($Path)
        $sha256 = [System.Security.Cryptography.SHA256]::Create()
        return [System.BitConverter]::ToString($sha256.ComputeHash($stream)).Replace('-', '')
    } finally {
        if ($sha256) { $sha256.Dispose() }
        if ($stream) { $stream.Dispose() }
    }
}

# --- Discovery and pickers ----------------------------------------------------

function Resolve-AppExecutable {
    param([string]$RepoRoot)
    foreach ($name in 'target\debug\rommfs-app.exe', 'target\release\rommfs-app.exe') {
        $candidate = Join-Path $RepoRoot $name
        if (Test-Path -LiteralPath $candidate -PathType Leaf) { return $candidate }
    }
    return $null
}

function Resolve-RetroBatRoot {
    $roots = @()
    try {
        foreach ($drive in [System.IO.DriveInfo]::GetDrives()) {
            if (-not $drive.IsReady) { continue }
            if ($drive.DriveType -eq [System.IO.DriveType]::Fixed -or $drive.DriveType -eq [System.IO.DriveType]::Removable) {
                $roots += (Join-Path $drive.RootDirectory.FullName 'RetroBat')
            }
        }
    } catch { }
    foreach ($root in $roots) {
        if ((Test-Path -LiteralPath (Join-Path $root 'system\version.info') -PathType Leaf) -or
            (Test-Path -LiteralPath (Join-Path $root 'RetroBat.exe') -PathType Leaf)) {
            return $root
        }
    }
    return $null
}

function Select-WizardPath {
    param([string]$Title, [switch]$Folder, [string]$Filter, [string]$Initial)
    Add-Type -AssemblyName System.Windows.Forms | Out-Null
    # OpenFileDialog has no Description property; Title is the caption for both.
    $dialog = if ($Folder) { New-Object System.Windows.Forms.FolderBrowserDialog } else { New-Object System.Windows.Forms.OpenFileDialog }
    try {
        $dialog.Title = $Title
        if ($Folder) {
            $dialog.Description = $Title
            $dialog.ShowNewFolderButton = $true
            if ($Initial -and (Test-Path -LiteralPath $Initial)) { $dialog.SelectedPath = $Initial }
        } else {
            $dialog.Filter = $Filter
            if ($Initial -and (Test-Path -LiteralPath $Initial)) { $dialog.InitialDirectory = $Initial }
        }
        if ($dialog.ShowDialog() -ne [System.Windows.Forms.DialogResult]::OK) { return $null }
        if ($Folder) { return $dialog.SelectedPath }
        return $dialog.FileName
    } finally { $dialog.Dispose() }
}

# --- Native command runner ----------------------------------------------------

function Invoke-WizardNativeCommand {
    param($State, [string]$Id, [scriptblock]$Command)
    $log = Join-Path $State.EvidencePath "commands\$Id.txt"
    "COMMAND: $Command" | Set-Content -LiteralPath $log
    $global:LASTEXITCODE = 0
    # Native stderr under a caller's ErrorActionPreference=Stop must not abort the
    # run; a command can warn on stderr and still succeed.
    $ErrorActionPreference = 'Continue'
    try {
        # Tee writes the combined stream to the log; Out-Host keeps it live on the
        # host but off the pipeline, so this function returns exactly one object.
        & $Command 2>&1 | Tee-Object -FilePath $log -Append | Out-Host
    } catch {
        $message = $_.Exception.Message
        Add-Content -LiteralPath $log -Value "ERROR: $message"
        Write-Host "ERROR: $message"
        return [ordered]@{ ExitCode = $null; Error = $message }
    }
    # $LASTEXITCODE after a native command is an int; capture it before anything
    # else can overwrite it. A scriptblock that ran no native command leaves it 0.
    $code = $LASTEXITCODE
    Add-Content -LiteralPath $log -Value "EXIT_CODE: $code"
    return [ordered]@{ ExitCode = $code; Error = $null }
}

# --- Host and install metadata ------------------------------------------------

function Get-HostMetadata {
    param($State)
    $lines = @()
    try {
        $os = Get-CimInstance Win32_OperatingSystem -ErrorAction Stop
        $lines += "OS=$($os.Caption) version=$($os.Version) build=$($os.BuildNumber) arch=$($os.OSArchitecture)"
    } catch { $lines += "OS=unavailable ($($_.Exception.Message))" }
    try {
        $principal = New-Object System.Security.Principal.WindowsPrincipal([System.Security.Principal.WindowsIdentity]::GetCurrent())
        $elevated = $principal.IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator)
        $lines += "PowerShellElevated=$elevated"
    } catch { $lines += "PowerShellElevated=unavailable ($($_.Exception.Message))" }
    try {
        $cldflt = Get-Service -Name CldFlt -ErrorAction Stop
        $lines += "CldFlt=$($cldflt.Status)"
    } catch { $lines += "CldFlt=unavailable ($($_.Exception.Message))" }
    $lines += "PowerShellVersion=$($PSVersionTable.PSVersion)"
    $lines += "RepoRoot=$($State.RepoRoot)"
    $lines += "EvidenceRoot=$($State.EvidencePath)"
    $commit = Invoke-WizardNativeCommand -State $State -Id 'ENV-COMMIT' -Command { git rev-parse HEAD }
    $status = Invoke-WizardNativeCommand -State $State -Id 'ENV-GIT-STATUS' -Command { git status --short }
    $lines += "CheckoutCommitExit=$($commit.ExitCode) (see commands/ENV-COMMIT.txt)"
    $lines += "InitialGitStatus=$($status.ExitCode) (see commands/ENV-GIT-STATUS.txt)"
    Write-EvidenceFile -State $State -Name 'ENV-HOST.txt' -Text (($lines -join [Environment]::NewLine) + [Environment]::NewLine)
}

function Write-AppEvidence {
    param($State)
    $app = $State.Paths.AppExe
    if (-not $app -or -not (Test-Path -LiteralPath $app -PathType Leaf)) {
        return @{ Status = 'NOT RUN'; Notes = 'rommfs-app.exe was not located'; Evidence = @() }
    }
    $item = Get-Item -LiteralPath $app
    $status = 'PASS'
    $notes = @()
    try {
        $hash = Get-WizardFileSha256 -Path $app
    } catch {
        $hash = 'unavailable'
        $status = 'PARTIAL'
        $notes += "app hash unavailable: $($_.Exception.Message)"
    }
    $text = @(
        "AppExe=$app"
        "AppSha256=$hash"
        "AppLastWriteUtc=$($item.LastWriteTimeUtc.ToString('o'))"
        $(if ($notes) { "HashLimitations=$($notes -join '; ')" })
        'Note=The executable hash and timestamp are recorded separately from the checkout commit;'
        'Note=a matching commit does not prove the executable was built from it.'
    ) | Where-Object { $null -ne $_ }
    $text = $text -join [Environment]::NewLine
    Write-EvidenceFile -State $State -Name 'commands\ENV-APP.txt' -Text ($text + [Environment]::NewLine)
    if (-not $notes) { $notes = @("sha256=$hash") }
    return @{ Status = $status; Notes = ($notes -join '; '); Evidence = @('commands/ENV-APP.txt') }
}

function Write-RetroBatEvidence {
    param($State)
    $root = $State.Paths.RetroBatRoot
    if (-not $root -or -not (Test-Path -LiteralPath $root)) {
        return @{ Status = 'NOT RUN'; Notes = 'RetroBat root was not located'; Evidence = @() }
    }
    $limitations = New-Object System.Collections.Generic.List[string]
    $hashOf = {
        param($path, $label)
        if (-not (Test-Path -LiteralPath $path -PathType Leaf -ErrorAction SilentlyContinue)) {
            $limitations.Add("$label missing") | Out-Null
            return 'unavailable'
        }
        try { return Get-WizardFileSha256 -Path $path }
        catch {
            $limitations.Add("$label hash unavailable: $($_.Exception.Message)") | Out-Null
            return 'unavailable'
        }
    }
    $versionInfo = Join-Path $root 'system\version.info'
    $esExe = Join-Path $root 'emulationstation\emulationstation.exe'
    $versionText = 'unavailable'
    if (Test-Path -LiteralPath $versionInfo -PathType Leaf -ErrorAction SilentlyContinue) {
        try { $versionText = (Get-Content -Raw -LiteralPath $versionInfo -ErrorAction Stop).Trim() }
        catch { $limitations.Add("system/version.info text unavailable: $($_.Exception.Message)") | Out-Null }
    }
    $esVersion = 'unavailable'
    if (Test-Path -LiteralPath $esExe -PathType Leaf -ErrorAction SilentlyContinue) {
        try { $esVersion = (Get-Item -LiteralPath $esExe -ErrorAction Stop).VersionInfo.FileVersion }
        catch { $limitations.Add("emulationstation.exe version unavailable: $($_.Exception.Message)") | Out-Null }
    }
    $lines = @(
        "RetroBatRoot=$root"
        "VersionInfoText=$versionText"
        "VersionInfoSha256=$(& $hashOf $versionInfo 'system/version.info')"
        "RetroBatExeSha256=$(& $hashOf (Join-Path $root 'RetroBat.exe') 'RetroBat.exe')"
        "EmulationStationVersion=$esVersion"
        "EmulationStationSha256=$(& $hashOf $esExe 'emulationstation.exe')"
        $(if ($limitations.Count) { "HashLimitations=$($limitations -join '; ')" })
        'Note=Recorded from the real install. Missing fields are recorded as unavailable, never inferred.'
    ) | Where-Object { $null -ne $_ }
    Write-EvidenceFile -State $State -Name 'commands\ENV-RETROBAT.txt' -Text (($lines -join [Environment]::NewLine) + [Environment]::NewLine)
    if ($limitations.Count) {
        return @{ Status = 'PARTIAL'; Notes = ($limitations -join '; '); Evidence = @('commands/ENV-RETROBAT.txt') }
    }
    return @{ Status = 'PASS'; Notes = 'real install metadata recorded and hashed'; Evidence = @('commands/ENV-RETROBAT.txt') }
}

# --- Read-only stock-file manifests -------------------------------------------

function Test-WizardHashableFile {
    param($File)
    $attributes = [int]$File.Attributes
    if ($attributes -band [int][System.IO.FileAttributes]::ReparsePoint) { return $false }
    # OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS: never read a placeholder.
    $placeholder = 0x1000 -bor 0x40000 -bor 0x400000
    if ($attributes -band $placeholder) { return $false }
    return $true
}

function Test-WizardTraversableDirectory {
    param([string]$Path)
    try {
        $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
        if (-not $item.PSIsContainer) { return $false }
        $attributes = [int]$item.Attributes
        if ($attributes -band [int][System.IO.FileAttributes]::ReparsePoint) { return $false }
        $placeholder = 0x1000 -bor 0x40000 -bor 0x400000
        if ($attributes -band $placeholder) { return $false }
        return $true
    } catch { return $false }
}

function Get-WizardManifest {
    param([string]$Root, [string[]]$SubPaths, [int]$MaxFiles = 500)
    $entries = @()
    $skipped = 0
    $truncated = $false
    foreach ($sub in $SubPaths) {
        $base = Join-Path $Root $sub
        if (-not (Test-Path -LiteralPath $base)) { continue }
        # The roms/saves root itself can be a link or placeholder; never traverse it.
        if (-not (Test-WizardTraversableDirectory -Path $base)) { $skipped++; continue }
        $stack = New-Object System.Collections.Stack
        $stack.Push($base)
        while ($stack.Count -gt 0 -and -not $truncated) {
            $directory = $stack.Pop()
            foreach ($item in Get-ChildItem -LiteralPath $directory -Force -ErrorAction SilentlyContinue) {
                if ($item.Name -like '.rommfs-*') { continue }
                if ($item.PSIsContainer) {
                    if ($item.Name -ieq 'RomM') { continue }
                    # Never follow a link/junction (e.g. RomM -> games-folder link).
                    if (-not (Test-WizardTraversableDirectory -Path $item.FullName)) { $skipped++; continue }
                    $stack.Push($item.FullName)
                    continue
                }
                if (-not (Test-WizardHashableFile -File $item)) { $skipped++; continue }
                if ($entries.Count -ge $MaxFiles) { $truncated = $true; break }
                try { $hash = Get-WizardFileSha256 -Path $item.FullName }
                catch { $skipped++; continue }
                $relative = $item.FullName.Substring($Root.Length).TrimStart('\', '/')
                $entries += ('{0}  {1}  {2}' -f $hash, $item.Length, $relative)
            }
        }
        if ($truncated) { break }
    }
    $header = '# hashed={0} skipped={1} truncated={2}' -f $entries.Count, $skipped, $truncated
    return @($header) + ($entries | Sort-Object)
}

function Test-WizardManifestComplete {
    param([string[]]$Manifest)
    if (-not $Manifest -or $Manifest.Count -eq 0) { return $false }
    if ($Manifest[0] -notmatch 'hashed=(\d+)\s+skipped=(\d+)\s+truncated=(\w+)') { return $false }
    if ([int]$Matches[1] -eq 0) { return $false }
    if ([int]$Matches[2] -gt 0) { return $false }
    if ($Matches[3] -ieq 'True') { return $false }
    return $true
}

function Compare-WizardManifest {
    param([string[]]$Before, [string[]]$After)
    $parse = {
        param($lines)
        $map = @{}
        foreach ($line in $lines) {
            if ($line -match '^([0-9A-Fa-f]{64})\s+(\d+)\s+(.*)$') { $map[$Matches[3]] = $Matches[1] }
        }
        return $map
    }
    $beforeMap = & $parse $Before
    $afterMap = & $parse $After
    $added = @($afterMap.Keys | Where-Object { -not $beforeMap.ContainsKey($_) })
    $removed = @($beforeMap.Keys | Where-Object { -not $afterMap.ContainsKey($_) })
    $changed = @($afterMap.Keys | Where-Object { $beforeMap.ContainsKey($_) -and $beforeMap[$_] -ne $afterMap[$_] })
    return [ordered]@{ Added = $added; Removed = $removed; Changed = $changed; BeforeCount = $beforeMap.Count; AfterCount = $afterMap.Count }
}

function Get-WizardManifestFindings {
    param($Diff)
    # gamelist.xml is expected to be merged; a changed/removed original there is not
    # a contradiction (the operator checks their own entries manually).
    $contradicting = @()
    $gamelist = @()
    foreach ($path in @($Diff.Changed) + @($Diff.Removed)) {
        if (($path -split '[\\/]')[-1] -ieq 'gamelist.xml') { $gamelist += $path } else { $contradicting += $path }
    }
    return [ordered]@{ Contradicting = $contradicting; Gamelist = $gamelist }
}

function Get-AdjustedManifestStatus {
    param([string]$Status, $Before, $After, $Diff)
    # A changed/removed non-gamelist original contradicts "byte-identical"; a
    # missing, empty or incomplete manifest is not evidence; gamelist merges are
    # expected. Only a human PASS is downgraded.
    $notes = @()
    if (-not $Before) {
        if ($Status -eq 'PASS') { $Status = 'PARTIAL'; $notes += 'no manifest evidence; byte-identical not proven' }
        return [ordered]@{ Status = $Status; Notes = $notes }
    }
    $findings = Get-WizardManifestFindings -Diff $Diff
    if ($findings.Contradicting.Count -and $Status -eq 'PASS') {
        $Status = 'FAIL'
        $notes += "stock files changed/removed: $($findings.Contradicting -join ', ')"
    }
    if (-not ((Test-WizardManifestComplete -Manifest $Before) -and (Test-WizardManifestComplete -Manifest $After)) -and $Status -eq 'PASS') {
        $Status = 'PARTIAL'
        $notes += 'manifest incomplete or empty; byte-identical not proven'
    }
    if ($findings.Gamelist.Count) { $notes += 'gamelist.xml merged as expected; verify your own entries manually' }
    return [ordered]@{ Status = $Status; Notes = $notes }
}

function Get-StepManifest {
    param($State, $Step, [string]$When)
    if (-not $Step.Manifest -or -not $State.Paths.RetroBatRoot) { return $null }
    $file = Join-Path $State.EvidencePath "harness\$($Step.Id)-$When.txt"
    if ($When -eq 'before' -and (Test-Path -LiteralPath $file)) {
        # Reuse the original baseline so a resume cannot hide a change.
        return @(Get-Content -LiteralPath $file)
    }
    $manifest = Get-WizardManifest -Root $State.Paths.RetroBatRoot -SubPaths $Step.Manifest
    Set-Content -LiteralPath $file -Value $manifest
    return $manifest
}

# --- Evidence actions ---------------------------------------------------------

function Open-WizardApp {
    param($State)
    if ($State.Launched -contains 'App') { return }
    if ($State.Paths.AppExe -and (Test-Path -LiteralPath $State.Paths.AppExe)) {
        try {
            Start-Process -FilePath $State.Paths.AppExe
            $State.Launched += 'App'
            Write-Host "Launched $($State.Paths.AppExe)"
        } catch { Write-Host "Could not launch the app: $($_.Exception.Message)" }
    } else {
        Write-Host 'No app executable path; launch it yourself.'
    }
}

function Save-WizardScreenshot {
    param($State, [string]$CaseId)
    if (-not $State.ScreenshotConsent) {
        Write-Host ''
        Write-Host 'Screenshot consent: this captures your FULL DESKTOP (all monitors).'
        Write-Host 'The image may contain private information. It is saved only to your evidence'
        Write-Host 'folder and is never uploaded. Captures happen only when you press C.'
        $answer = Read-Host 'Allow full-desktop screenshots for this run? [y/N]'
        if ($answer -notmatch '^(y|yes)$') { Write-Host 'Screenshot skipped.'; return $null }
        $State.ScreenshotConsent = $true
    }
    $label = Read-Host 'Screenshot label (blank = case id)'
    if (-not $label) { $label = $CaseId }
    Write-Host 'Capturing in 4 seconds - switch to the window you want to capture.'
    for ($i = 4; $i -ge 1; $i--) {
        Write-Host "  $i..."
        Start-Sleep -Seconds 1
    }
    Add-Type -AssemblyName System.Drawing | Out-Null
    Add-Type -AssemblyName System.Windows.Forms | Out-Null
    $bounds = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
        $safe = ($label -replace '[^A-Za-z0-9._-]', '_')
        $folder = Join-Path $State.EvidencePath 'screenshots'
        $index = (Get-ChildItem -LiteralPath $folder -Filter "$safe-*.png" -ErrorAction SilentlyContinue).Count + 1
        $path = Join-Path $folder ("$safe-$index.png")
        $bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally {
        $graphics.Dispose()
        $bitmap.Dispose()
    }
    Write-Host "Saved screenshot: $path"
    return $path
}

# --- Automated commands -------------------------------------------------------

function Invoke-AutoChecksPhase {
    param($State)
    # Only command-backed steps; ENV-* have no Command and are not offered here.
    $todo = @($script:Steps | Where-Object { $_.Mode -eq 'Auto' -and $_.Command -and -not $State.Results.Contains($_.Id) })
    if ($todo.Count -eq 0) { return }
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        foreach ($step in $todo) {
            $State.Results[$step.Id] = @{ Status = 'NOT RUN'; Notes = 'cargo not on PATH'; Evidence = @(); AtUtc = (Get-Date).ToUniversalTime().ToString('o') }
        }
        Save-WizardState -State $State
        return
    }
    $answer = Read-Host 'Run the automated cargo checks now (fmt, headless tests, clippy, app build)? [y/N]'
    if ($answer -notmatch '^(y|yes)$') {
        foreach ($step in $todo) {
            $State.Results[$step.Id] = @{ Status = 'NOT RUN'; Notes = 'declined by operator'; Evidence = @(); AtUtc = (Get-Date).ToUniversalTime().ToString('o') }
        }
        Save-WizardState -State $State
        return
    }
    Push-Location $State.RepoRoot
    try {
        foreach ($step in $todo) {
            Write-Host "Running $($step.Id) ..."
            $run = Invoke-WizardNativeCommand -State $State -Id $step.Id -Command $step.Command
            $text = Get-Content -Raw -LiteralPath (Join-Path $State.EvidencePath "commands\$($step.Id).txt") -ErrorAction SilentlyContinue
            $counts = Get-CargoTestCounts -Text $text
            $notes = "exit=$($run.ExitCode)"
            if ($run.Error) { $notes = "error=$($run.Error)" }
            if ($step.Kind -eq 'Test') {
                $notes += " executed=$($counts.Executed) failed=$($counts.Failed) ignored=$($counts.Ignored)"
            }
            $status = Get-AutoCommandStatus -ExitCode $run.ExitCode -Error $run.Error -Kind $step.Kind -Counts $counts
            if ($status -eq 'UNVERIFIED' -and -not $run.Error) { $notes += ' (no executed tests / no exit code)' }
            $State.Results[$step.Id] = @{ Status = $status; Notes = $notes; Evidence = @("commands/$($step.Id).txt"); AtUtc = (Get-Date).ToUniversalTime().ToString('o') }
            Save-WizardState -State $State
        }
    } finally { Pop-Location }
}

# --- Finish -------------------------------------------------------------------

function Copy-AppLogs {
    param($State)
    $logRoot = Join-Path $env:LOCALAPPDATA 'rommfs\logs'
    if (-not (Test-Path -LiteralPath $logRoot)) {
        return @{ Status = 'NOT RUN'; Notes = "no logs directory at $logRoot"; Evidence = @() }
    }
    $destination = Join-Path $State.EvidencePath 'logs'
    New-Item -ItemType Directory -Path $destination -ErrorAction SilentlyContinue | Out-Null
    $copied = 0
    foreach ($file in Get-ChildItem -LiteralPath $logRoot -Filter 'rommfs*.log*' -File -ErrorAction SilentlyContinue) {
        $text = Get-Content -Raw -LiteralPath $file.FullName -ErrorAction SilentlyContinue
        [System.IO.File]::WriteAllText((Join-Path $destination $file.Name), (Get-RedactedText $text))
        $copied++
    }
    if ($copied -eq 0) {
        return @{ Status = 'NOT RUN'; Notes = "no rommfs*.log files in $logRoot"; Evidence = @() }
    }
    return @{ Status = 'PASS'; Notes = "$copied log file(s) copied and redacted"; Evidence = @('logs/') }
}

function Write-WizardReport {
    param($State, [string]$Path)
    $statuses = @()
    $rows = @()
    foreach ($step in $script:Steps) {
        $result = $State.Results[$step.Id]
        if ($result) {
            $status = $result.Status
            $notes = $result.Notes
            $evidence = ($result.Evidence -join ', ')
        } else {
            $status = 'NOT RUN'
            $notes = ''
            $evidence = ''
        }
        $statuses += $status
        $rows += "| $($step.Id) | $status | $($step.Title) | $notes | $evidence |"
    }
    $overall = Get-OverallStatus -Statuses $statuses
    $lines = @(
        "Run / date: $($State.RunId) / $($State.StartedUtc)"
        'Checkout commit: see commands/ENV-COMMIT.txt; app hash and timestamp: see commands/ENV-APP.txt'
        'Initial git status: commands/ENV-GIT-STATUS.txt; final git status: commands/ENV-FINAL-STATUS.txt'
        'Host / Windows / elevation / Cloud Files: see ENV-HOST.txt'
        'RetroBat version marker / executable hashes: see commands/ENV-RETROBAT.txt'
        "Evidence root: $($State.EvidencePath)"
        ''
        'Catalogue cases record a functional observation only. The raw updated_at field and'
        'a full before/after benchmark are not visible from the app, so those claims stay'
        'UNVERIFIED unless the operator read them directly.'
        ''
        'Warning: logs and screenshots may still contain private information. This ZIP'
        'is local and is never uploaded automatically. Review it before sharing.'
        ''
        '| Case ID | Status | Step | Notes | Evidence |'
        '| --- | --- | --- | --- | --- |'
    ) + $rows + @(
        ''
        "Overall: $overall"
        'Overall is PASS only when every step is PASS. A skipped, blocked, partial or'
        'unverified step, or an automated command that executed zero tests, keeps the'
        'overall result below PASS.'
    )
    [System.IO.File]::WriteAllText($Path, (($lines -join [Environment]::NewLine) + [Environment]::NewLine))
    return $overall
}

function New-EvidenceZip {
    param([string]$EvidencePath, [string]$ZipPath)
    # Compress-Archive errors are non-terminating by default; make them fatal and
    # stage to a temp file so a failure keeps the previous ZIP and the evidence.
    $temp = "$ZipPath.tmp.zip"
    try {
        if (Test-Path -LiteralPath $temp) { Remove-Item -LiteralPath $temp -Force }
        Compress-Archive -Path (Join-Path $EvidencePath '*') -DestinationPath $temp -CompressionLevel Optimal -ErrorAction Stop
        Move-Item -LiteralPath $temp -Destination $ZipPath -Force -ErrorAction Stop
    } finally {
        if (Test-Path -LiteralPath $temp) { Remove-Item -LiteralPath $temp -Force -ErrorAction SilentlyContinue }
    }
    return $ZipPath
}

function Invoke-FinishPhase {
    param($State)
    # Always refresh logs, report and ZIP so a rerun is never a stale finish.
    Push-Location $State.RepoRoot
    try {
        $final = Invoke-WizardNativeCommand -State $State -Id 'ENV-FINAL-STATUS' -Command { git status --short }
        $finalStatus = Get-AutoCommandStatus -ExitCode $final.ExitCode -Error $final.Error -Kind 'Check' -Counts $null
        $State.Results['ENV-FINAL-STATUS'] = @{ Status = $finalStatus; Notes = "exit=$($final.ExitCode)"; Evidence = @('commands/ENV-FINAL-STATUS.txt'); AtUtc = (Get-Date).ToUniversalTime().ToString('o') }
    } finally { Pop-Location }
    $State.Results['LOGS-COLLECT'] = (Copy-AppLogs -State $State) + @{ AtUtc = (Get-Date).ToUniversalTime().ToString('o') }
    # Record packaging as done before writing the report so the report inside the
    # ZIP does not show the ZIP itself as NOT RUN. If packaging fails this is
    # corrected to FAIL below.
    $State.Results['EVIDENCE-ZIP'] = @{ Status = 'PASS'; Notes = $State.ZipPath; Evidence = @($State.ZipPath); AtUtc = (Get-Date).ToUniversalTime().ToString('o') }
    $reportPath = Join-Path $State.EvidencePath 'report.md'
    $null = Write-WizardReport -State $State -Path $reportPath
    $State.Finish.ReportPath = $reportPath
    try {
        $zipPath = New-EvidenceZip -EvidencePath $State.EvidencePath -ZipPath $State.ZipPath
        $State.Results['EVIDENCE-ZIP'].Evidence = @($zipPath)
        $State.Finish.ZipPath = $zipPath
    } catch {
        $State.Results['EVIDENCE-ZIP'].Status = 'FAIL'
        $State.Results['EVIDENCE-ZIP'].Notes = $_.Exception.Message
        $State.Finish.ZipPath = $null
        $null = Write-WizardReport -State $State -Path $reportPath
        Write-Host "Packaging failed: $($_.Exception.Message)"
        Write-Host "Evidence kept unpackaged at $($State.EvidencePath)"
    }
    Save-WizardState -State $State
    if ($State.Finish.ZipPath) {
        try { Start-Process -FilePath 'explorer.exe' -ArgumentList "/select,`"$($State.Finish.ZipPath)`"" } catch { }
    }
    return $State.Results['EVIDENCE-ZIP'].Status
}

# --- Interactive walkthrough --------------------------------------------------

function Read-StepResult {
    param($State, $Step)
    $notes = @()
    $shots = @()
    $draft = $State.Drafts[$Step.Id]
    if ($draft) {
        # Notes and screenshots survive an interrupted step.
        $notes = @($draft.Notes)
        $shots = @($draft.Evidence)
    }
    while ($true) {
        $raw = Read-Host '[Enter]=Pass  F=Fail  B=Blocked  S=Skip  P=Partial  U=Unverified  C=Screenshot  N=Note  Q=Save & quit'
        $key = if ($raw) { $raw.Trim().ToLowerInvariant() } else { '' }
        $base = @{ Notes = ($notes -join ' | '); Evidence = $shots; AtUtc = (Get-Date).ToUniversalTime().ToString('o') }
        switch -Regex ($key) {
            '^$' { $State.Drafts.Remove($Step.Id); return @{ Status = 'PASS' } + $base }
            '^f' { $State.Drafts.Remove($Step.Id); return @{ Status = 'FAIL' } + $base }
            '^b' { $State.Drafts.Remove($Step.Id); return @{ Status = 'BLOCKED' } + $base }
            '^s' { $State.Drafts.Remove($Step.Id); return @{ Status = 'NOT RUN' } + $base }
            '^p' { $State.Drafts.Remove($Step.Id); return @{ Status = 'PARTIAL' } + $base }
            '^u' { $State.Drafts.Remove($Step.Id); return @{ Status = 'UNVERIFIED' } + $base }
            '^c' {
                $shot = Save-WizardScreenshot -State $State -CaseId $Step.Id
                if ($shot) { $shots += "screenshots/$([System.IO.Path]::GetFileName($shot))" }
                $State.Drafts[$Step.Id] = @{ Notes = @($notes); Evidence = @($shots) }
                Save-WizardState -State $State
                continue
            }
            '^n' {
                $note = Read-Host 'Note (blank to keep)'
                if ($note) { $notes += $note }
                $State.Drafts[$Step.Id] = @{ Notes = @($notes); Evidence = @($shots) }
                Save-WizardState -State $State
                continue
            }
            '^q' { return $null }
            default { Write-Host 'Unknown key. Use Enter/F/B/S/P/U/C/N/Q.' }
        }
    }
}

function Invoke-EnvironmentPhase {
    param($State)
    if (-not $State.Results.Contains('ENV-HOST')) {
        Write-Host 'Collecting host and build metadata ...'
        Push-Location $State.RepoRoot
        try { Get-HostMetadata -State $State } finally { Pop-Location }
        $State.Results['ENV-HOST'] = @{ Status = 'PASS'; Notes = 'host metadata collected; missing fields recorded'; Evidence = @('ENV-HOST.txt'); AtUtc = (Get-Date).ToUniversalTime().ToString('o') }
        Save-WizardState -State $State
    }
    if (-not $State.Paths.AppExe) {
        $found = Resolve-AppExecutable -RepoRoot $State.RepoRoot
        if ($found) {
            Write-Host "Found app executable: $found"
            $State.Paths.AppExe = $found
        } else {
            Write-Host 'No built app executable found; pick one (or cancel to record it as NOT RUN).'
            $State.Paths.AppExe = Select-WizardPath -Title 'Select rommfs-app.exe' -Filter 'Executable (*.exe)|*.exe' -Initial $State.RepoRoot
        }
        Save-WizardState -State $State
    }
    # Refresh read-only metadata on resume so hashes from an older broken run are
    # never silently retained as PASS.
    $State.Results['ENV-APP'] = Write-AppEvidence -State $State
    Save-WizardState -State $State
    if (-not $State.Paths.RetroBatRoot) {
        $found = Resolve-RetroBatRoot
        if ($found) {
            Write-Host "Found RetroBat root: $found"
            $State.Paths.RetroBatRoot = $found
        } else {
            Write-Host 'No RetroBat installation auto-discovered; pick the real install root (or cancel).'
            $State.Paths.RetroBatRoot = Select-WizardPath -Title 'Select the real RetroBat installation root' -Folder
        }
        Save-WizardState -State $State
    }
    $State.Results['ENV-RETROBAT'] = Write-RetroBatEvidence -State $State
    Save-WizardState -State $State
}

function Invoke-RommFsTestWizard {
    if (-not $env:LOCALAPPDATA) {
        Write-Error 'This wizard runs on Windows; LOCALAPPDATA is not set.'
        return $false
    }
    $support = Join-Path $PSScriptRoot 'windows-test-support.ps1'
    if (-not (Test-Path -LiteralPath $support)) {
        Write-Error "Missing helper: $support"
        return $false
    }
    $repoRoot = Split-Path -Parent $PSScriptRoot
    $wizardRoot = Join-Path $env:LOCALAPPDATA 'rommfs\test-wizard'
    . $support
    $state = $null
    $lastRun = Find-LastWizardRun -Root $wizardRoot
    if ($lastRun) {
        $answer = Read-Host "Resume the last run at $lastRun ? [y/N]"
        if ($answer -match '^(y|yes)$') {
            $state = Read-WizardState -Path (Join-Path $lastRun 'wizard-state.json')
        }
    }
    if (-not $state) {
        $runId = '{0}-{1}' -f (Get-Date -Format 'yyyyMMdd-HHmmss'), ([guid]::NewGuid().ToString('N').Substring(0, 8))
        $evidencePath = Join-Path $wizardRoot $runId
        $zipPath = Join-Path $wizardRoot "$runId.zip"
        $evidencePath = Initialize-RommFsEvidence -EvidencePath $evidencePath
        $state = New-WizardState -RunId $runId -RepoRoot $repoRoot -EvidencePath $evidencePath -ZipPath $zipPath
        Save-WizardState -State $state
        Write-Host "New run: $runId"
        Write-Host "Evidence: $evidencePath"
    } else {
        Write-Host "Resuming run: $($state.RunId)"
        Write-Host "Evidence: $($state.EvidencePath)"
        # A previous process's launch marker is stale: the app may have been closed.
        $state.Launched = @()
    }
    Write-Host ''
    Write-Host 'Reads are automatic. This wizard never changes your games, saves, emulator'
    Write-Host 'configuration, drivers, registry or checkout. Screenshots need your consent.'
    Write-Host 'Answer each step: [Enter]=Pass  F=Fail  B=Blocked  S=Skip  P=Partial  U=Unverified'
    Write-Host 'C=Screenshot  N=Note  Q=Save and quit (rerun to resume).'
    $consent = Read-Host 'Continue? The wizard may open the app and Explorer and writes only to your evidence folder. [Y/n]'
    if ($consent -match '^(n|no)$') {
        Write-Host 'Cancelled. No changes were made.'
        return $true
    }
    Invoke-EnvironmentPhase -State $state
    Invoke-AutoChecksPhase -State $state
    if (-not $state.Paths.AppExe -or -not (Test-Path -LiteralPath $state.Paths.AppExe -PathType Leaf)) {
        $state.Paths.AppExe = Resolve-AppExecutable -RepoRoot $state.RepoRoot
    }
    # The optional build may have rewritten the executable at the same path, so
    # recapture its hash/timestamp before launching anything.
    if ($state.Paths.AppExe -and (Test-Path -LiteralPath $state.Paths.AppExe)) {
        $state.Results['ENV-APP'] = Write-AppEvidence -State $state
        Save-WizardState -State $state
    }
    $manual = @($script:Steps | Where-Object { $_.Mode -eq 'Manual' })
    $index = 0
    foreach ($step in $manual) {
        $index++
        if ($state.Results.Contains($step.Id)) {
            Write-Host "[$index/$($manual.Count)] already recorded: $($step.Id) ($($state.Results[$step.Id].Status))"
            continue
        }
        try { Clear-Host } catch { }
        Write-Host ("=== Step {0}/{1}: {2} - {3} ===" -f $index, $manual.Count, $step.Id, $step.Title)
        if ($step.Paths) {
            Write-Host "App executable : $($state.Paths.AppExe)"
            Write-Host "RetroBat root  : $($state.Paths.RetroBatRoot)"
            Write-Host "Evidence folder: $($state.EvidencePath)"
        }
        # Capture the baseline before showing the action so the operator cannot act
        # while it is being hashed; a resume reuses the saved baseline.
        $manifestBefore = Get-StepManifest -State $state -Step $step -When 'before'
        Write-Host $step.Instruction
        if ($step.Open) { Open-WizardApp -State $state }
        Write-Host ''
        $result = Read-StepResult -State $state -Step $step
        if ($null -eq $result) {
            $packaging = Invoke-FinishPhase -State $state
            Write-Host ''
            if ($state.Finish.ZipPath) {
                Write-Host "Saved and packaged. Send this file: $($state.Finish.ZipPath)"
            } else {
                Write-Host "Saved. Packaging failed ($packaging); evidence kept at $($state.EvidencePath)."
            }
            Write-Host "Rerun the wizard to resume run $($state.RunId)."
            return $true
        }
        if ($step.Manifest) {
            if ($manifestBefore) {
                $manifestAfter = Get-StepManifest -State $state -Step $step -When 'after'
                $diff = Compare-WizardManifest -Before $manifestBefore -After $manifestAfter
                $summary = "manifest added=$($diff.Added.Count) removed=$($diff.Removed.Count) changed=$($diff.Changed.Count) (before=$($diff.BeforeCount) after=$($diff.AfterCount))"
                Write-EvidenceFile -State $state -Name "harness\$($step.Id)-diff.txt" -Text (
                    @($summary, '', 'Added:', $diff.Added, 'Removed:', $diff.Removed, 'Changed:', $diff.Changed) -join [Environment]::NewLine)
                $result.Evidence = @($result.Evidence) + @("harness/$($step.Id)-before.txt", "harness/$($step.Id)-after.txt", "harness/$($step.Id)-diff.txt")
                $adjust = Get-AdjustedManifestStatus -Status $result.Status -Before $manifestBefore -After $manifestAfter -Diff $diff
                $result.Status = $adjust.Status
                $result.Notes = (@($result.Notes, $summary) + $adjust.Notes | Where-Object { $_ }) -join ' | '
                if ($adjust.Status -eq 'FAIL') { Write-Host 'FAIL: stock files changed or removed; byte-identical is contradicted.' }
            } else {
                $adjust = Get-AdjustedManifestStatus -Status $result.Status -Before $null -After $null -Diff $null
                $result.Status = $adjust.Status
                $result.Notes = (@($result.Notes) + $adjust.Notes | Where-Object { $_ }) -join ' | '
            }
        }
        $state.Results[$step.Id] = $result
        $evidenceLines = @(
            "Case: $($step.Id)"
            "Title: $($step.Title)"
            "Status: $($result.Status)"
            "AtUtc: $($result.AtUtc)"
            "Notes: $($result.Notes)"
            "Screenshots: $($result.Evidence -join ', ')"
        )
        Write-EvidenceFile -State $state -Name "desktop\$($step.Id).md" -Text (($evidenceLines -join [Environment]::NewLine) + [Environment]::NewLine)
        Save-WizardState -State $state
    }
    $packaging = Invoke-FinishPhase -State $state
    Write-Host ''
    Write-Host "Run $($state.RunId) finished. Packaging: $packaging"
    Write-Host "Report: $($state.Finish.ReportPath)"
    if ($state.Finish.ZipPath) {
        Write-Host "Send this file: $($state.Finish.ZipPath)"
        Write-Host 'The ZIP is local only; nothing was uploaded. Review it for private information.'
    } else {
        Write-Host "Packaging failed; evidence kept unpackaged at $($state.EvidencePath)."
    }
    return $true
}

function Get-WizardHelp {
    Get-Help $PSCommandPath -Detailed
}

# --- Step definitions ---------------------------------------------------------

$script:Steps = @(
    @{ Id = 'ENV-HOST'; Mode = 'Auto'; Title = 'Collect host, Windows build, elevation and Cloud Files metadata' }
    @{ Id = 'ENV-APP'; Mode = 'Auto'; Title = 'Record app executable hash and timestamp' }
    @{ Id = 'ENV-RETROBAT'; Mode = 'Auto'; Title = 'Record real RetroBat version marker and executable hashes' }
    @{ Id = 'AUTO-FMT'; Mode = 'Auto'; Kind = 'Check'; Title = 'cargo fmt --all -- --check'; Command = { cargo fmt --all -- --check } }
    @{ Id = 'AUTO-HEADLESS'; Mode = 'Auto'; Kind = 'Test'; Title = 'portable/headless test suite'; Command = { cargo test --locked -p rommfs-core -p rommfs-fixture -p rommfs-cfapi -p rommfs-app --no-default-features -- --test-threads=1 } }
    @{ Id = 'AUTO-WORKSPACE'; Mode = 'Auto'; Kind = 'Test'; Title = 'native workspace test suite'; Command = { cargo test --locked --workspace -- --test-threads=1 } }
    @{ Id = 'AUTO-CLIPPY'; Mode = 'Auto'; Kind = 'Check'; Title = 'clippy -D warnings'; Command = { cargo clippy --locked --workspace --all-targets -- -D warnings } }
    @{ Id = 'AUTO-BUILD'; Mode = 'Auto'; Kind = 'Check'; Title = 'cargo build -p rommfs-app'; Command = { cargo build --locked -p rommfs-app } }
    @{ Id = 'SETUP-CONNECT'; Mode = 'Manual'; Open = $true; Paths = $true; Title = 'Connect and choose an empty games folder'; Instruction = @'
Sign in to the test server in the app and connect.
Choose an empty local NTFS games folder, but do NOT start it yet: Start fails before
RetroBat setup. Confirm the RetroBat tab detects the installation above.
'@ }
    @{ Id = 'SETUP-PLAYDATA-ON'; Mode = 'Manual'; Open = $true; Title = 'Enable favourites and play-history sync'; Instruction = @'
On the Saves tab, turn on the favourites and play-history sync switch.
If the Account tab says "Needs permission", press Reconnect and sign in again.
This must be on before any favourites or play-count step.
'@ }
    @{ Id = 'SETUP-OWNERSHIP'; Mode = 'Manual'; Open = $true; Manifest = @('roms'); Title = 'Set up RetroBat without touching your own files'; Instruction = @'
On the RetroBat tab run Set up RetroBat against the real install root above.
Each roms\<system> must still be a real folder with your files and now hold a RomM link.
The wizard hashed your stock roms\ files before and after and reports the summary.
'@ }
    @{ Id = 'SETUP-OWNERSHIP-USER-OWNED'; Mode = 'Manual'; Title = 'User-owned edge cases (optional, disposable install only)'; Instruction = @'
Optional, DISPOSABLE install only. Never do this in your real games folder.
In a throwaway RetroBat copy, add a user-owned roms\<system> link, a file named like a
system folder, and an existing roms\<system>\RomM, then run setup again.
Expected: those are skipped with a reason; every other system is linked; repeat reuses links.
'@ }
    @{ Id = 'SETUP-UAC-DECLINE'; Mode = 'Manual'; Open = $true; Title = 'Decline the UAC prompt'; Instruction = @'
Needs a FRESH disposable install with no links yet and Developer Mode off. Do not change
Developer Mode. Run Set up RetroBat and decline the UAC prompt: nothing may change.
If no prompt appears or links already exist, press B; do not undo anything.
'@ }
    @{ Id = 'SETUP-UAC-APPROVE'; Mode = 'Manual'; Open = $true; Title = 'Approve the UAC prompt'; Instruction = @'
Same fresh disposable install. Run Set up RetroBat and approve the UAC prompt once.
Every system folder gets its RomM link. RomMFS itself must stay unelevated.
'@ }
    @{ Id = 'SETUP-RECOVERY-INTERRUPT'; Mode = 'Manual'; Title = 'Interrupted setup resumes (optional, disposable install only)'; Instruction = @'
Optional, DISPOSABLE install only. Start setup and close/interrupt it after some links appear.
Run setup again: the rerun must complete the remaining links. Otherwise press S.
'@ }
    @{ Id = 'SETUP-RECOVERY-SCHEMA2'; Mode = 'Manual'; Title = 'Earlier-layout migration (only if one already exists)'; Instruction = @'
Only if this host already has a schema-2 install (.rommfs-original-roms).
Run Set up RetroBat; folders an earlier version moved aside must return into roms\.
If you do not already have such an install, press S to skip.
'@ }
    @{ Id = 'RETROBAT-PRISTINE'; Mode = 'Manual'; Open = $true; Manifest = @('roms', 'saves'); Title = 'Stock RetroBat setup and mount'; Instruction = @'
On a stock RetroBat install, run Set up RetroBat and mount a games folder.
Stock roms\ and saves\ originals must be unchanged; the wizard hashes them before/after.
A gamelist.xml merge is expected - verify your own entries manually, the wizard notes it.
Each system gets a RomM link and RomM games appear next to stock games. Playing is covered later.
'@ }
    @{ Id = 'DUP-HIDE'; Mode = 'Manual'; Open = $true; Title = 'Hide the RomM copy of a game you own'; Instruction = @'
Copy one RomM game file yourself into the matching roms\<system>\ folder (not under RomM\).
Refresh in RomMFS and open RetroBat: the RomM copy must be hidden and your own entry untouched.
'@ }
    @{ Id = 'DUP-UNHIDE'; Mode = 'Manual'; Open = $true; Title = 'Unhide when your local copy goes away'; Instruction = @'
Delete the local copy you added and refresh: the RomM copy must reappear.
Then turn off "Hide RomM games you already have" and refresh: only RomMFS-hidden entries reappear.
'@ }
    @{ Id = 'PLAYDATA-FAVOURITE'; Mode = 'Manual'; Open = $true; Title = 'Favourites both ways'; Instruction = @'
Favourite a RomM game in RetroBat, close RetroBat, and check it is a favourite in RomM.
Favourite a different game in RomM, refresh in RomMFS, and check RetroBat shows it as a favourite.
If both sides changed the same game, the RetroBat value wins.
'@ }
    @{ Id = 'PLAYDATA-PLAYCOUNT'; Mode = 'Manual'; Open = $true; Title = 'Play count and last played reach RomM'; Instruction = @'
Play a RomM game twice in RetroBat (launch, exit cleanly), close RetroBat, and check RomM
shows the play count and last played. Do not reset the local value to fake a decrease.
'@ }
    @{ Id = 'CARD-PS1'; Mode = 'Manual'; Open = $true; Title = 'PS1 memory card upload and restore'; Instruction = @'
On Saves, turn on PS1 memory cards (DuckStation). Save in DuckStation, close it, and check RomM
shows a memory-card version for this account.
Then on a fresh disposable second copy (same machine is fine, emulator stopped) opt in to the same
account and card folder: the card is restored and must match the uploaded version. Do not delete
your local card. If DuckStation or a second disposable copy is unavailable, press S.
'@ }
    @{ Id = 'CARD-PS2'; Mode = 'Manual'; Open = $true; Title = 'PS2 memory card upload and restore'; Instruction = @'
Only if PCSX2/PS2 is installed. Turn on PS2 memory cards (PCSX2), save in PCSX2, close it, and
check RomM shows a version. Restore it on a fresh disposable second copy with the emulator stopped
and the same account, then confirm it matches. Otherwise press S.
'@ }
    @{ Id = 'CARD-CONFLICT'; Mode = 'Manual'; Title = 'Memory card conflict keeps both (needs two disposable copies)'; Instruction = @'
Needs two independent disposable RetroBat copies for the same game and card.
Change the card in copy A and sync; change the same card in copy B and sync.
Expected: B keeps its local card and the remote version appears beside it as
.rommfs-conflict-v<id>, listed in Save review. If you do not have two disposable copies, press S.
'@ }
    @{ Id = 'UI-SHOTS-OVERVIEW'; Mode = 'Manual'; Open = $true; Title = 'Screenshot Overview, Save sync and Activity (default size)'; Instruction = @'
At the default window size, capture Overview, then Save sync, then Activity.
Scroll each page to the bottom and capture where scrolling is under test.
Press C once per page and label it, then press Enter.
'@ }
    @{ Id = 'UI-SHOTS-SETTINGS'; Mode = 'Manual'; Open = $true; Title = 'Screenshot every Settings tab (default size)'; Instruction = @'
Capture each Settings tab at the default size: Account, Games folder, RetroBat, Saves, App.
Scroll each tab to the bottom. Press C per tab and label it, then press Enter.
'@ }
    @{ Id = 'UI-SHOTS-MAX'; Mode = 'Manual'; Open = $true; Title = 'Screenshot every page and tab maximised'; Instruction = @'
Maximise the window and repeat the sweep: Overview, Save sync, Activity and each Settings tab.
Press C per page/tab and label it. Capture any clipping or lost scrolling.
'@ }
    @{ Id = 'UI-ARTWORK-PROGRESS'; Mode = 'Manual'; Open = $true; Title = 'Artwork and gamelist progress is visible'; Instruction = @'
Refresh the library on Overview. The "Artwork & gamelists" row must show live stage and
counts, then "Up to date - N ago" (or a Retry on failure), and Activity must show the
accented progress. Press C while it runs, then press Enter.
'@ }
    @{ Id = 'UI-ES-FOLDER'; Mode = 'Manual'; Title = 'EmulationStation shows the RomM folder'; Instruction = @'
In RetroBat, open EmulationStation and find how the RomM games are reached.
Record whether RomM/ appears as a folder and the exact menu path you used
(START - User Interface Settings - Gamelist Options - Show folders).
If you cannot observe it on this host, press U (unverified) with a note.
'@ }
    @{ Id = 'CATALOGUE-TIMING'; Mode = 'Manual'; Open = $true; Title = 'Catalogue refresh timing (functional observation)'; Instruction = @'
Press Refresh library on a real instance and note the elapsed time and counts from Activity.
This is a functional observation only; a benchmark needs the log flags and a cold/warm run,
so record the app-observed timing and mark U if you cannot confirm the flags.
'@ }
    @{ Id = 'CATALOGUE-UPDATED-AT'; Mode = 'Manual'; Open = $true; Title = 'Incremental refresh after a file-only change'; Instruction = @'
Change only a ROM's secondary files on the server, rescan, then run an incremental refresh.
Note whether the change appeared. The raw updated_at field is not visible from the app:
press U (unverified) unless you can read it, and record a full before/after refresh too.
'@ }
    @{ Id = 'ENV-FINAL-STATUS'; Mode = 'Auto'; Group = 'Finish'; Title = 'Record final git status' }
    @{ Id = 'LOGS-COLLECT'; Mode = 'Auto'; Group = 'Finish'; Title = 'Copy and redact the known app logs' }
    @{ Id = 'EVIDENCE-ZIP'; Mode = 'Auto'; Group = 'Finish'; Title = 'ZIP the evidence and reveal it in Explorer' }
)

if ($MyInvocation.InvocationName -ne '.') {
    if ($Help) {
        Get-WizardHelp
        exit 0
    }
    if ((Invoke-RommFsTestWizard) -eq $false) { exit 1 }
}
