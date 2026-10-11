# RomMFS: human-friendly desktop UI implementation plan

## Task for the implementation agent

Implement this plan in the existing RomMFS repository. This is a focused improvement to a Rust/GPUI desktop sync client, not a product rewrite. Inspect the existing code, then work through the tasks in order. Do not respond with another proposal instead of implementing.

**Product goal:** Connect to RomM, make the user's games available through a local folder with on-demand downloads, and make optional save sync understandable. After setup, users should usually need only a status check or “Open games folder”.

The review is based on the supplied screenshots, not a source-code inspection. Treat the observations below as reproduction targets, not proven diagnoses of the underlying code.

### Scope boundaries

Keep Rust, GPUI, existing dependencies, configuration, authentication, platform adapters, and working download/mount/sync code. Reuse the installed component library where present. Do not upgrade the UI stack to accomplish this work.

Preserve existing installations and the user's opt-in/no-silent-overwrite requirement for saves. Do not add new restrictions, mandatory reinstalls, permission policies, or save-format exclusions under the guise of improving UX. Do not silently change reconciliation, deletion, or overwrite behavior. Flag any existing conflict with the user's requirement explicitly.

Do not add a game launcher, cover-art browser, new filesystem engine, cloud service, telemetry, generic workflow framework, or a comprehensive emulator compatibility project. Fix reproducible defects directly involved in these screens; keep unrelated engine changes separate.

CFAPI is a Windows API. Inspect and preserve the actual Linux implementation; do not assume CFAPI behavior or Windows-only fixes apply there. [S5]

---

## 1. What the screenshots reveal

| Observation | Required improvement |
|---|---|
| “Connected” is prominent while the games folder is not mounted. | Identify this specifically as the **RomM connection**, separately from folder readiness. |
| Save sync shows “Up to date” with zero mapped files; another view exposes a paused/unsupported message far below. | Derive the summary from the actual sync state and make a current blocker visible at the top. Reproduce whether these states occur together. |
| Sign-in, cancel-sign-in, sign-out, and password fallback appear together. | Render the controls appropriate to the current authentication state. |
| Settings mixes account management, startup, RetroBat installation, folder linking, and support-file downloads. | Separate everyday settings from optional setup tasks. |
| Save review exposes 1,855 unmatched files, repetitive mapping controls, and “…1212 more folders”. | Provide a searchable review table with reachable results, not a truncated diagnostic dump. |
| “Map group to game” can refer to emulator-wide folders. | Make the selected files and destination game explicit. A folder is not proof that its contents belong to one game. |
| Setup failures appear at the bottom as raw operating-system errors. | Put a concise explanation and relevant recovery action beside the failed operation. Keep technical details available. |
| Repeated full-width buttons, small muted text, and weak grouping give everything similar visual weight. | Establish clear headings, compact actions, readable text, and a primary action. |
| “Closing RomMFS stops the mount” conflicts with the explanation that closing hides it in the tray. | Verify close versus quit on each platform and use one accurate explanation. |
| The activity page foregrounds a large stream of repeated warnings. | Show operations and outcomes first; retain the full technical log separately. |

These are not all cosmetic issues. A correct layout cannot compensate for false status, inaccessible results, or failures without useful context.

## 2. Target structure

Use a compact left navigation with four destinations:

| Destination | Purpose |
|---|---|
| **Overview** | Is the games folder working? What needs fixing? Open the folder. |
| **Save sync** | Understand sync status and review files that need a decision. |
| **Activity** | See running operations, recent results, and diagnostic details. |
| **Settings** | Change connection, folders, optional RetroBat integration, and startup preferences. |

Place the server host and explicitly labelled connection status at the bottom of the sidebar. Remove the oversized duplicate app-name header. Retain normal platform window controls.

Do not show an application-wide green “everything is fine” indicator merely because authentication succeeded. An optional save-sync or RetroBat problem must also not make a working games folder appear broken.

### Visual specification

Use the existing theme with clearer semantic roles rather than building a theme system. The proposed layout targets are:

