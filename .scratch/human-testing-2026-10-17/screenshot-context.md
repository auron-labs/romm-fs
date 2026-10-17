# Screenshot timeline and test context

Human testing on 17 October 2026, Australia/Sydney. Read alongside [the results report](report.md). Images are ordered as supplied in the conversation; numbering matches that report. This is a conversational sequence, not a timestamp audit. Statements about actions outside an image come from the user's messages and pasted command outputs.

## 01 — Initial state after reopening RomM-FS

**Stage:** Establish connection, catalogue and existing RetroBat setup before mounting.

The user had closed RomM-FS and reopened it for testing. The overview shows RomM connected, a loaded catalogue, games folder stopped, and RetroBat ready at `C:\RetroBat`. Save sync says nothing to sync yet. The next requested action was Start games folder. This image does not establish whether the previous close used the tray or fully exited.

![Initial connected overview](screenshots/rommfs-app_qVG5bdhZKj.png)

## 02 — First mount is in progress

**Stage:** Start the existing managed games folder.

After clicking Start games folder, the user saw a yellow starting state and preparation progress. This is an intermediate state, not evidence of failure. The user initially suspected files retained after a previous exit were preventing the mount; later evidence separated successful mounting from metadata failure.

![Mount starting](screenshots/rommfs-app_PaxHrgvogh.png)

## 03 — Activity shows mount preparation advancing

**Stage:** Observe the first mount while work is underway.

Activity shows preparation at 7,552 / 16,603 and 14.8 seconds. Logs show checking/claiming the mount directory, opening the private cache, and starting Cloud Files. Save warnings already appear for emulator files and shared memory-card locations. The progress total and catalogue file count use different displayed figures; no audit established how those totals relate.

![Activity during mount preparation](screenshots/rommfs-app_aicyKCa4eG.png)

## 04 — Mount succeeds, frontend metadata fails

**Stage:** Distinguish mount outcome from post-mount metadata publication.

The green Games folder running operation and mount-completed log establish the mounted state. The error concerns frontend metadata: the managed root was rejected as a symbolic link or reparse point. A separate log warning reports 403 Forbidden for favourites/play history. The next test continued against the mounted filesystem despite the metadata problem.

![Mounted filesystem with metadata failure](screenshots/rommfs-app_Gxfitb5tpA.png)

## 05 — Overview presents partial success

**Stage:** Confirm status after the initial mount completes.

Games folder ready and Stop/Open controls are available while Artwork & gamelists says Needs attention. This corroborates two distinct outcomes: the mount is running; metadata publishing is not healthy. The user was asked to open the games folder without launching RetroBat yet.

![Ready mount and failed artwork section](screenshots/rommfs-app_sSDoEcKQLD.png)

## 06 — Managed root manually opened in Explorer

**Stage:** Check platform folder enumeration.

The user reported that Open games folder took them to home and then supplied Explorer views of `C:\Users\azza\RomM`. This image shows the correct root after navigation, with platform folders and retained `.rommfs-metadata`. It supports successful directory listing, but does not capture the wrong initial destination or prove enumeration caused no content downloads.

![Managed root platform folders](screenshots/codex-clipboard-d97e601e-4f17-41a1-8712-9bca2a9073e1.png)

## 07 — Existing metadata index inspected

**Stage:** Understand retained files rather than deleting them.

Explorer shows `.rommfs-metadata/index.json` with an older modification date. Retained frontend state was explained as expected; it is not by itself evidence of failed ROM cleanup. No changes to this index were requested or performed during this inspection.

![Metadata index folder](screenshots/explorer_S0wlHEIBIe.png)

## 08 — Game Boy ROM filenames enumerate

**Stage:** Check game entries before deliberately reading a ROM.

The GB folder shows named `.gb` entries and supporting directories. This demonstrates that the mounted view exposes game paths. Visible file sizes and icons do not establish hydration state or correct bytes. The next substantive filesystem check used Golf's SHA256 twice.

![Game Boy filenames in Explorer](screenshots/explorer_iu6xfI2qJL.png)

## 09 — Golf and an older gamelist are visible

**Stage:** Select a small ROM for read testing and inspect retained metadata.

Golf.gb appears alongside gamelist.xml with an older timestamp. Subsequent pasted PowerShell output showed two identical Golf hashes. The older gamelist helped explain why RetroBat could list games even though the current metadata refresh failed; its exact provenance and contents were not audited here.

![Golf entry and gamelist](screenshots/codex-clipboard-55a00bff-2e49-44ea-8232-d6306c157964.png)

