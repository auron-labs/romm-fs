## GUI
- ui has no visual indicator that it's mounting apart from the yellow light next to mounting...
- there's no logging while its mounting
- the window has no scroll, so the log area gets squashed
- doesn't use the romm browser method to login
- doesn't save credentials for next open
- doesn't allow for "start at login" option
- the text under the Save sync section is very confusing
- does the downloads section collapse or clear or anything? 
- might be worth using tabs to separate the pages a bit better
- no icon in taskbar
- i unmounted, then tried to re-mount but got an error 500 from romm. restarting the app fixed it. no logs because apprently llms hate logging errors.

## Project
- why does it take so long to start up? 32s 
- should allow selecting which emulation system to work with. currently only retrobat and default
- do we cache the catalog? how often do we update? how do we cache it?

### Default

- use RomM path mapping settings

### Retrobat

- use the retrobat folder layout
- can we place romm metadata into retrobat? mount gameslist.xml in each platform with data generated from our internal database, along with images
- can we auto-install retrobat for users?
- could we make it so when saves get saved in our virtual folder it gets auto-synced?
- can we include updates/dlc for switch
- can we include cheats and other filetypes?

- <base-path>
-- bios
-- cheats
-- emulationstation
-- roms
--- <platform>
-- saves
-- screenshots
-- sounds