- Approximately 190 logical pixels for navigation; 24 pixels around page content; 16 pixels inside sections; 8–12 pixels between related controls.
- Approximately 24-pixel page headings, 16-pixel section headings, and 14-pixel body text. Reserve smaller text for secondary metadata, not essential instructions or errors.
- Approximately 36-pixel controls; modest 6–8-pixel corner radii; neutral surfaces and restrained borders. Blue for the main action; semantic warning/error/success colors accompanied by words or icons.
- A maximum readable content width around 960 pixels. Let tables use available space. At smaller widths, stack sections rather than clipping controls.

Use one filled primary action for the current page/task; secondary actions should not compete with it. Use stable-label switches for immediate preferences such as “Start at login”, not buttons that alternate between long enable/disable sentences. [S2]

Use the platform UI font, visible keyboard focus, logical tab order, and keyboard-operable dialogs. Return focus to the initiating control when a dialog closes. Preserve full paths through copy/details even when their visual display is shortened. Aim for at least 4.5:1 contrast for normal text; do not use faded helper text for blockers. [S2]

Implement within the existing GPUI layout and event model. The project’s pinned version and examples are authoritative for APIs; current online examples are only a reference. [S4]

## 3. Overview: everyday use

The default screen should contain a page heading and three compact sections, ordered by importance.

### Games folder

Show a human-readable state, the configured folder, the relevant action, and a short library summary.

| Actual condition | Summary and action |
|---|---|
| No completed folder setup | “Set up your games folder” → **Set up games folder** |
| Configured but stopped | “Games folder stopped” → **Start games folder** |
| Starting | “Starting games folder…” with actual operation progress |
| Provider active and folder usable | “Games folder ready” → **Open games folder** |
| Start failed | “Couldn’t start the games folder” → recovery action with **Retry** when applicable |

“Ready” means the folder/provider is usable, not that every game has downloaded or every emulator can run it. Do not infer this state merely from the persisted enabled setting or the existence of a directory.

When active, place **Stop games folder** and **Refresh library** as secondary actions. Do not permanently display both Start and a disabled Stop. Explain the consequences of stopping based on the actual platform implementation, including whether already downloaded files remain accessible.

Display library counts with explicit meanings. The screenshots do not establish whether “11,531 ROMs” includes the “4,563 skipped” items. Trace those values before relabelling them. Make skipped items inspectable with grouped reasons; do not imply that they are all downloadable.

Library refresh is a metadata operation, not a download of all games. Label it “Updating game list…” and use the real units available. Do not relabel catalogue pages as games. Show determinate progress only with a reliable denominator, otherwise show the current stage. [S3]

Use brief on-demand copy such as: **“Game files download when an application opens them.”** Do not promise that only intentional gameplay can trigger a download, or that every displayed game is available offline. Windows placeholders can hydrate through application access. [S5]

### Save sync summary

Show the same derived status used on the Save sync page, with a secondary **Set up save sync** or **Review save sync** action. Keep it independent of games-folder readiness. Do not duplicate its configuration form here.

### RetroBat summary

Show this only when RetroBat is selected/configured or setup is incomplete. Present “Not configured”, “Ready”, or the current problem with **Set up RetroBat** / **Finish setup**. A detected installation is not proof that folder linking completed.

Do not fill unused space with statistics, log output, or new features.

## 4. Setup and settings

### First run

Use a short setup view for an unconfigured installation:

1. **Connect to RomM.** Server address and one primary **Sign in with browser** action. Password sign-in is a secondary alternative. While a browser sign-in is actually pending, show progress and Cancel. After authentication, replace these controls with the account/connection summary.
2. **Choose the games folder.** Offer the existing default and Browse. Preserve the existing choice of standard folders or RetroBat layout. Show only relevant fields and prerequisites for the selected layout. Finish by starting the existing setup operation and reporting its result.

Optional save sync and support-file downloads must not become prerequisites for a standard games folder. Respect real dependencies of the chosen RetroBat setup; do not rearrange folder-link operations without inspecting them.

Existing configured users go to Overview, even when the server is temporarily unavailable. Do not reset configuration or replay onboarding as an error recovery strategy.