## 10 — Successful remount after Stop cleanup

**Stage:** Verify stop/start lifecycle and repeatable reads.

Between images 09 and 10, Stop appeared unresponsive until the user changed pages. PowerShell then confirmed Golf.gb was absent. After Start, Golf's SHA256 again matched the previous reads. This overview confirms ready state with the same metadata error. Cache reuse was requested but not proven by transfer tracing. Later, the user confirmed close-to-tray and explicit Quit tests worked; those tests have no dedicated screenshots.

![Ready after remount](screenshots/rommfs-app_vOwSt0Uck7.png)

## 11 — Entering save-sync tests after real gameplay

**Stage:** Transition from mount/gameplay testing to battery-save detection.

The user had launched RetroBat and Golf successfully, then created a Pokémon Red in-game save. Pasted filesystem output located the new 32-KiB save under `C:\RetroBat\saves\gb`, outside the ROM mount. Overview still says nothing to sync yet and offers Review saves. Update ready refers to the games-folder snapshot; it was not investigated in this step.

![Overview before examining saves](screenshots/rommfs-app_Xcqr0BfarJ.png)

## 12 — Save sync was enabled, but no ordinary saves were mapped yet

**Stage:** Inspect existing consent and scan state before changing settings.

Disable save sync indicates syncing was already enabled, contrary to the earlier assumption that enabling would be a future step. The page shows zero mapped, three unmatched, and 1,857 ignored, with shared-card exclusions. Pokémon is absent from the visible review. The next instruction was one Refresh, not manual linking.

![Zero mapped before refresh](screenshots/rommfs-app_nye92MZPni.png)

## 13 — Refresh automatically links Golf and Pokémon

**Stage:** Verify automatic filename/game mapping after a manual rescan.

Both `.srm` files now appear as Linked and the page says Syncing saves. This establishes mapping after Refresh, not successful upload by itself. The continued No save scan yet badge is inconsistent with the visible mapped files. The user next inspected Activity and the RomM web save list.

![Two mapped saves after refresh](screenshots/codex-clipboard-23259219-6856-4cb7-8f33-93c1f742c66d.png)

## 14 — First upload events and confusing Activity presentation

**Stage:** Observe outbound capture/upload/verification.

Activity displays separate captured, uploading and verified rows for both games using numeric IDs and revision UUIDs. The aggregate still shows Syncing saves / 2 outgoing. In the same message the user reported Activity looking odd and hover feedback lagging by roughly 100 ms. The image cannot measure input latency or establish its cause. Unrelated desktop content overlaps the upper-right area.

![First verified upload activity](screenshots/codex-clipboard-122e06d0-99b7-4b02-ac9f-1c8486d7d147.png)

## 15 — Same upload stage, scrolled Activity/log view

**Stage:** Additional evidence of Activity layout and reconciliation messages.

The second Activity view shows the same upload stages plus reconciliation logs for Golf and Pokémon. Repeated warnings for emulator support/configuration files fill much of the log. This gives context for the user's usability complaint; it does not prove the warnings caused the lag.

![Scrolled Activity and reconciliation logs](screenshots/rommfs-app_b02GtaRVMy.png)

## 16 — Initial Pokémon save appears on the real server

**Stage:** Independently inspect upload presence in RomM.

RomM's Pokémon Save data tab contains one version under the RomM-FS save group, about 32 KiB, with the first Pokémon revision filename. Later, downloading this entry and hashing it confirmed equality with local save `19061BAE…`. The screenshot establishes presence; the pasted hashes supply byte-integrity evidence.

![First Pokemon save in RomM](screenshots/codex-clipboard-78615149-1323-4ccd-8c2a-2fcb3335319d.png)

## 17 — Changed gameplay save uploaded automatically

**Stage:** Detect a new save without manually pressing Refresh.

The user continued Pokémon, changed progress, saved and exited. The local hash changed to `A18B8806…`. Activity now shows three additional verified Pokémon revisions and Linked saves are up to date. Multiple revisions may reflect intermediate emulator writes; redundancy was not established. A later download of the newest revision matched the local hash.

![Automatic subsequent uploads and idle status](screenshots/codex-clipboard-dca3dc46-bd80-4dd4-9d98-6745697ff402.png)

## 18 — Matching local/remote baseline in expanded details

**Stage:** Inspect what restore controls are available before changing local files.

The expanded Pokémon row shows equal local and remote `A18B8806…` hashes and a remote ID. No restore control is visible for this linked state. The assistant checked the testing contract and abandoned a proposed missing-local-file approach: intentional deletion must not trigger automatic restoration. Restart persistence was tested next.

