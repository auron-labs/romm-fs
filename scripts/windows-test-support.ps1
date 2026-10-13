function Initialize-RommFsEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [string]$EvidencePath
    )

    $fullEvidencePath = [System.IO.Path]::GetFullPath($EvidencePath)
    if (Test-Path -LiteralPath $fullEvidencePath) {
        throw "Evidence path already exists; choose a new per-run path: $fullEvidencePath"
    }

    New-Item -ItemType Directory -Path $fullEvidencePath -ErrorAction Stop | Out-Null
    @('commands', 'native', 'harness', 'desktop', 'saves', 'screenshots') |
        ForEach-Object {
            New-Item -ItemType Directory -Path (Join-Path $fullEvidencePath $_) -ErrorAction Stop |
                Out-Null
        }

    return $fullEvidencePath
}

function Invoke-LoggedCommand {
    param(
        [Parameter(Mandatory)]
        [string]$Id,

        [Parameter(Mandatory)]
        [scriptblock]$Command
    )

    # $Evidence is supplied by the caller's current PowerShell scope, matching
    # the Windows acceptance guide's existing command calls.
    $log = Join-Path (Join-Path $Evidence 'commands') "$Id.txt"
    "COMMAND: $Command" | Set-Content -LiteralPath $log
    & $Command 2>&1 | Tee-Object -FilePath $log -Append
    $exitCode = $LASTEXITCODE
    "EXIT_CODE: $exitCode" | Tee-Object -FilePath $log -Append
    return $exitCode
}