### Settings organization

**Connection:** Show the authenticated account and server, with Change server and Sign out. Display sign-in controls only when needed. Do not display a blank account label with a raw database ID as the main identity.

**Folders and RetroBat:** Display the games folder and the selected RetroBat installation once. Save sync should reuse that installation rather than maintaining another independent selector for the same setting. A change must update every dependent screen. Put custom save-folder selection behind **Use a different save folder**.

**Background operation:** Use **Start at login** and accurately describe close/quit behavior. Expose only capabilities implemented on the current platform. Do not add an operating-system service or invent missing Linux tray support.

**Advanced:** Retain technical tuning such as save-change delay, but remove it from the default task flow. Use explanatory labels, for example “Wait after a save changes”, with seconds as the unit.

### RetroBat setup and support files

Use a focused setup panel, not an ever-growing settings form. Show the selected installation, the requested folder connection, current result, and one action.

For an existing path that prevents linking, show the exact path and detected item type. **Review existing item** should explain what the existing operation proposes to remove/replace and why. Preserve the actual data-handling policy; do not invent new permissions or tell users to reinstall solely because the old helper text says “fresh installation”.

For the screenshot’s privilege failure, use copy such as:

> **Couldn’t finish RetroBat setup**  
> Windows denied permission to create the games-folder link.  
> **View setup help** · **Retry** · Technical details

The details must include the actual link paths, failed operation, and OS error 1314. Inspect the link-creation implementation before prescribing a remedy. Do not claim Developer Mode fixes every link failure or make running the entire app as administrator a blanket requirement.

Move BIOS/firmware/keys/cheats into a separate **Support files** section with one **Download support files** action. Explain that these come from the connected RomM library. Show current item/count and the existing Cancel action only during a cancellable download. Optional support-file failures must not overwrite the games-folder status.

## 5. Save sync: status first, exceptions second

The default page should show the sync preference, operational status, selected source folder, last completed check when known, and a compact summary. Do not start with setup inputs or the raw file list.

### Make status truthful

Derive the view from actual state, rather than independent labels or the presence of a worker. Keep configured, enabled, running, and successfully checked distinct.

| Condition | Appropriate presentation |
|---|---|
| User has not enabled sync | **Save sync off**; no implied protection |
| Source configuration invalid | **Save sync needs setup** with the concrete reason and a fix action |
| Enabled but no completed check | **Waiting to check saves** / **Checking saves…** |
| No eligible linked files | **No files ready to sync**, with an explanation or review action |
| Transfers pending/running | **Syncing saves…** with actual counts where available |
| Current operation failed | **Save sync needs attention**, including the scope and failed operation |
| Linked files checked, no work pending | **Linked saves are up to date**, retaining a separate count of unlinked files |

Unknown state is not success. Zero pending uploads is not proof that saves were discovered or synchronized. Conversely, known exclusions should not permanently mark an otherwise working sync operation as broken.

If the backend reports partial success, preserve it: explain which files are syncing and which are not. Do not globally block healthy linked saves merely because unrelated files are unlinked.

### Replace the mapping dump

Open a focused review table from **Review files**. Provide search by path/game, status filters, a visible result count, and pagination using existing table/list components. Start with a bounded page such as 50 rows; never leave the remainder inaccessible behind “more folders”.

Useful statuses are **Linked**, **Not linked**, **Excluded**, and **Needs attention**. Use “Synced” only where a completed synchronization actually supports it. Keep these concepts separate.

Each row shows the game when known, platform when known, relative file/folder path, file count, reason, and relevant action. Selection state must use stable file/group identities, not row positions.

**Choose game** opens a searchable picker for that selection, shows the exact files affected, and commits an explicit association. Remove the global game selector that silently determines what later row buttons do. Bulk assignment is appropriate only when the selection genuinely belongs to one game; expose individual selection for mixed groups. Keep existing single-file mapping under an advanced action.