## Production Run Logs
```
08:16:51 INFO  [connect] connected as aaron
08:17:22 WARN  [catalogue] rom 1640 ("Batman Returns.sfc"): 0 file entries — multi-file/folder games are unsupported, skipped
08:17:22 WARN  [catalogue] rom 2471 ("images"): 3 file entries — multi-file/folder games are unsupported, skipped
08:17:22 WARN  [catalogue] rom 418 ("media"): 1212 file entries — multi-file/folder games are unsupported, skipped
08:17:22 WARN  [catalogue] rom 2477 ("Disney Epic Mickey: Rebrushed.nsp"): visible name adjusted to "Disney Epic Mickey_ Rebrushed.nsp"
08:17:22 WARN  [catalogue] rom 2500 ("The Legend of Zelda: Echoes of Wisdom.xci"): visible name adjusted to "The Legend of Zelda_ Echoes of Wisdom.xci"
08:17:22 WARN  [catalogue] rom 530 ("media"): 330 file entries — multi-file/folder games are unsupported, skipped
08:17:22 WARN  [catalogue] rom 600 ("media"): 426 file entries — multi-file/folder games are unsupported, skipped
08:17:22 WARN  [catalogue] rom 824 ("media"): 802 file entries — multi-file/folder games are unsupported, skipped
08:17:22 WARN  [catalogue] rom 969 ("media"): 852 file entries — multi-file/folder games are unsupported, skipped
08:17:22 WARN  [catalogue] rom 1068 ("media"): 558 file entries — multi-file/folder games are unsupported, skipped
08:17:22 WARN  [catalogue] rom 1192 ("media"): 300 file entries — multi-file/folder games are unsupported, skipped
08:17:22 WARN  [catalogue] rom 1342 ("media"): 515 file entries — multi-file/folder games are unsupported, skipped
08:17:22 WARN  [catalogue] rom 1368 ("media"): 948 file entries — multi-file/folder games are unsupported, skipped
08:17:22 WARN  [catalogue] rom 1461 ("media"): 546 file entries — multi-file/folder games are unsupported, skipped
08:17:22 WARN  [catalogue] rom 1758 ("media"): 1132 file entries — multi-file/folder games are unsupported, skipped
08:17:22 WARN  [catalogue] rom 1988 ("media"): 1359 file entries — multi-file/folder games are unsupported, skipped
08:17:22 WARN  [catalogue] rom 2038 ("media"): 270 file entries — multi-file/folder games are unsupported, skipped
08:17:22 WARN  [catalogue] rom 2147 ("media"): 636 file entries — multi-file/folder games are unsupported, skipped
08:17:22 WARN  [catalogue] rom 2200 ("media"): 294 file entries — multi-file/folder games are unsupported, skipped
08:17:22 WARN  [catalogue] rom 2502 ("media"): 88 file entries — multi-file/folder games are unsupported, skipped
08:17:22 WARN  [catalogue] rom 2567 ("media"): 371 file entries — multi-file/folder games are unsupported, skipped
08:17:22 WARN  [catalogue] rom 2472 ("media"): 1728 file entries — multi-file/folder games are unsupported, skipped
08:17:22 WARN  [catalogue] rom 237 ("media"): 1360 file entries — multi-file/folder games are unsupported, skipped
08:17:22 WARN  [catalogue] rom 1621 ("Professor Layton and the Curious Village.nds"): 0 file entries — multi-file/folder games are unsupported, skipped
08:17:22 WARN  [catalogue] rom 2501 ("The Smurfs: Dreams.nsp"): visible name adjusted to "The Smurfs_ Dreams.nsp"
08:17:22 WARN  [catalogue] rom 1325 ("Streets of Rage 3.md"): 0 file entries — multi-file/folder games are unsupported, skipped
08:17:22 INFO  [catalogue] catalogue loaded: 25 platforms, 2448 roms, 23 skipped
08:17:22 INFO  [mount] checking and preparing mount directory "C:\\Users\\azza\\RomM"
08:17:22 INFO  [mount] loading catalogue for "C:\\Users\\azza\\RomM"
08:17:53 WARN  [catalogue] rom 1640 ("Batman Returns.sfc"): 0 file entries — multi-file/folder games are unsupported, skipped
08:17:53 WARN  [catalogue] rom 2471 ("images"): 3 file entries — multi-file/folder games are unsupported, skipped
08:17:53 WARN  [catalogue] rom 418 ("media"): 1212 file entries — multi-file/folder games are unsupported, skipped
08:17:53 WARN  [catalogue] rom 2477 ("Disney Epic Mickey: Rebrushed.nsp"): visible name adjusted to "Disney Epic Mickey_ Rebrushed.nsp"
08:17:53 WARN  [catalogue] rom 2500 ("The Legend of Zelda: Echoes of Wisdom.xci"): visible name adjusted to "The Legend of Zelda_ Echoes of Wisdom.xci"
08:17:53 WARN  [catalogue] rom 530 ("media"): 330 file entries — multi-file/folder games are unsupported, skipped
08:17:53 WARN  [catalogue] rom 600 ("media"): 426 file entries — multi-file/folder games are unsupported, skipped
08:17:53 WARN  [catalogue] rom 824 ("media"): 802 file entries — multi-file/folder games are unsupported, skipped
08:17:53 WARN  [catalogue] rom 969 ("media"): 852 file entries — multi-file/folder games are unsupported, skipped
08:17:53 WARN  [catalogue] rom 1068 ("media"): 558 file entries — multi-file/folder games are unsupported, skipped
08:17:53 WARN  [catalogue] rom 1192 ("media"): 300 file entries — multi-file/folder games are unsupported, skipped
08:17:53 WARN  [catalogue] rom 1342 ("media"): 515 file entries — multi-file/folder games are unsupported, skipped
08:17:53 WARN  [catalogue] rom 1368 ("media"): 948 file entries — multi-file/folder games are unsupported, skipped
08:17:53 WARN  [catalogue] rom 1461 ("media"): 546 file entries — multi-file/folder games are unsupported, skipped
08:17:53 WARN  [catalogue] rom 1758 ("media"): 1132 file entries — multi-file/folder games are unsupported, skipped
08:17:53 WARN  [catalogue] rom 1988 ("media"): 1359 file entries — multi-file/folder games are unsupported, skipped
08:17:53 WARN  [catalogue] rom 2038 ("media"): 270 file entries — multi-file/folder games are unsupported, skipped
08:17:53 WARN  [catalogue] rom 2147 ("media"): 636 file entries — multi-file/folder games are unsupported, skipped
08:17:53 WARN  [catalogue] rom 2200 ("media"): 294 file entries — multi-file/folder games are unsupported, skipped
08:17:53 WARN  [catalogue] rom 2502 ("media"): 88 file entries — multi-file/folder games are unsupported, skipped
08:17:53 WARN  [catalogue] rom 2567 ("media"): 371 file entries — multi-file/folder games are unsupported, skipped
08:17:53 WARN  [catalogue] rom 2472 ("media"): 1728 file entries — multi-file/folder games are unsupported, skipped
08:17:53 WARN  [catalogue] rom 237 ("media"): 1360 file entries — multi-file/folder games are unsupported, skipped
08:17:53 WARN  [catalogue] rom 1621 ("Professor Layton and the Curious Village.nds"): 0 file entries — multi-file/folder games are unsupported, skipped
08:17:53 WARN  [catalogue] rom 2501 ("The Smurfs: Dreams.nsp"): visible name adjusted to "The Smurfs_ Dreams.nsp"
08:17:53 WARN  [catalogue] rom 1325 ("Streets of Rage 3.md"): 0 file entries — multi-file/folder games are unsupported, skipped
08:17:53 INFO  [catalogue] catalogue loaded: 25 platforms, 2448 roms, 23 skipped
08:17:53 INFO  [mount] claiming ownership of "C:\\Users\\azza\\RomM"
08:17:53 INFO  [mount] opening private cache C:\Users\azza\AppData\Local\rommfs\cache\https___romm.jcaks.net
08:17:53 INFO  [mount] starting Windows Cloud Files mount at "C:\\Users\\azza\\RomM"
08:17:54 INFO  [mount] mounted at C:\Users\azza\RomM in 32.17s

08:50:20 ERROR [connect] invalid catalogue payload: empty server URL
08:50:27 INFO  [connect] connected as aaron
08:50:58 WARN  [catalogue] rom 1640 ("Batman Returns.sfc"): 0 file entries — multi-file/folder games are unsupported, skipped
08:50:58 WARN  [catalogue] rom 2471 ("images"): 3 file entries — multi-file/folder games are unsupported, skipped
08:50:58 WARN  [catalogue] rom 418 ("media"): 1212 file entries — multi-file/folder games are unsupported, skipped
08:50:58 WARN  [catalogue] rom 2477 ("Disney Epic Mickey: Rebrushed.nsp"): visible name adjusted to "Disney Epic Mickey_ Rebrushed.nsp"
08:50:58 WARN  [catalogue] rom 2500 ("The Legend of Zelda: Echoes of Wisdom.xci"): visible name adjusted to "The Legend of Zelda_ Echoes of Wisdom.xci"
08:50:58 WARN  [catalogue] rom 530 ("media"): 330 file entries — multi-file/folder games are unsupported, skipped
08:50:58 WARN  [catalogue] rom 600 ("media"): 426 file entries — multi-file/folder games are unsupported, skipped
08:50:58 WARN  [catalogue] rom 824 ("media"): 802 file entries — multi-file/folder games are unsupported, skipped
08:50:58 WARN  [catalogue] rom 969 ("media"): 852 file entries — multi-file/folder games are unsupported, skipped
08:50:58 WARN  [catalogue] rom 1068 ("media"): 558 file entries — multi-file/folder games are unsupported, skipped
08:50:58 WARN  [catalogue] rom 1192 ("media"): 300 file entries — multi-file/folder games are unsupported, skipped
08:50:58 WARN  [catalogue] rom 1342 ("media"): 515 file entries — multi-file/folder games are unsupported, skipped
08:50:58 WARN  [catalogue] rom 1368 ("media"): 948 file entries — multi-file/folder games are unsupported, skipped
08:50:58 WARN  [catalogue] rom 1461 ("media"): 546 file entries — multi-file/folder games are unsupported, skipped
08:50:58 WARN  [catalogue] rom 1758 ("media"): 1132 file entries — multi-file/folder games are unsupported, skipped
08:50:58 WARN  [catalogue] rom 1988 ("media"): 1359 file entries — multi-file/folder games are unsupported, skipped
08:50:58 WARN  [catalogue] rom 2038 ("media"): 270 file entries — multi-file/folder games are unsupported, skipped
08:50:58 WARN  [catalogue] rom 2147 ("media"): 636 file entries — multi-file/folder games are unsupported, skipped
08:50:58 WARN  [catalogue] rom 2200 ("media"): 294 file entries — multi-file/folder games are unsupported, skipped
08:50:58 WARN  [catalogue] rom 2502 ("media"): 88 file entries — multi-file/folder games are unsupported, skipped
08:50:58 WARN  [catalogue] rom 2567 ("media"): 371 file entries — multi-file/folder games are unsupported, skipped
08:50:58 WARN  [catalogue] rom 2472 ("media"): 1728 file entries — multi-file/folder games are unsupported, skipped
08:50:58 WARN  [catalogue] rom 237 ("media"): 1360 file entries — multi-file/folder games are unsupported, skipped
08:50:58 WARN  [catalogue] rom 1621 ("Professor Layton and the Curious Village.nds"): 0 file entries — multi-file/folder games are unsupported, skipped
08:50:58 WARN  [catalogue] rom 2501 ("The Smurfs: Dreams.nsp"): visible name adjusted to "The Smurfs_ Dreams.nsp"
08:50:58 WARN  [catalogue] rom 1325 ("Streets of Rage 3.md"): 0 file entries — multi-file/folder games are unsupported, skipped
08:50:58 INFO  [catalogue] catalogue loaded: 25 platforms, 2448 roms, 23 skipped
08:50:58 INFO  [mount] checking and preparing mount directory "C:\\Users\\azza\\RomM"
08:50:58 INFO  [mount] loading catalogue for "C:\\Users\\azza\\RomM"
08:51:30 WARN  [catalogue] rom 1640 ("Batman Returns.sfc"): 0 file entries — multi-file/folder games are unsupported, skipped
08:51:30 WARN  [catalogue] rom 2471 ("images"): 3 file entries — multi-file/folder games are unsupported, skipped
08:51:30 WARN  [catalogue] rom 418 ("media"): 1212 file entries — multi-file/folder games are unsupported, skipped
08:51:30 WARN  [catalogue] rom 2477 ("Disney Epic Mickey: Rebrushed.nsp"): visible name adjusted to "Disney Epic Mickey_ Rebrushed.nsp"
08:51:30 WARN  [catalogue] rom 2500 ("The Legend of Zelda: Echoes of Wisdom.xci"): visible name adjusted to "The Legend of Zelda_ Echoes of Wisdom.xci"
08:51:30 WARN  [catalogue] rom 530 ("media"): 330 file entries — multi-file/folder games are unsupported, skipped
08:51:30 WARN  [catalogue] rom 600 ("media"): 426 file entries — multi-file/folder games are unsupported, skipped
08:51:30 WARN  [catalogue] rom 824 ("media"): 802 file entries — multi-file/folder games are unsupported, skipped
08:51:30 WARN  [catalogue] rom 969 ("media"): 852 file entries — multi-file/folder games are unsupported, skipped
08:51:30 WARN  [catalogue] rom 1068 ("media"): 558 file entries — multi-file/folder games are unsupported, skipped
08:51:30 WARN  [catalogue] rom 1192 ("media"): 300 file entries — multi-file/folder games are unsupported, skipped
08:51:30 WARN  [catalogue] rom 1342 ("media"): 515 file entries — multi-file/folder games are unsupported, skipped
08:51:30 WARN  [catalogue] rom 1368 ("media"): 948 file entries — multi-file/folder games are unsupported, skipped
08:51:30 WARN  [catalogue] rom 1461 ("media"): 546 file entries — multi-file/folder games are unsupported, skipped
08:51:30 WARN  [catalogue] rom 1758 ("media"): 1132 file entries — multi-file/folder games are unsupported, skipped
08:51:30 WARN  [catalogue] rom 1988 ("media"): 1359 file entries — multi-file/folder games are unsupported, skipped
08:51:30 WARN  [catalogue] rom 2038 ("media"): 270 file entries — multi-file/folder games are unsupported, skipped
08:51:30 WARN  [catalogue] rom 2147 ("media"): 636 file entries — multi-file/folder games are unsupported, skipped
08:51:30 WARN  [catalogue] rom 2200 ("media"): 294 file entries — multi-file/folder games are unsupported, skipped
08:51:30 WARN  [catalogue] rom 2502 ("media"): 88 file entries — multi-file/folder games are unsupported, skipped
08:51:30 WARN  [catalogue] rom 2567 ("media"): 371 file entries — multi-file/folder games are unsupported, skipped
08:51:30 WARN  [catalogue] rom 2472 ("media"): 1728 file entries — multi-file/folder games are unsupported, skipped
08:51:30 WARN  [catalogue] rom 237 ("media"): 1360 file entries — multi-file/folder games are unsupported, skipped
08:51:30 WARN  [catalogue] rom 1621 ("Professor Layton and the Curious Village.nds"): 0 file entries — multi-file/folder games are unsupported, skipped
08:51:30 WARN  [catalogue] rom 2501 ("The Smurfs: Dreams.nsp"): visible name adjusted to "The Smurfs_ Dreams.nsp"
08:51:30 WARN  [catalogue] rom 1325 ("Streets of Rage 3.md"): 0 file entries — multi-file/folder games are unsupported, skipped
08:51:30 INFO  [catalogue] catalogue loaded: 25 platforms, 2448 roms, 23 skipped
08:51:30 INFO  [mount] claiming ownership of "C:\\Users\\azza\\RomM"
08:51:30 INFO  [mount] opening private cache C:\Users\azza\AppData\Local\rommfs\cache\https___romm.jcaks.net
08:51:30 INFO  [mount] starting Windows Cloud Files mount at "C:\\Users\\azza\\RomM"
08:51:32 INFO  [mount] mounted at C:\Users\azza\RomM in 33.61s
```


