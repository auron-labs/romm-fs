# RomMFS

Windows-first proof of concept that mounts a [RomM](https://romm.app)
library as ordinary filesystem paths (e.g. `C:\RomM\nes\Example Game.nes`)
with **Generic folders** and **RetroBat** layouts — while ROM
bytes only download the first time something actually reads them.

Built as a Rust workspace; uses Windows' built-in **Cloud Files API (CFAPI)**
through a custom `windows-sys` adapter and shows one small **GPUI** window.
No separately installed filesystem driver or ProjFS optional feature is required.
This checkout is under development. The [dated validation record](validation/2026-10-12.md) includes portable results and a Windows Server 2022 diagnostic run; Windows 10/11 client and several manual acceptance cases remain open. Portable tests and cross-target checking are not native runtime evidence. The implementation includes remembered sessions, verified installer downloads, ordinary-file saves, supported auxiliary materialization, and Windows tray/login startup.

## Workspace

| crate | role |
|---|---|
| `rommfs-core` | RomM client, portable catalogue/cache/save logic, RetroBat setup/metadata, and a Windows-only Credential Manager adapter. Portable tests run headless. |
| `rommfs-fixture` | In-process HTTP fixture server implementing the verified RomM 5.3.1 contract for tests (route hit counters prove no-download-on-listing, single-flight downloads). |
| `rommfs-cfapi` | CFAPI adapter: NTFS placeholders, lazy hydration, read-only ACLs, safe dehydration, and ownership validation. |
| `rommfs-app` | The GPUI window + headless controller (connect → catalogue → mount, live download progress, bounded log). |

## Requirements

- Windows 10 version 1709+ or Windows 11 with the built-in Cloud Files platform.
  The mount root must be on a **local NTFS volume**, outside another cloud provider's
  sync root. Network shares and FAT/exFAT roots are unsupported. RomMFS checks the
  platform and volume; it does not install drivers or elevate.
- An MSVC Rust toolchain and Visual Studio C++ build tools, including the
  Windows SDK. The CFAPI adapter uses generated `windows-sys` bindings.
- Rust 1.89+ (the locked GPUI dependency graph includes `uuid 1.27`, which
  requires Rust 1.89)
- A reachable RomM server. Tested against **RomM 5.3.1** — see
  `docs/contracts/romm-api.md` for the verified API surface (token grant,
   `/api/platforms`, `/api/roms` pagination and individual original-file content).
   Pairing and metadata extensions are source-backed and fixture-tested, not
   live-validated against that original instance.

## Run

```bash
cargo run -p rommfs-app
```

Enter the server URL and choose **Sign in with browser**. Approve the pairing
in RomM. Windows remembers the paired secret in Credential Manager; password
sign-in remains available but is not remembered. Choose a mount root
(an empty directory, or one RomMFS previously claimed), press **Start games folder**.
The ownership marker is stored beside the mount directory as
`<folder>.rommfs-root`; keep it for subsequent mounts.
Read-only ROM files appear per platform; reading a file downloads it once
into a private cache (`%LOCALAPPDATA%\rommfs\cache\<server>`); later reads
are local. Entries unused for 14 days are evicted from the private cache;
native sharing/dehydration failures defer eviction while files are in use.
While mounted, NTFS also stores hydrated ROM bytes; eviction must dehydrate
that copy before removing the private cache. Allow disk space for both copies.

On Windows, closing the window hides it while mounting and save sync continue.
The tray offers status, Open and Quit; **Quit** drains workers and stops the mount.
If the tray cannot be created, closing the visible window quits instead.
**Settings → Start at login** requires remembered browser sign-in and
launches in the tray, reconnects, mounts, then resumes the same enabled save scope.
Passwords and tokens are never logged. Logout removes remembered sign-in and
the login-startup entry, and cancels its active work.

Authentication precedes cached-catalogue restoration. A warm connection uses the
last complete account-scoped snapshot and schedules one background refresh.
**Refresh library** also requests fresh metadata. Starting/remounting reuses the
loaded catalogue rather than fetching it twice. Structural changes take effect
on the next mount; a failed refresh retains the previous complete snapshot.

The **Activity** view shows operations and diagnostics. **Activity → Open log
folder** opens `%LOCALAPPDATA%\rommfs\logs`. Logs rotate at 2 MiB with three
archives; relevant paths, IDs and error stages are retained, not sign-in secrets.

## Test

Follow the [Windows testing guide](testing/windows.md) for local server
setup, automated tests, native CFAPI, the merged UI regression cases, live RomM
reads, RetroBat, save sync, tray/startup, cleanup, and evidence reporting.

For a disposable demo server, run these from the repository root in PowerShell
with Python 3 and Docker Desktop (Linux containers):

```powershell
py -3 scripts/create-demo-roms.py
docker compose -f compose.dev.yaml up -d
```

Open `http://127.0.0.1:8081`, create a test account, and scan the library in RomM.
The generator creates 18 zero-filled files under `romm/roms/<platform>/<game>.<ext>`
with representative sizes (about 130 MiB total). These files cannot run in an
emulator. Existing matching files are verified; other existing data is preserved.
Use `python3` instead of `py -3` on Linux/macOS. RomM 5.3.1 does not support
SQLite, so [compose.dev.yaml](../compose.dev.yaml) uses MariaDB. See the guide for
size/count options, persistence, alternate ports, and reset instructions.
The strict live test below uses its own committed library on port 8080.

Portable core, fixture, and headless app-controller tests run on Linux,
macOS, and Windows:

```bash
cargo test -p rommfs-core -p rommfs-fixture -p rommfs-cfapi -p rommfs-app --no-default-features -- --test-threads=1
```

Serial execution reduces simultaneous native watchers. A shared Linux host near
its inotify-instance limit can still reject watcher creation; see
[validation notes](validation/2026-10-12.md#validation-boundary).

The desktop app and native CFAPI mount require Windows. On Windows, run the workspace
tests (including the native CFAPI mount test) with:

```powershell
cargo test --workspace
# Run only the native mount test:
cargo test -p rommfs-cfapi --test cfapi_native
```

The native test (`cfapi_native.rs`) mounts a real `RommFs` on a temp root
and checks that enumeration never downloads, first read downloads once
byte-exact, warm reads are cached, ROM mutations fail,
open handles prevent eviction, eviction triggers a fresh download, and remount
reuses the private cache. The dated [validation record](validation/2026-10-12.md) reports the Windows Server 2022 diagnostic run and remaining Windows 10/11 acceptance.

### Live-RomM harness

`testing/romm-harness/` runs a real RomM in Docker with three deterministic
placeholder ROMs (`nes`, `snes`, `gb` — committed, sha256-verifiable):

```bash
cd testing/romm-harness && docker compose up -d   # see its README
cargo test -p rommfs-cfapi --test live_romm -- --ignored
```

The E2E connects the live server through real CFAPI and byte-compares every
projected ROM against the committed sources.

## Generic folders

Point the frontend's ROM directory at the mount root, e.g.
`C:\RomM` — platform folders appear as `C:\RomM\nes`, `C:\RomM\snes`, …
ES-DE's scraped-folder config maps each to its system (`nes`, `snes`,
`gb`, …) the same way it would a local ROM library. Scraping/listing is
instant (metadata only); the first launch of a game downloads its file once.

## Set up RetroBat

> RomM games appear alongside your own ROMs. RomMFS never moves, replaces or
> deletes anything already in RetroBat's `roms` folder, including the games
> and gamelists a fresh RetroBat install ships.

In **Settings → RetroBat**, use **Download official installer** and then
**Launch installer…**, or select an installation you installed separately. Start RetroBat once and
close it before setup so it creates its system configuration.
RomMFS discovers the latest stable official Windows package, streams its bytes,
checks the release size and SHA-256 digest, and rechecks integrity before launch.
Cancel a download in RomMFS; cancel installation inside the official wizard.
RomMFS does not guess unattended switches, terminate a writing installer, or
remove partial installation files. Stop RetroBat and its emulators before setup.
Compatible versions are accepted by required layout,
not version equality; 8.2.1 is the source/package research reference.

In **Settings**, select RetroBat, choose its installation and a separate managed
ROM folder, then choose **Set up RetroBat**. For each system RetroBat defines,
setup adds one folder link, `roms/<system>/RomM` → `<managed folder>/<system>`.
`roms/<system>` stays a real folder holding your own files; setup creates it
only when RetroBat has none. A system that can't be linked (its folder is a link
or file of your own, or it already has something named `RomM`) is skipped with
the reason, and the other systems are still linked. Switch save settings are
configured where possible; a problem there is reported and never blocks linking.

