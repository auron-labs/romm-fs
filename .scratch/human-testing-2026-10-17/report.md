# RomM-FS human testing — 17 October 2026

**Overall: PARTIAL.** Core mount/gameplay and ordinary-save transfers worked on the user's real setup. Metadata publication failed. Conflict protection was not conclusively exercised because a manually uploaded, renamed file was rejected by the owned-revision format check. Several UI responsiveness and status problems were observed or reported. This is not complete Windows acceptance.

All 24 screenshots supplied in this chat are preserved, unmodified, in `screenshots/` and embedded below. `screenshot-manifest.csv` records original paths and SHA256 hashes; copied files were verified against their sources. Screenshots contain personal paths, server identity, and unrelated desktop content. Nothing in this bundle was uploaded by the assistant.

## Environment and evidence limits

- Date: 2026-10-17, Australia/Sydney.
- User PowerShell: 7.6.6. Interactive Windows client; exact Windows edition/build not established (assistant CIM queries were denied).
- Repository: `C:\Users\azza\Documents\Code\romm-fs`. HEAD at report collection: `84068e6f68aec959a12436c0dd4f15c4eed57ae1`. The checkout contained extensive pre-existing staged/unstaged changes; HEAD does not identify the running executable's exact source.
- Real RomM server: `https://romm.jcaks.net`; server version and permissions were not independently established.
- Managed root: `C:\Users\azza\RomM`. RetroBat: `C:\RetroBat`; app displayed checked version `8.3.0-stable-win64`.
- Catalogue displayed 24 platforms, 2,466 games, 11,531 files and 4,563 files not shown. Displayed counts were not audited against server inventory.
- Pokémon battery save: `C:\RetroBat\saves\gb\Pokemon - Red Version.srm`, 32,768 bytes. Golf save also existed, 8,192 bytes.
- Human operations and pasted command outputs are evidence where stated. Screenshots establish displayed states, not internal correctness. No request tracing, timing profile, clean-host driver audit, or second-client test was performed.

## Results

| Check | Result | Evidence and limits |
| --- | --- | --- |
| Connection/catalogue | Observed working | Connected UI and populated catalogue. Fresh authentication/scopes not independently tested. |
| Mount | PASS for exercised path | Activity recorded mount in 21.3 seconds; UI reached ready. Metadata then failed separately. |
| Folder enumeration | PASS | User manually browsed platform directories and GB filenames. No proof of zero downloads during enumeration. |
| ROM reads | PASS for repeatability | Golf SHA256 matched across repeated reads and stop/start. Source ROM hash was not obtained, so source-byte accuracy remains unverified. |
| Cache reuse | UNVERIFIED | Repeated hashes alone do not prove no additional network transfer. |
| Stop cleanup | PASS for selected file | `Test-Path ...\gb\Golf.gb` returned False. Remaining folders and gamelists are expected retained frontend state. No complete-tree cleanup audit. |
| Remount | PASS | Golf readable again with same hash. |
| Close to tray | PASS, user confirmation | Hash remained readable and tray reopened the window. |
| Explicit Quit cleanup | PASS for selected file | App exited and Golf disappeared without pressing Stop first. Original concern about exit leaving ROMs behind did not reproduce in this case. |
| RetroBat launch/listing/gameplay | PASS, user confirmation | Real RetroBat opened, listed Golf, and launched gameplay. Artwork/descriptions absent. No substitute process used. |
| Battery-save creation | PASS | Pokémon saved through gameplay; file present outside managed ROM root. |
| Automatic mapping | PASS after Refresh | Golf and Pokémon linked after Save sync Refresh. New-file discovery without manual refresh not established. Sync was already enabled when first inspected. |
| First upload | PASS, byte verified | RomM displayed save; downloaded file hash matched local `19061B…`. |
| Automatic changed-save upload | PASS, byte verified | Gameplay changed local hash to `A18B88…`; new verified revisions appeared without requested manual Refresh; newest download matched. |
| Restart persistence | PARTIAL | Mapping survived, no new RomM versions reported, but UI stayed Syncing/2 outgoing for several minutes before settling itself. |
| Remote-only update | PASS for exercised sequence | User uploaded older save with sync disabled, re-enabled, and confirmed application. Latest progress subsequently restored; local `A18B88…` matched saved download. |
| Two-sided conflict | INCONCLUSIVE / BLOCKED | Earlier attempt may have re-uploaded unchanged baseline. Final valid byte divergence used renamed `01-original.bin`, which triggered unverified-owned-format/ambiguous-history rejection before intended conflict handling. |
| Preservation on ambiguous remote input | Observed safe pause | UI showed needs attention and local `D4B1A8…` hash, plus “no remote file was adopted.” No independent final hash or remote inventory audit after this pause. |
| Offline queue, locking, cancellation, second device | NOT RUN | No claims from portable tests or other scenarios. |