## Planning decisions (2026-10-07)

This section records the discussion and research before writing the implementation plan.
The original findings and production logs above remain the evidence to address.

### Agreed scope

- Support generic folders and RetroBat.
- Link RetroBat's ROM directory to the RomMFS-managed ROM folder.
- Require a fresh RetroBat installation without an exact-version lock. 8.2.1 is a
  research reference only; compatible upgrades must remain usable. Check the required
  layout/settings and report specific missing capabilities instead of rejecting a version.
- Reject ROM-directory files that are not owned by RomMFS, including stock files. Show
  the conflicting paths and leave them untouched; do not back up or migrate the ROM folder.
- Defer all actual Windows execution and all human testing. Complete implementation and
  applicable automated Linux checks without making deferred validation a prerequisite.
- Configure Eden/Citron to use RetroBat's saves folder when the setting can be changed
  safely. Make targeted, repeatable config edits with a backup while RetroBat is closed.
- Watch saves recursively and transfer files unchanged, preserving names, relative paths,
  companion files, and multiple files belonging to one game. Accept all save formats.
- Defer shared saves / shared memory cards. Retain reliable associations with RomM game IDs.
- Optional Windows startup: start in the tray, reconnect, mount, and resume previously
  enabled save sync.

### Confirmed implementation facts

