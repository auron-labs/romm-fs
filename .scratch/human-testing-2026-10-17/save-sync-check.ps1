#requires -Version 5.1
<#
.SYNOPSIS
Labels save versions by content and checks save-sync results without changing saves.
.EXAMPLE
.\scripts\save-sync-check.ps1
.EXAMPLE
.\scripts\save-sync-check.ps1 -Action Compare -RemoteFile 'C:\Users\azza\Downloads\save.bin'
.EXAMPLE
.\scripts\save-sync-check.ps1 -Action Watch -ExpectedHash '<SHA256>'
#>
[CmdletBinding()]
param(
    [ValidateSet('Inventory', 'Compare', 'Watch')][string]$Action = 'Inventory',
    [string]$SavePath = 'C:\RetroBat\saves\gb\Pokemon - Red Version.srm',
    [string]$Downloads = (Join-Path $env:USERPROFILE 'Downloads'),
    [string]$EvidenceRoot = (Join-Path $env:TEMP 'rommfs-save-checks'),
    [string]$RemoteFile,
    [ValidatePattern('^[a-fA-F0-9]{64}$')][string]$ExpectedHash,
    [ValidateRange(1, 300)][int]$TimeoutSeconds = 60
)
$ErrorActionPreference = 'Stop'

function Get-SaveLabel([string]$Hash) {
    switch ($Hash.ToUpperInvariant()) {
        '19061BAE6B649699EB37BA069E058B213AEAC05CF5C08A43041CC53B9B93BBA7' { return '01-original' }
        'A18B8806132EB90F63BB5C1ECED68B0FAB402050B008BC1099674FF831B3A2A1' { return '02-synced-baseline' }
        '29F1EA83F891171B9F2234EA83B8F1EBDCAE768B4E5692FAB13E92A828B82E0D' { return '03-local-conflict-candidate' }
        default { return 'other-' + $Hash.Substring(0, 12).ToLowerInvariant() }
    }
}

function Get-SaveHash([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "File not found: $Path" }
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
}

function Invoke-SaveCheck {
    if ($Action -eq 'Compare' -and -not $RemoteFile) { throw 'Compare requires -RemoteFile (a downloaded RomM save).' }
    if ($Action -eq 'Watch' -and -not $ExpectedHash) { throw 'Watch requires -ExpectedHash.' }
    $localHash = Get-SaveHash $SavePath
    if ($Action -eq 'Watch') {
        $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
        do {
            $localHash = Get-SaveHash $SavePath
            if ($localHash -eq $ExpectedHash) { break }
            Start-Sleep -Milliseconds 500
        } while ([DateTime]::UtcNow -lt $deadline)
    }

    $run = Join-Path $EvidenceRoot ((Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
    New-Item -ItemType Directory -Path $run | Out-Null
    $snapshot = Join-Path $run 'local-snapshot.srm'
    Copy-Item -LiteralPath $SavePath -Destination $snapshot
    if ((Get-SaveHash $snapshot) -ne $localHash -or (Get-SaveHash $SavePath) -ne $localHash) {
        throw "Local save changed during capture. Close the emulator and retry. Partial evidence: $run"
    }
    $rows = @([pscustomobject]@{ Role = 'LOCAL'; Label = Get-SaveLabel $localHash; SHA256 = $localHash; Path = $SavePath })
    $outcome = 'INVENTORY ONLY - no sync result inferred'

    if ($Action -eq 'Inventory') {
        $candidates = @()
        if (Test-Path -LiteralPath $Downloads) {
            $candidates += @(Get-ChildItem -LiteralPath $Downloads -File -Filter 'rommfs-*.bin')
        }
        $parent = Split-Path -Parent $Downloads
        if (Test-Path -LiteralPath $parent) {
            $candidates += @(Get-ChildItem -LiteralPath $parent -File -Filter 'Pokemon-Red-*.srm')
        }
        foreach ($file in $candidates) {
            $hash = Get-SaveHash $file.FullName
            $label = Get-SaveLabel $hash
            # Stage one copy per content hash, never overwrite a prior copy.
            $target = Join-Path $run ($label + '.bin')
            if (-not (Test-Path -LiteralPath $target)) {
                Copy-Item -LiteralPath $file.FullName -Destination $target
                if ((Get-SaveHash $target) -ne $hash) { throw "Source changed during capture: $($file.FullName)" }
            }
            $rows += [pscustomobject]@{ Role = 'COPY'; Label = $label; SHA256 = $hash; Path = $file.FullName }
        }
    }
    if ($Action -eq 'Compare') {
        $remoteHash = Get-SaveHash $RemoteFile
        $rows += [pscustomobject]@{ Role = 'DOWNLOADED'; Label = Get-SaveLabel $remoteHash; SHA256 = $remoteHash; Path = $RemoteFile }
        if ($localHash -eq $remoteHash) { $outcome = 'MATCH - local and downloaded bytes agree' }
        else { $outcome = 'MISMATCH - local and downloaded bytes differ' }
    }
    if ($Action -eq 'Watch') {
        if ($localHash -eq $ExpectedHash) { $outcome = 'EXPECTED HASH OBSERVED - not proof of transfer by itself' }
        else { $outcome = 'TIMEOUT - expected local hash was not observed' }
    }
    $rows | Export-Csv -LiteralPath (Join-Path $run 'files.csv') -NoTypeInformation
    [ordered]@{ Action = $Action; Outcome = $outcome; ExpectedHash = $ExpectedHash; Files = $rows } |
        ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $run 'result.json') -Encoding UTF8
    $rows | Format-Table Role, Label, SHA256 -AutoSize | Out-Host
    Write-Host $outcome
    Write-Host "Backups, labelled upload copies and report: $run"
    Write-Host 'Filename dates do not establish the latest remote version. Select that version in RomM before downloading.'
    if ($outcome -like 'MISMATCH*' -or $outcome -like 'TIMEOUT*') { return 1 }
    return 0
}

if ($MyInvocation.InvocationName -ne '.') { exit (Invoke-SaveCheck) }