Do not call all 1,855 discovered files “saves needing attention”. The examples include emulator configuration and shared-looking containers, but file extensions alone do not prove their meaning. Use existing classifier knowledge; unknown items remain “Not linked” with an honest reason. Do not invent blanket `.cfg`, `.dat`, or memory-card rules.

Investigate why the supplied installation produces zero mapped files. If this is a reproducible resolver/path defect, fix it with a focused regression case. If the mapping genuinely lacks required information, explain the limitation. Do not replace diagnosis with a new fuzzy-matching engine or thousands of manual assignments.

Preview and association changes must not themselves upload, overwrite, or delete files unexpectedly. Show the selected game's platform/title, not only its internal ID.

## 6. Errors, progress, and Activity

### A shared presentation, not a new framework

Reuse existing operation results and error types. A small shared view helper is enough to give failures a title, explanation, relevant action, and expandable details. Place current failures at the top of the affected section; retain them until resolved or superseded, rather than burying them in a log or a disappearing toast. [S1]

For operations touched by this work, preserve the actual context: operation, target path/game, cause chain, OS/HTTP error, and retry outcome. Log start and final outcome using existing logging facilities. Use an existing operation identifier where available to connect the screen with its diagnostic entry.

Keep paths, filenames, and error codes useful. Do not add broad redaction that removes diagnostic context. Do not log passwords, tokens, cookies, or authorization headers.

Do not clear an error merely because Retry was clicked. Show the new attempt, then replace the prior result after receiving the actual outcome. Prevent duplicate submissions of the same active operation without disabling unrelated controls.

A disabled action needs an adjacent explanation and a reachable way to address it. Do not require hovering over a disabled button to discover why the main workflow is blocked. [S2]

### Activity page

Show active operations first, then recent results. Each row should identify what happened and to which item: updating the game list, downloading a ROM, syncing a save, setting up RetroBat, or downloading support files. Include all currently implemented operation types rather than showing “No active transfers” during another invisible background task.

Progress must represent work actually completed. Use bytes for transfers where available and real item/page counts for other operations. Do not invent percentages, rates, completion times, or successful outcomes. [S3]

Group repeated diagnostic messages by a stable reason, with a count and expandable affected-item details. Expected exclusions should be informational in the activity summary; failures and degraded behavior remain visible. Do not suppress the underlying logs or downgrade every warning indiscriminately.

Keep **Technical log**, **Open log folder**, and **Copy details** available. The technical log must remain searchable and copyable, but it is not the default representation of app health.

## 7. Implementation order

Complete these as small, reviewable changes. Reuse real repository paths; the task names below are not instructions to create a parallel architecture.

### Task 1 — Trace current behavior and fix misleading state

Read the manifest/lockfile, UI entry points, configuration, operation handlers, and status sources. Briefly map the four existing screens to their handlers and authoritative state. Identify the actual Windows/Linux adapters.

Reproduce the contradictory statuses, disabled enable-sync action, zero mappings, and stale setup errors. Add only the missing result/context plumbing necessary. Fix confirmed defects locally; distinguish unverified causes.

**Done when:** Connection, folder, RetroBat, and save-sync states cannot imply one another's success; current blockers appear beside their actions. Add regression tests for the status combinations actually found.

### Task 2 — Build the shell and Overview

Implement the navigation, spacing/type hierarchy, games-folder section, and compact optional-feature summaries. Reuse shared presentation helpers only where repeated use justifies them.

**Done when:** A configured user can see whether the folder works, start/retry it when needed, and open it without visiting Settings or reading the log. Switching pages does not stop background work or reset progress.

### Task 3 — Simplify connection and optional setup

Implement the short first-run flow and settings organization. Remove duplicate fields and irrelevant authentication controls. Move RetroBat/support-file operations into focused sections with local progress and results.

**Done when:** Existing configuration survives; setting up a standard folder does not require save sync or RetroBat. Failed optional setup has an understandable next step.

### Task 4 — Replace save review

Implement the truthful summary, searchable paginated table, per-selection game picker, and explicit affected-file preview. Resolve confirmed mapping defects found in Task 1 without expanding the supported-format scope.