- Connect loads the catalogue, then StartMount loads it again. The production log shows
  approximately 31 seconds for each load; native mount setup follows in about 1-2 seconds.
  The reason each catalogue fetch takes 31 seconds still needs request-level timings.
- Catalogue metadata is currently an in-memory snapshot. Downloaded ROM bytes persist in
  the private cache; the catalogue has no persistent cache or periodic refresh.
- A recursive notify watcher already exists in save_sync_agent/actor.rs. Extend it and
  its mappings instead of adding a second watcher. Current mapping/profile/transfer code
  imposes Game Boy, Gambatte, .srm, one-path-per-ROM, and 1 MiB restrictions.
- Completed download rows persist until displaced by the 128-row cap. There is no clear
  history or collapse control.
- The window stacks all sections vertically; only individual lists/logs scroll. This
  lets the log shrink almost out of view. Mounting has a status marker but no stage progress.
- The current client ignores RomM's descriptive metadata and artwork fields. The pinned
  RomM schema already exposes name, summary, genres, developers, publishers, release date,
  player count, rating, covers, and screenshots; these can feed local gamelist generation.
- The reported remount HTTP 500 has not been reproduced or explained. Keep it as a
  reliability investigation with stage/request diagnostics, rather than assume a cause.

### Proposed metadata automation