## Findings

1. **Metadata publication failure — high functional impact.** Mount succeeds, then artwork/gamelists fail with `unsupported: save path traverses a symbolic link or reparse point: C:\Users\azza\RomM`. Existing frontend metadata is retained and gameplay still works. Source inspection found metadata publication invoking the shared save-path reparse guard. Hypothesis: it rejects the app's CFAPI-managed root; native reparse tag/ownership diagnosis remains needed. Do not broadly relax save-path protections as a workaround.
2. **Play-data permission warning — separate blocker.** Activity reported favourites/history unavailable with 403 Forbidden. Token scopes and endpoint permissions not verified. Do not conflate with the filesystem metadata failure.
3. **Open games folder wrong destination — user-reported.** User said it opened home (`~/`). Screenshots show subsequent manual navigation to the correct root, not the initial button destination. Needs isolated reproduction.
4. **Stop status update — observed by user.** Two Stop clicks appeared ineffective; navigating Activity then Overview revealed stopped. File absence confirmed cleanup. Distinguish delayed shutdown from missing repaint/event notification.
5. **Quit feedback — user-reported.** Taskbar entry remained during shutdown and could not reopen. App eventually exited and cleaned the selected ROM correctly.
6. **Hover lag — user-reported estimate ~100 ms.** Not measured. Activity presents many event rows/raw revision IDs and large logs; no causal performance diagnosis performed.
7. **Save-sync status lag/inconsistency.** After restart, Syncing/2 outgoing persisted despite repeated reconciliation and no new server versions. Assistant read local log tail and saw reconciliation roughly once per minute. It eventually settled without Refresh. Refresh then displayed Disabled → Syncing → Synced despite no user disable action.
8. **Misleading scan state.** “No save scan yet” remained visible after linked saves and verified uploads.
9. **Activity presentation.** Captured/uploading/verified stages remain separate raw-ID rows; historical stages appear alongside current aggregate work. Status sometimes still said outgoing after verified events. Presentation makes actual pending work hard to determine.
10. **Backup classification.** Final review lists `.rommfs-backup-…` files as excluded with “shared saves/cards are not supported.” This wording appears misleading for app-generated ordinary-save backups; classification not investigated.
11. **Manual upload format restriction complicates recovery/conflict tests.** A content-verified `01-original.bin` upload was rejected as outside the verified owned revision format, with six ambiguous incoming history entries. This demonstrates protective refusal, not failure of the intended two-sided conflict algorithm. Whether renamed browser uploads should be supported is a contract/product question.

## Automated checks attempted

Commands ran through `mise x -- mbx` as required. No complete suite pass:

