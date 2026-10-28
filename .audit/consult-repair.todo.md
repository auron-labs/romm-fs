# Consult repair

- [x] Read the Principles section of the poteto-mode skill.
- [x] Phase A: Frame.
- [x] Phase B: Design the workflow.
- [x] Phase C: Run the loop.
- [x] Phase D: Keep the audit trail.
- [x] Phase E: Verify and hand back; native limits remain explicit.

Definition of done is the seven outcomes in consult/ROMMFS-REPAIR-PLAN.md,
with observed checks separated from unavailable native Windows checks.

- [x] Blocking first steps. Read supplied plan and current owners. No new baseline requested.
- [x] Independent workstreams. Controller, CFAPI, saves, and UI each have an isolated worktree.
- [x] Shared mutable state. Coordinate API changes before integration; root alone integrates and runs final checks.
- [x] Smallest safe decomposition. Existing owners remain; no replacement frameworks or dependencies.
- [x] Integrate controller process checks, jobs, metadata retry and truthful summaries.
- [x] Integrate retained CFAPI lifecycle, reconciliation and metadata reuse.
- [x] Integrate persisted per-save decisions and verified incoming replacement.
- [x] Integrate setup, Overview and Saves flows.
- [x] Review combined changes and resolve correctness findings.
- [x] Update current docs affected by changed behavior.
- [x] Run format, portable suite, native host UI check and portable lint.
- [x] Assess Windows build path: unavailable; macOS inspection build produced.
- [x] Record seven Pass / Fail / Not run outcomes and exact remaining native limits.

Final portable suite: 405 passed, zero failed or ignored. Format, Clippy,
native-host UI check, macOS MCP inspection build and diff check pass.
Rendered setup inspected; full MCP interactive journey did not complete.
Windows CFAPI/process/save-publication execution and Windows delivery are
Not run. See docs/validation/2026-10-28.md. No native acceptance is claimed.

The plan already fixes the architecture and behavior. Design comparisons stay
within existing ownership boundaries. T3 tools and model sheet are unavailable;
implementation uses the available collaboration tools with inherited models.
Different-family review is unavailable and cannot be reported as passed.
No PR or commit is requested, so PR-opening and history-rewrite steps are skipped.