- Retain descriptive metadata in the catalogue cache and generate gamelist.xml per platform.
- Generate paths from the actual projected filenames, including Windows sanitization and
  the selected generic/RetroBat platform mapping.
- Download/cache available artwork and reference it with local paths. Fetching metadata
  and artwork must not hydrate ROM files. Missing artwork should not block mounting.
- Refresh generated metadata with catalogue refresh, retaining local favourites and play
  history. Publish complete files atomically and coordinate refresh with RetroBat's writes.
- Account for writable gamelists and frontend state alongside read-only ROM placeholders.
  The current mount-wide read-only policy and cleanup rules need adaptation; simply adding
  XML to the existing ROM-only tree does not complete this feature.
- RomM also supports server-side gamelist export. Compare its verified API with local
  generation; local generation can directly use the actual projected Windows paths.

### Remaining notes to cover in the plan

| Area | Direction |
| --- | --- |
| Mounting and errors | Stage progress, catalogue page counts/timings, useful HTTP errors, durable diagnostic logs, and repeated stop/start recovery. |
| Catalogue and startup | Eliminate the duplicate load, persist catalogue/metadata, define refresh and failure behavior, and expose last refresh status. |
| Authentication | RomM browser pairing, securely remembered session, and clear expiry/reconnect handling. |
| Desktop UI | Separate mount/library, save sync, activity, and settings; usable scrolling, clearer save copy, visible loading states, download history controls, taskbar/tray icon. |
| Generic layout | Honour RomM's platform/path mapping information; verify what the supported API exposes. |
| RetroBat setup | Fresh installation, ROM directory link, required save settings, repeatable setup, and layout/config compatibility checks; no exact-version gate or ROM-folder backups. |
| Installation assistance | Keep the requested automated RetroBat install/setup in the plan; verify official installer options and package identity. |
| Updates/DLC and other files | Keep requested Switch updates/DLC, cheats, and other file types in scope for investigation; verify RomM file categories/endpoints and RetroBat destinations. Do not silently treat them as ordinary launchable games. |
| Skipped entries | Distinguish missing file records from multi-file games and media directories; avoid misleading warnings and duplicate log noise. |