- `mise x -- mbx test --locked --workspace -- --test-threads=1`: compilation failed in `crates/rommfs-cfapi/tests/cfapi_native.rs` lines 391 and 418: undefined `gamelist`.
- `mise x -- mbx test --locked -p rommfs-core -p rommfs-fixture -p rommfs-app -- --test-threads=1`: compilation failed on unresolved `windows_sys::Win32::System::Memory::LocalFree` in `retrobat_setup/windows.rs:151`, and undefined `NoopHydratedRemover` in controller mount tests at line 260.
- Sandbox initially blocked mise tool-directory access; test runs were retried with approved escalation. Native host inventory via CIM was denied.
- Existing compile errors were not fixed in this testing session. No zero-test or compile-only result is counted as runtime acceptance.

## Save identity ledger

| Label | SHA256 | Meaning |
| --- | --- | --- |
| 01-original | `19061BAE6B649699EB37BA069E058B213AEAC05CF5C08A43041CC53B9B93BBA7` | Initial local save and first verified server download. |
| 02-synced-baseline | `A18B8806132EB90F63BB5C1ECED68B0FAB402050B008BC1099674FF831B3A2A1` | Changed gameplay save, verified upload and later restored local baseline. |
| 03-local-conflict-candidate | `29F1EA83F891171B9F2234EA83B8F1EBDCAE768B4E5692FAB13E92A828B82E0D` | First conflict candidate; uploaded during early attempt, so no longer a reliable unsynced local change. |
| New local candidate | `D4B1A8D21F587D4B10FA8776FAAD725AC223097D0E0A30B61009D1244F6C8504` | Additional gameplay save created with sync disabled for final attempt. |

Final pre-enable comparison independently confirmed local `D4B1A8…` vs downloaded latest remote `19061B…`. On re-enable, the app paused on unrecognised revision format rather than presenting a normal conflict. The assistant's earlier assertion that conflict protection had failed was premature and is superseded by this inconclusive result.

## Helper and retained evidence

Created `scripts/save-sync-check.ps1` and `docs/testing/save-sync-helper.md`. The helper snapshots local saves, labels known content hashes, inventories downloads/backups, compares a selected download, and waits for an expected local hash. It does not upload, toggle consent, modify live saves, or resolve conflicts. Labels are specific to this session.

Executed synthetic checks: matching comparison, mismatch detection, watch timeout, and empty inventory; all passed. Ran actual inventory and hash-verified local/download copies. This is helper verification, not an application acceptance pass.

Labelled copies originally staged under `target/save-sync-evidence/20261017-200441-88e8657d`. Final comparison evidence reported by user: `C:\Users\azza\AppData\Local\Temp\rommfs-save-checks\20261017-200931-ef9c6f66`. A copy of available helper evidence is included under `save-evidence/`. It contains real gameplay saves.

## Handoff

- Last observed: mount ready, metadata error, save sync needs attention, local details show new `D4B1A8…` progress. Assistant asked to disable sync; user has not confirmed doing so. Do not assume final consent state.
- Keep all local backups and remote versions. No deletion/reset or journal manipulation was performed by the helper.
- Fix/investigate metadata root validation, UI event updates/status lag, and Activity responsiveness separately.
- Establish the intended manual-revision contract before repeating conflict testing. Use a controlled fixture or second supported client that produces valid owned revisions; do not equate arbitrary browser-upload rejection with conflict handling.
- Remaining acceptance includes valid two-sided conflict, locking, offline retry, interrupted transfers, cancellation/session changes, driver-free clean-host checks, and cache/request-tracing evidence.

## Screenshot evidence

Screenshots follow in conversational order. Original filenames are preserved.

### 01 — Connected catalogue; games folder stopped; RetroBat ready

![01 — Connected catalogue; games folder stopped; RetroBat ready](screenshots/rommfs-app_qVG5bdhZKj.png)


### 02 — Games folder starting

![02 — Games folder starting](screenshots/rommfs-app_PaxHrgvogh.png)


### 03 — Mount progress and Activity log

![03 — Mount progress and Activity log](screenshots/rommfs-app_aicyKCa4eG.png)


### 04 — Mount running; frontend metadata error and permission warning

![04 — Mount running; frontend metadata error and permission warning](screenshots/rommfs-app_Gxfitb5tpA.png)