**Done when:** Every reported result is reachable; the screenshot-scale dataset is navigable; zero linked files cannot appear as fully synchronized; mapping one selection cannot affect another unintentionally.

### Task 5 — Improve Activity and finish UI consistency

Present running operations and recent outcomes, group noisy repetitions, and retain technical diagnostics. Apply consistent copy, keyboard behavior, error placement, and control sizing to all changed screens.

**Done when:** A user can identify a failed operation and copy useful evidence from the app itself.

### Task 6 — Verify the real workflow

Use the checks below. Fix observed failures before describing them as resolved. Do not turn this into an unrelated testing-platform project or a sequence of mandatory owner approvals.

## 8. Focused verification and handoff

Use existing tests and fixtures. Add cases that exercise real user-visible state transitions, not just assertions that widgets render.

| Scenario | Required evidence |
|---|---|
| Signed out → sign-in pending → authenticated | Correct controls at each step; no contradictory sign-in/sign-out UI. |
| Start folder succeeds or fails | Final status comes from the actual result; failure includes its target and cause. |
| Cached library exists; refresh fails | Failure remains scoped to refresh; retained usable state is not falsely discarded or marked freshly updated. |
| Save sync enabled with invalid source or zero linked files | No false “Up to date”; reason and relevant action visible. |
| Linked saves succeed while other files are unlinked | Partial coverage is explicit; valid work is not hidden or unnecessarily blocked. |
| Large review dataset, searching, paging, assignment | All results reachable; counts and stable selection remain correct. |
| Retry after failure; switch pages during work | No duplicate operation, stale success, lost progress, or cleared failure without a result. |
| Existing RetroBat link/path and permission failures | Correct affected path, non-destructive review, and context-specific help. |
| Resize and keyboard navigation | Main actions remain visible/reachable; dialogs restore focus; essential copy is readable. |

Then run the actual app on available supported environments. Using a small known test ROM, check that listing the folder does not cause an app-initiated bulk download and that reading the selected file completes through the real provider with the expected contents. Observe actual hydration/download events; do not infer success just because filenames appear.

Exercise save behavior using disposable test saves, not the owner's real saves. Check the actual opt-in, mapping, successful operation, and error path relevant to the changed code. Verify close-versus-quit behavior on the platform being tested.

Capture real screenshots of the implemented Overview, Save sync, Activity, and an actionable error. Fixture-driven screenshots are useful for rare states but must be labelled as fixtures, not runtime evidence.

If Windows or Linux execution is unavailable, continue with the available work and state exactly what remains unverified. Do not claim a mock proves CFAPI/Linux integration; do not block all progress on a manual cross-platform certification exercise.

Final handoff: summarize what changed, confirmed bugs fixed, checks actually run, screenshot locations, and remaining concrete limitations. A cosmetically improved interface is not evidence that the reported mounting problem has been fixed.

---

## Reference notes

These are design/technology references, not instructions to introduce React, Fluent packages, or a different GPUI version. The screen structure, dimensions, task sequence, and copy above are proposals specific to this app.

**[S1] Microsoft Fluent 2 — Message bar.** Contextual placement, concise explanation, and recovery actions.  
`https://fluent2.microsoft.design/components/web/react/core/messagebar/usage`

**[S2] Microsoft Fluent 2 — Button.** Primary-action hierarchy, settings switches, contrast, and explaining disabled actions.  
`https://fluent2.microsoft.design/components/web/react/core/button/usage`

**[S3] Microsoft Fluent 2 — Progress bar.** Determinate versus indeterminate progress and meaningful labels/units.  
`https://fluent2.microsoft.design/components/web/react/core/progressbar/usage`

**[S4] GPUI official site.** Rust UI framework and native layout/render examples; use APIs compatible with this repository.  
`https://gpui.rs/`

**[S5] Microsoft Learn — Build a Cloud Sync Engine that Supports Placeholder Files.** Windows-specific CFAPI, placeholder/full-file distinctions, and application-triggered hydration.  
`https://learn.microsoft.com/en-us/windows/win32/cfapi/build-a-cloud-file-sync-engine`