### Source checks still needed before implementation

- Current RetroBat launcher source uses yuzu_mutualize to set Eden's save_directory to
  <saves>/switch. The launcher loads es_settings.cfg with global/system/game overrides.
  Verify serialized key/value and override behavior from source/config samples. Actual
  Eden/Citron execution is deferred, along with all other Windows/human checks.
- Inspect frontend gamelist behavior and Windows ACL contracts from source. Implement the
  writable metadata arrangement on Linux; actual Windows validation is deferred.
- Confirm remote representation for multiple files per game and preserve relative paths;
  shared-card mapping is explicitly deferred.

### Research references

- [RetroBat 8.2.1 release](https://github.com/RetroBat-Official/retrobat/releases/tag/8.2.1)
- [RetroBat Switch save-location option](https://wiki.retrobat.org/systems-and-emulators/supported-game-systems/game-consoles/nintendo-game-consoles/switch)
- [Eden launcher source, current master](https://github.com/RetroBat-Official/emulatorlauncher/blob/master/emulatorLauncher/Generators/Eden.Generator.cs)
- [Launcher configuration loading, current master](https://github.com/RetroBat-Official/emulatorlauncher/blob/master/emulatorLauncher/Program.cs)
- [RomM response schema at the repository's verified API revision](https://github.com/rommapp/romm/blob/95599dadbe93c8f7b8a8647148fafbe293df3167/backend/endpoints/responses/rom.py)
- [RomM gamelist export documentation](https://docs.romm.app/5.3.0-alpha/reference/exports/)
- [RetroBat gamelist read/write implementation, current master](https://github.com/RetroBat-Official/emulationstation/blob/master/es-app/src/Gamelist.cpp)

Research used official docs and source; it did not run a Windows/RetroBat session or
change application code. Source inspection does not establish Windows runtime behavior.

### Follow-up research and plan corrections

The current [PLAN.md](PLAN.md) incorporates the no-version-lock policy, rejection of
unowned ROM-directory files, and deferred Windows/human validation. Its Eden content
section records source-backed destinations: external updates/DLC under
`roms/switchupdates`, keys under `emulators/eden/user/keys`, and unpacked firmware under
`emulators/eden/user/nand/system/Contents/registered`. RetroBat configures external
content loading; firmware/key placement is a separate operation. See the plan for
references and the remaining API representation questions.