### 05 — Games folder ready; metadata needs attention

![05 — Games folder ready; metadata needs attention](screenshots/rommfs-app_sSDoEcKQLD.png)


### 06 — Manual Explorer navigation to managed root

![06 — Manual Explorer navigation to managed root](screenshots/codex-clipboard-d97e601e-4f17-41a1-8712-9bca2a9073e1.png)


### 07 — Retained metadata index

![07 — Retained metadata index](screenshots/explorer_S0wlHEIBIe.png)


### 08 — Game Boy directory enumeration

![08 — Game Boy directory enumeration](screenshots/explorer_iu6xfI2qJL.png)


### 09 — Golf and retained gamelist visible

![09 — Golf and retained gamelist visible](screenshots/codex-clipboard-55a00bff-2e49-44ea-8232-d6306c157964.png)


### 10 — Remount ready; metadata error persists

![10 — Remount ready; metadata error persists](screenshots/rommfs-app_vOwSt0Uck7.png)


### 11 — Overview before save inspection

![11 — Overview before save inspection](screenshots/rommfs-app_Xcqr0BfarJ.png)


### 12 — Save sync enabled but zero mapped saves before Refresh

![12 — Save sync enabled but zero mapped saves before Refresh](screenshots/rommfs-app_nye92MZPni.png)


### 13 — Refresh links Golf and Pokemon saves

![13 — Refresh links Golf and Pokemon saves](screenshots/codex-clipboard-23259219-6856-4cb7-8f33-93c1f742c66d.png)


### 14 — Activity captured/uploading/verified rows and outgoing status

![14 — Activity captured/uploading/verified rows and outgoing status](screenshots/codex-clipboard-122e06d0-99b7-4b02-ac9f-1c8486d7d147.png)


### 15 — Activity layout and reconciliation log

![15 — Activity layout and reconciliation log](screenshots/rommfs-app_b02GtaRVMy.png)


### 16 — Pokemon save visible in RomM

![16 — Pokemon save visible in RomM](screenshots/codex-clipboard-78615149-1323-4ccd-8c2a-2fcb3335319d.png)


### 17 — Further verified revisions; linked saves up to date

![17 — Further verified revisions; linked saves up to date](screenshots/codex-clipboard-dca3dc46-bd80-4dd4-9d98-6745697ff402.png)


### 18 — Expanded Pokemon row with matching local/remote baseline hashes

![18 — Expanded Pokemon row with matching local/remote baseline hashes](screenshots/codex-clipboard-50bdd8e9-e82c-49ed-a0cf-974b0e175e5d.png)


### 19 — Restart Activity remains syncing with two outgoing

![19 — Restart Activity remains syncing with two outgoing](screenshots/codex-clipboard-1cec1022-53e4-4744-af40-ff529ffc915c.png)


### 20 — Restart Save sync retains mappings but shows syncing

![20 — Restart Save sync retains mappings but shows syncing](screenshots/codex-clipboard-8b1649ad-eb5e-4109-bb6e-2cbda42db188.png)


### 21 — Remote version history during initial inconclusive conflict attempt

![21 — Remote version history during initial inconclusive conflict attempt](screenshots/codex-clipboard-ff7f1fe5-f61e-4780-b839-03ff5346d970.png)


### 22 — Final overview: save sync needs attention

![22 — Final overview: save sync needs attention](screenshots/codex-clipboard-7a41b6b0-0ac8-4762-bb02-f468d2aab712.png)


### 23 — Unverified revision format, ambiguous incoming history, local D4B1 preserved

![23 — Unverified revision format, ambiguous incoming history, local D4B1 preserved](screenshots/codex-clipboard-a058e9fb-7a5b-420b-9d86-4cac175b0e0b.png)


### 24 — Activity confirms no remote file adopted

![24 — Activity confirms no remote file adopted](screenshots/codex-clipboard-786a6fe5-1d97-4101-8b21-9818e939cd06.png)