Windows needs permission to create folder links. Setup checks this before
changing anything and asks through UAC when needed; declining the prompt
leaves RetroBat exactly as it was.

Setup records what it did in `<RetroBat>/.rommfs-setup.json`. Rerunning adds
links for newly available systems and restores removed ones. Changing the games
folder or server and rerunning moves RomMFS's own links to the new target.
Installs from earlier versions are converted: a `roms` or `roms/<system>` link
RomMFS created becomes a real folder again, and any folder an earlier version
moved to `.rommfs-original-roms` is moved back. Targeted frontend settings edits
get an `es_settings.cfg.rommfs-<id>.bak` backup. The system-level Switch save
option is enabled; RetroBat's Eden/Citron launchers then generate their distinct
save-location keys.

Mounting merges RomM games into each linked system's own
`roms/<system>/gamelist.xml`, under `./RomM/…` paths. Entries and folders RomMFS
doesn't own are kept as they are. Systems whose `RomM` link was removed or
redirected are excluded from publication. Favourites, play counts, last-played times and
other frontend fields on RomM entries are carried over when RomMFS republishes:
by stable ROM/file identity, or by path when that identity record is missing.
Gamelists are written before artwork finishes downloading, keeping the artwork
entries already show, and are updated again once artwork is cached (eight
concurrent downloads). A gamelist that can't be read is left untouched and
reported; other systems are still updated. Each changed gamelist is replaced
atomically while RetroBat is stopped; publication waits while it is running.
If a gamelist can't be written, other systems are still attempted and the error
is reported. After fixing folder access, refresh or remount to retry; saved
identity state protects favourites and history even if filenames changed.
Artwork and the identity index live in the managed folder. Switch update/DLC
`.nsp` files go in flat `switchupdates` and are not listed as launchable games.
Actual RetroBat loading and Windows ACL protection remain awaiting native
validation.

