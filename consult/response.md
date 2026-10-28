**You’ve already provided enough evidence to start fixing this.** I’ve written
one handoff that tells Sol to implement seven bounded repairs, with exact code
targets and completion checks.

### :chatgpt-content-reference{index="2"}[Download ROMMFS-REPAIR-PLAN.md](sandbox:/workspace/scratch/494ddf8b6dde/deliverables/ROMMFS-REPAIR-PLAN.md)

## What is actually wrong

The current source contains concrete problems that explain why the app is
frustrating:

- **It blocks work using the wrong check.** `stopped_retrobat_guard` opens five
  executables exclusively for writing. Failure means it couldn’t obtain file
  access; it does **not** establish that you launched RetroBat. That check gates
  setup, metadata publication, and support-file downloads.
  controller.rs[Source](sandbox:/workspace/scratch/494ddf8b6dde/review/codebase/crates/rommfs-app/src/controller.rs)
- **It invents pending library changes.** Every successful refresh sets
  `pending_remount` whenever the folder is mounted, without checking whether the
  projected files actually changed. Separately, the UI can retain successful
  RetroBat setup after receiving a failed setup check.
  catalogue.rs[Catalogue logic](sandbox:/workspace/scratch/494ddf8b6dde/review/codebase/crates/rommfs-app/src/controller/catalogue.rs)
  ·
  state.rs[State handling](sandbox:/workspace/scratch/494ddf8b6dde/review/codebase/crates/rommfs-app/src/controller/state.rs)
- **Normal stop/start destroys and rebuilds the game placeholders.** That
  creates substantial avoidable filesystem work. It isn’t necessary to establish
  whether Defender is contributing before removing this work.
  windows.rs[Source](sandbox:/workspace/scratch/494ddf8b6dde/review/codebase/crates/rommfs-cfapi/src/windows.rs)

Some screenshot problems have already received fixes in the supplied code. The
handoff explicitly preserves those improvements instead of assigning them again.

## The repair sequence

| Step                               | Explicit change                                                                                                                                                                                          | Required result                                                                                                      |
| ---------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| **1. Remove the executable locks** | Check actual running applications only where a particular write needs them closed. Defer that write, with a modest retry interval.                                                                       | A file-access check stops blocking unrelated work or falsely telling you to close RetroBat.                          |
| **2. Reuse the games folder**      | Normal Stop/Quit disconnects the provider and retains placeholders. Restart changes only entries that actually changed.                                                                                  | An unchanged restart creates/deletes **zero placeholders**. Existing files and cached content survive.               |
| **3. Remove unnecessary work**     | Move cold loading and mount preparation off the command loop. Stop recopying unchanged artwork and repeatedly scanning the same tree during one publication.                                             | Commands remain responsive during slow jobs, and repeated updates avoid needless disk writes.                        |
| **4. Make statuses truthful**      | Inspect actual RetroBat links, clear stale success, compare library contents before requesting remount, and distinguish partial publication from success.                                                | Pages agree; deleted links offer repair; unchanged refreshes stop demanding “Apply library changes.”                 |
| **5. Fix the everyday flow**       | Setup becomes **Connect → choose folders/RetroBat → connect RetroBat → start**. Reconnect preserves your page. Saves opens on Tracked; technical internals stay in details.                              | Setup requires no backtracking, and normal use has a clear next action.                                              |
| **6. Finish save recovery**        | Add **Keep this PC’s save** and **Use this RomM copy**, with the latter limited to a verified copy. Preserve the losing copy and persist the resolution through both download and upload reconciliation. | Resolving a conflict actually resumes syncing and stays resolved after restart. Export is no longer the only action. |
| **7. Verify and deliver**          | The agent checks the changed behavior with existing fixtures/tooling and produces the Windows build through the existing build path where available.                                                     | A concrete build and short Pass / Fail / Not run results—not another request for a baseline session.                 |

**The Stop/Quit behavior change is intentional and visible in the plan:** game
entries remain on disk, while new downloads require RomMFS to be running.
Windows provides separate operations for disconnecting a provider and
unregistering its root; unregistering traverses and converts or deletes
placeholders.
[Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfdisconnectsyncroot?utm_source=chatgpt.com)

The handoff also explicitly forbids **rewrites, new frameworks, speculative
hardening, feature cuts, configuration resets, and system-wide Defender or
permission changes**. It preserves the protections that prevent losing your
files and saves.

Give Sol that **one file**. Its opening instruction is to implement the repairs,
not produce another plan. The application code is unchanged here; the
deliverable is the implementation handoff.

