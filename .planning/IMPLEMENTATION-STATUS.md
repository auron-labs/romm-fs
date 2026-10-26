# Planning and evidence status — 24 October 2026

Reviewed against checkout `40e59cf`, the retained planning documents, test logs,
and the [23–24 October agent report](../.scratch/agent-testing-2026-10-24/report.md).
This is an evidence reconciliation, not a new application test run. Current
implementation details belong in [architecture](../docs/architecture.md);
the [12 October validation record](../docs/validation/2026-10-12.md) remains
the historical record for that date.

## Current position

The core Windows/RetroBat path has substantial runtime evidence: catalogue,
placeholders, hydration, real emulator gameplay, ordinary-save upload, safe
conflict retention/export, first incoming restore, offline upload recovery,
deletion review, restart persistence, and tray shutdown. The latest agent run
used Windows Server 2022 as Administrator at `360e13a`, not a Windows 10/11
client. It reports 235 linked systems on RetroBat 8.2.1, with 2,063 stock ROM
files byte-identical and 1,852 stock save files untouched.

The [UI remediation implementation record](../docs/ux-remediation.md) supersedes
the old scratch implementation brief. The 18 October
[before-fix report](../.scratch/archived/mcp-testing-2026-10-18/report.html) and
[retest](../.scratch/archived/mcp-fixes-2026-10-18/report.html) record the Activity
accessibility crash, library-details layout and skipped-file wording fixes.
The 20 October logs additionally record a failing regression test followed by
its passing run and a complete portable suite. Reported hover latency remains
unmeasured; do not treat reduced log rendering or the new stray-window finding
as proof of its cause.

## Next work

1. Implement the [Windows report fixes](../.scratch/windows-report-fixes-2026-10-24/spec.md),
   which remain `ready-for-agent`: diagnose/fix the extra black window that
   intercepts clicks, fix the Windows-only unused test import, preserve setup
   failure/skip reasons, explain existing-file export refusals, and make tracked
   save counts consistent. Source checks still show the unused import and the
   mapped-target count in the tracked-save banner. The stray window's owner is
   unconfirmed; fix it only after native diagnosis.
2. Complete outstanding native acceptance: Windows 10/11, EmulationStation-visible
   metadata and play-data behavior, standard-user/UAC legs, scaling, the deeper
   FILES matrix, auxiliary/BIOS transfers, full account/server switching and
   folder visibility offline. The latest report marks these partial, blocked
   or not run. See the [Windows procedure](../docs/testing/windows.md).
3. Continue the [16-item improvement roadmap](PLAN.md). It remains a backlog,
   not completed work. Its first items—ROM checksum/cache integrity,
   persistent save-history fingerprints and explicit conflict resolution—are
   distinct from the tested baseline. For example, current ROM publication
   checks the reported byte count, while warm-cache lookup checks file type
   and version without checking actual length. Later items include request
   cancellation, reconnect, storage limits, emulator layouts/hooks, verified
   BIOS placement, grouped saves, resume/offline preparation, release packaging,
   compatibility reporting, a RetroBat menu entry and native save states.

The tester's `/VERYSILENT` installer observation is outside the Windows-fixes
spec: RomMFS intentionally launches the interactive installer and does not pass
that flag.

## Retained test evidence

- [Latest agent report](../.scratch/agent-testing-2026-10-24/report.md): reports
  **371 Windows workspace tests passed**, but strict Windows Clippy failed on
  the unused `read_local_card_hash` test import. This is diagnostic Server 2022
  evidence, not complete native acceptance.
- [20 October portable log](../.scratch/archived/activity-accessibility-2026-10-20/rommfs-activity-portable.log):
  **385 passed, zero failed or ignored**, summed across its 33 result records.
  Its [Clippy log](../.scratch/archived/activity-accessibility-2026-10-20/rommfs-activity-clippy.log)
  has a successful completion marker. The 19 October portable log also totals
  385 passes; host UI and Clippy logs show successful completion.
- Earlier `plan6` logs total 488 passes for their own run; the UI remediation
  record reports 383. These runs have different scopes/builds and are not a
  single cumulative test count. Retained red logs are expected before-fix
  evidence, not evidence that their later green runs failed.
- The 17–18 October human/MCP reports, screenshots, JSON captures, hashes,
  backups and save evidence are preserved together under `.scratch/archived/`. Earlier
  metadata/locking failures and inconclusive conflict attempts remain historical
  observations; later passes do not prove every earlier live configuration
  or every metadata scenario is fixed.

## Archive reconciliation

Moved superseded material to `.scratch/archived/` without deleting evidence:

- `.planning/archived/` → [archived planning](../.scratch/archived/planning/):
  the old PRD, backend decision, completed implementation/UI plans, findings,
  empty issue list and 11 October testing note. These contain superseded rules,
  including the old whole-ROM-directory setup design and deferred memory cards.
- `.planning/ISSUES.md` → [17 October issues](../.scratch/archived/ISSUES-2026-10-17.md):
  broad old complaints are superseded by implementation records and dated
  findings; the unmeasured hover-latency concern is retained above.
- `.scratch/UI-UX-REMEDIATION.md` → [original UX brief](../.scratch/archived/UI-UX-REMEDIATION.md):
  implemented work and remaining native checks are recorded in `docs/ux-remediation.md`.

Relative document links were adjusted for the moves; historical behavior and
test outcomes were not rewritten. No `plans/` directory exists in this checkout.
`.scratch/STATE.md` is a generated 19 October snapshot from another host; its
"0 active" count is not the current backlog and it was left untouched.

The eight completed testing/evidence folders were also moved intact into
`.scratch/archived/`: `human-testing-2026-10-17`, `human-testing-2026-10-18`,
`mcp-testing-2026-10-18`, `mcp-fixes-2026-10-18`, `metadata-status-2026-10-18`,
`fixes-2026-10-19`, `activity-accessibility-2026-10-20`, and
`ux-remediation-preview`. The latest `agent-testing-2026-10-24` report and
pending `windows-report-fixes-2026-10-24` spec remain directly under `.scratch/`
because they guide the next work. Archiving earlier evidence does not close
its outstanding acceptance gaps.