**Install library BIOS, keys and cheats** is an explicit separate action while
the mount and emulators are stopped. It uses RomM's `firmware.read` inventory for
ordinary BIOS files, Switch `prod.keys`/`title.keys`, and unpacked `.nca` firmware.
Switch content goes into installed Eden/Citron portable user folders; ordinary
BIOS goes into the effective RetroBat BIOS folder. ROM-file category `cheat`
supports RetroArch `.cht` files in the effective `cheats/retroarch` directory.
Archives are not extracted. Unsupported emulator cheat formats are reported.
Use **Cancel auxiliary download**, Logout or Quit to cancel and drain the job;
already completed owned files remain, and partial files are not published.
An ownership/version index under `.rommfs-auxiliary` makes repeats safe. Unowned,
emulator-modified, or changed-version files are preserved and reported; installing
a changed server version requires explicitly removing the unchanged owned file
and rerunning the action. These directories are excluded from save sync.

## Optional save sync

Save sync is **off by default**. Choose a save-data folder or a RetroBat installation,
check the displayed server, authenticated account, effective saves folder,
existing-file supported/skipped counts, mapped game target count, and target
path preview, then explicitly enable it.
The debounce defaults to 5 seconds; only whole seconds from 1 through 3600 are
accepted, and invalid values are not saved. Consent is scoped to that server,
account, installation, and effective saves folder. Paired sign-in is remembered
separately and never enables a new sync scope by itself.

On Windows, discovery checks fixed/removable `X:\RetroBat` roots, then image
paths of running RetroBat/EmulationStation processes. Candidates are validated
read-only, deduplicated, and listed by path. You choose one explicitly; a
previously selected path is shown first even if discovery does not return it,
and **Browse…** can select an installation discovery missed. A missing prior
selection stays selected and paused rather than silently switching to another.
Sync runs only while this app is open, connected, authenticated, and opted in.
It runs in the app session, including while the window is hidden. The explicit RetroBat setup action described
above is separate from enabling saves. The account needs RomM `platforms.read` and
`roms.read` for existing ROM features, plus `me.read`, `assets.read`, and
`assets.write` for network save sync; exporting a save already staged locally
does not make save API requests or require those save scopes.

All ordinary file formats are accepted, including companions, extensionless,
empty and large files, and multiple/nested files per game. Bytes are streamed
without emulator-format conversion or a 1 MiB limit. Select **save data only**,
not an emulator's entire portable folder. Shared saves and shared memory cards
remain unsupported; known shared-card folders are conservatively excluded.
Automatic association uses a unique game filename or a
server-provided save target; use **Map an unmatched file** for ambiguous paths.
Before consent, scanning inspects filenames only, without reading save contents
or calling save APIs. It refuses redirected/unsafe paths and stops with an
explicit error above 100,000 directory entries. File paths must fit RomM's
255-character slot contract; overlong paths are reported, never truncated.
Existing SRAM-only consent cannot enable the broader scope. Matching legacy
journal records retain baselines, removals, incoming files and queued snapshots.

Uploads use immutable local snapshots, per-generation UUID filenames, and
RomM's `overwrite=false&autocleanup=false` request options. These prevent
client-requested replacement/automatic cleanup, but are not a promise that a
server retains every remote version: server-side retention can prune history
(RomM 5.3.1, for example, can enforce a 50-save per-ROM/emulator/user limit).
Incoming content creates a never-existing target or replaces an unchanged,
previously synchronized baseline. Windows file sharing defers active saves;
the prior baseline is retained in a `.rommfs-backup-<id>` sibling and its location
is recorded before replacement. Simultaneous changes preserve both copies for
review. A previously
tracked local delete is remembered; sync neither deletes the remote copy nor
restores it automatically.
Use **Export…** to write a staged incoming copy to a separate `.rommfs-incoming`
file; export cannot replace a mapped save or resolve/clear the review item.
Review/export remain available while paused or disabled, and export is local-only.