![Expanded matching baseline hashes](screenshots/codex-clipboard-50bdd8e9-e82c-49ed-a0cf-974b0e175e5d.png)

## 19 — After restart, Activity appears stuck syncing

**Stage:** Verify mappings and idle state survive Quit/reopen.

The user reported it was stuck doing nothing. Activity shows two outgoing but no new captured/upload events, only reconciliation messages. Assistant log inspection found ongoing reconciliation roughly once a minute, so the worker was not proven frozen. The user confirmed no additional server versions. Later the status settled by itself before Refresh could be clicked.

![Restart Activity delayed syncing status](screenshots/codex-clipboard-1cec1022-53e4-4744-af40-ff529ffc915c.png)

## 20 — Restart retained mappings but scan/status labels remained misleading

**Stage:** Corroborate the restart state on the Save sync page.

Golf and Pokémon remain Linked while the header says Syncing and No save scan yet. This supports mapping persistence, with delayed or inaccurate aggregate feedback. Later the user reported Refresh temporarily showing Disabled → Syncing → Synced. That transient sequence was reported in text, not captured here.

![Restart Save sync mappings](screenshots/codex-clipboard-8b1649ad-eb5e-4109-bb6e-2cbda42db188.png)

## 21 — First attempted conflict test: version history is ambiguous

**Stage:** Determine whether both remote and local bytes actually changed.

Before this image, remote-only update testing and restoration of `A18B8806…` succeeded, with matching hashes. For the first conflict attempt, the user created local `29F1EA83…`, uploaded a file through RomM with sync disabled, then re-enabled; the app uploaded another version. This history shows the newest app revision and a manually uploaded entry carrying the old baseline revision filename. That filename raised doubt about which bytes were uploaded, but does not prove its content. The assistant retracted the initial conflict-failure claim and requested content verification. A helper was then written to reduce filename confusion.

![Version history during inconclusive first conflict attempt](screenshots/codex-clipboard-ff7f1fe5-f61e-4780-b839-03ff5346d970.png)

## 22 — Final controlled divergence triggers Needs attention

**Stage:** Re-enable after independently checking different local/remote bytes.

The helper labelled known files by hash. The user uploaded `01-original.bin`, downloaded Latest and verified remote `19061BAE…`. Because the `29F1EA83…` local version had already synced, the user made an additional local gameplay save with sync disabled, yielding `D4B1A8…`. After re-enabling, Overview shows save sync needs attention while the mount remains ready. The next images explain why this was not a normal conflict decision.

![Final attention state on overview](screenshots/codex-clipboard-7a41b6b0-0ac8-4762-bb02-f468d2aab712.png)

## 23 — Rejected revision format rather than ordinary conflict handling

**Stage:** Inspect the final protective pause and local preservation.

Expanded details show local `D4B1A8…`, the earlier recognised remote hash `29F1EA83…`, and the warning: one remote record is outside the verified owned revision format; no remote file was adopted. Six rows are labelled ambiguous_remote_history, and app-generated backup paths appear among exclusions. The manually renamed `01-original.bin` upload introduced the format issue. This demonstrates rejection/pause, not a conclusive pass or failure of the intended two-sided conflict algorithm. Local preservation is supported by the displayed hash; no final independent hash was supplied after this screenshot.

![Detailed rejected remote format and ambiguous history](screenshots/codex-clipboard-a058e9fb-7a5b-420b-9d86-4cac175b0e0b.png)

## 24 — Activity confirms the same protective refusal

**Stage:** Corroborate the final save-sync outcome in logs.

Activity says six incoming / one need review, and logs state no remote file was adopted because of the owned-revision format restriction. The assistant asked the user to disable save sync and preserve all copies; the user did not explicitly confirm the final toggle state before requesting reports. Further conflict testing should use a valid revision producer or a controlled fixture rather than renamed browser uploads.

![Activity warning for rejected remote record](screenshots/codex-clipboard-786a6fe5-1d97-4101-8b21-9818e939cd06.png)

## Stages without screenshots

The conversation also records successful repeated Golf hashing, selected-file Stop/Quit cleanup, close-to-tray, RetroBat gameplay, battery-save creation, local/server byte comparisons, remote-only application and restoration, and helper comparisons. Those are documented in [the main report](report.md), but none should be attributed to an image that does not show the action. Hover timing, cache reuse, and a valid normal conflict decision were not independently measured or established.
