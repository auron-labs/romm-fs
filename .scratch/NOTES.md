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