Pending snapshots, incoming review items, and consent/settings survive app
restarts in the private `%LOCALAPPDATA%\rommfs\settings\` save-sync database and
snapshot directory. If sync is offline or the app closes, uploads wait for a
later enabled session; no background service continues work. ROM cache data is
stored separately.

### Verification evidence and remaining smoke checks

Source refs reviewed (not evidence that a bundled binary is byte-identical):
RetroBat 8.2.1 `9761d47af5902410f020164163a7b27a6facc38b`, RetroArch v1.21.0
`baee906ef35b99283f9a1a060a2ce5ac86159b63`, Gambatte upstream
`d9d6cd06382d1ced30de34d56d3609452323dab1`, and RomM 5.2.0
`42e8043372522089a3bd724e8f4a0635aa1c6bf9`. The workspace HTTP fixture
models the RomM 5.3.1 source ref `95599dadbe93c8f7b8a8647148fafbe293df3167`;
fixture contract coverage is not a live-server test. The RomM 5.2.0 OpenAPI was
reviewed read-only; no live authentication, inventory, or upload requests were sent.

The following Windows/live checks are **not run** on the Linux development host.
In PowerShell on Windows, run the native filesystem and watcher tests:

```powershell
cargo test --locked -p rommfs-core --lib save_sync::path::tests::windows_save_read_handle_denies_concurrent_write_and_delete -- --exact --test-threads=1
cargo test --locked -p rommfs-core --lib save_sync::path::tests::windows_directory_guard_blocks_rename_until_released -- --exact --test-threads=1
cargo test --locked -p rommfs-app --no-default-features --lib save_sync_agent::files::tests::windows_publication_does_not_replace_a_file_created_at_the_boundary -- --exact --test-threads=1
cargo test --locked -p rommfs-app --no-default-features --lib save_sync_agent::tests::lifecycle::real_filesystem_notification_drives_the_agent_without_waiting_for_fallback_scan -- --exact --test-threads=1
```

For a live two-install smoke test, back up both saves for a disposable Game Boy
title, use two separate RetroBat installs and one test RomM account with the
required scopes, and keep both installs on the same Game Boy catalogue entry.
Refresh/discover each install and verify its path/account/profile and preview;
enable sync on install A and wait for its UUID save to appear in RomM. Give
install B a different test save before enabling it, confirm review preserves
that local file, and use the native picker to export the incoming copy to a
separate `.rommfs-incoming` file. Disable sync, restart the app, and confirm the
pending review remains exportable without save API requests. Restore the backed-up
test saves. This
manual exercise is also **not run** until performed against a Windows host and
live RomM server.

## Observed limitations (PoC scope)

- ROM files are projected individually, including valid files from multi-file games. Files with unsupported categories, missing records, or unsafe paths are skipped and reported.
- Each mounted tree uses a catalogue snapshot. Explicit/background refresh can update the stored catalogue; a running mount keeps its current tree, and remount reuses the loaded catalogue when available.
- The mounted volume is read-only, including new files and directories.
  Store saves outside the ROM mount.
- CFAPI uses real NTFS placeholders. Clean Stop removes only verified, unmodified
  app-owned placeholders and restores the original root ACL. Busy or changed files
  defer cleanup and remain for restart; user files are never recursively cleared.
  Keep both sibling `.rommfs-root` and `.rommfs-lock` files while using that root.
- Migration from the old backend requires a fresh empty mount folder. Old
  roots may contain hydrated ROMs, modified files, or local saves; RomMFS
  refuses to mount over them and never clears them automatically.
- Server identity includes the configured URL's base path. When upgrading
  a mount configured with a base path, choose a new empty mount folder;
  older root markers used only the host and cannot safely identify that
  server. The old folder's files remain available.
- Auth tokens live in memory only. Save-sync consent, selection, journal, and
  staged snapshots persist separately under the user's private settings path;
  ROM bytes remain in the separate content cache.
- Windows-only mount APIs and dependencies; root validation, core, fixture,
  and headless app tests remain portable.

## Development notes

- Custom CFAPI adapter via `windows-sys`; implementation and outstanding native
  checks are recorded in [validation record](validation/2026-10-12.md); the original
  decision is preserved in [the archive](../.planning/archived/BACKEND-DECISION.md).
- The [approved scope](../.planning/archived/PLAN.md) and original
  [source spec](../.planning/archived/PRD.md) are archived; current behavior and
  remaining validation are recorded in the dated validation record above.
- Format all crates with `cargo fmt --all`. On Windows, lint the full workspace
  with `cargo clippy --workspace --all-targets`; on other platforms, lint the
  portable crates and headless app with
  `cargo clippy -p rommfs-core -p rommfs-fixture -p rommfs-cfapi -p rommfs-app --no-default-features --all-targets`.
