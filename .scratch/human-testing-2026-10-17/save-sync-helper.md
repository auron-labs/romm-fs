# Save-sync file checks

Run `./scripts/save-sync-check.ps1` from PowerShell at the repository root,
with the emulator closed. No Rust or package installation is needed.

The default inventory is for the Pokémon Red acceptance session on this host.
Override `-SavePath`, `-Downloads`, and `-EvidenceRoot` for other installations.
It snapshots the local save and hashes matching downloaded RomM files and
Pokémon backup files. Each run creates a separate folder under
`$env:TEMP/rommfs-save-checks` with `files.csv`, `result.json`, and copies named
by verified content: `01-original.bin`, `02-synced-baseline.bin`, and
`03-local-conflict-candidate.bin` when available. Other hashes receive their own
labels. These three labels refer to this session's known hashes, not arbitrary
games. Original files are never changed. Evidence is local and contains paths
and save content; nothing is uploaded.

Compare a specifically downloaded remote version with the local save:

```powershell
./scripts/save-sync-check.ps1 -Action Compare -RemoteFile 'C:\path\download.bin'
```

Wait up to 60 seconds for a particular local save hash:

```powershell
./scripts/save-sync-check.ps1 -Action Watch -ExpectedHash '<64-character SHA256>'
```

Comparison mismatch and watch timeout return exit code 1. A matching hash is
evidence of equal bytes, not proof of a transfer, correct game mapping, or a
conflict decision. Inventory never reports acceptance PASS. Select the intended
version in RomM; download filenames and dates cannot prove remote ordering.

For a conflict test, record a synchronized baseline first, disable sync, create
and snapshot a different local save, and upload a labelled copy with bytes
different from both the baseline and the changed local save to the same remote
group. Verify the remote copy by downloading it before re-enabling sync. Keep
the emulator closed. Observe the conflict UI and hash the local save afterward;
do not infer a conflict defect merely from uploading another copy of the baseline.
The helper does not control consent, uploads, or conflict resolution.
