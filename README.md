# RomMFS

Windows-first proof of concept that mounts a [RomM](https://romm.app)
library as ordinary filesystem paths (e.g. `C:\RomM\nes\Example Game.nes`)
so frontends like ES-DE discover ROMs like a normal ROM folder — while ROM
bytes only download the first time something actually reads them.

Built as a Rust workspace; uses Windows’ built-in **Cloud Files API (CFAPI)**
through a custom `windows-sys` adapter and shows one small **GPUI** window.
No separately installed filesystem driver or ProjFS optional feature is required.
The native Windows build and runtime workflow are **not yet verified**.

## Workspace

| crate | role |
|---|---|
| `rommfs-core` | RomM HTTP client, catalogue→inode tree, sanitization, SQLite cache index, atomic download manager, eviction policy, `RommFs` facade. No Windows APIs — all unit tests run headless. |
| `rommfs-fixture` | In-process HTTP fixture server implementing the verified RomM 5.3.1 contract for tests (route hit counters prove no-download-on-listing, single-flight downloads). |
| `rommfs-cfapi` | CFAPI adapter: NTFS placeholders, lazy hydration, read-only ACLs, safe dehydration, and ownership validation. |
| `rommfs-app` | The GPUI window + headless controller (connect → catalogue → mount, live download progress, bounded log). |

## Requirements

- Windows 10 version 1709+ or Windows 11 with the built-in Cloud Files platform.
  The mount root must be on a **local NTFS volume**, outside another cloud provider’s
  sync root. Network shares and FAT/exFAT roots are unsupported. RomMFS checks the
  platform and volume; it does not install drivers or elevate.
- An MSVC Rust toolchain and Visual Studio C++ build tools, including the
  Windows SDK. The CFAPI adapter uses generated `windows-sys` bindings.
- Rust 1.89+ (the locked GPUI dependency graph includes `uuid 1.27`, which
  requires Rust 1.89)
- A reachable RomM server. Tested against **RomM 5.3.1** — see
  `.planning/API-CONTRACT.md` for the verified API surface (token grant,
  `/api/platforms`, `/api/roms` pagination, `/api/roms/{id}/content/{name}`).

## Run

```bash
cargo run -p rommfs-app
```

Enter the server URL + credentials, press **Connect**, choose a mount root
(an empty directory, or one RomMFS previously claimed), press **Start**.
The ownership marker is stored beside the mount directory as
`<folder>.rommfs-root`; keep it for subsequent mounts.
Read-only ROM files appear per platform; reading a file downloads it once
into a private cache (`%LOCALAPPDATA%\rommfs\cache\<server>`); later reads
are local. Entries unused for 14 days are evicted from the private cache;
native sharing/dehydration failures defer eviction while files are in use.
While mounted, NTFS also stores hydrated ROM bytes; eviction must dehydrate
that copy before removing the private cache. Allow disk space for both copies.

Closing the window stops the mount — there is no tray/background process,
and no credentials are ever persisted or logged.

## Test

For an LLM testing on a Windows host, follow the
[Windows CFAPI and RetroBat testing guide](testing/WINDOWS-TESTING.md).
It covers automated tests, desktop mounts, live RomM reads, RetroBat save sync,
cleanup, and evidence reporting.

Portable core, fixture, and headless app-controller tests run on Linux,
macOS, and Windows:

```bash
cargo test -p rommfs-core -p rommfs-fixture -p rommfs-cfapi -p rommfs-app --no-default-features
```

The desktop app and native CFAPI mount require Windows. On Windows, run the workspace
tests (including the native CFAPI mount test) with:

```powershell
cargo test --workspace
# Run only the native mount test:
cargo test -p rommfs-cfapi --test cfapi_native
```

The native test (`cfapi_native.rs`) mounts a real `RommFs` on a temp root
and checks that enumeration never downloads, first read downloads once
byte-exact, warm reads are cached, mutations and new file creation fail,
open handles prevent eviction, eviction triggers a fresh download, and remount
reuses the private cache. Native Windows build and runtime tests have not run
on this Linux development host and are required to validate the backend.

### Live-RomM harness

`testing/romm-harness/` runs a real RomM in Docker with three deterministic
placeholder ROMs (`nes`, `snes`, `gb` — committed, sha256-verifiable):

```bash
cd testing/romm-harness && docker compose up -d   # see its README
cargo test -p rommfs-cfapi --test live_romm -- --ignored
```

The E2E connects the live server through real CFAPI and byte-compares every
projected ROM against the committed sources.

## Frontend setup (example: ES-DE)

Point the frontend's ROM directory at the mount root, e.g.
`C:\RomM` — platform folders appear as `C:\RomM\nes`, `C:\RomM\snes`, …
ES-DE's scraped-folder config maps each to its system (`nes`, `snes`,
`gb`, …) the same way it would a local ROM library. Scraping/listing is
instant (metadata only); the first launch of a game downloads its file once.

## Optional save sync

Save sync is **off by default**. Select a discovered RetroBat installation,
check the displayed server, authenticated account, effective saves folder,
existing-file supported/skipped counts, mapped game target count, and target
path preview, then explicitly enable it.
The debounce defaults to 5 seconds; only whole seconds from 1 through 3600 are
accepted, and invalid values are not saved. Consent is scoped to that server,
account, installation, and effective saves folder; credentials are not persisted.

On Windows, discovery checks fixed/removable `X:\RetroBat` roots, then image
paths of running RetroBat/EmulationStation processes. Candidates are validated
read-only, deduplicated, and listed by path. You choose one explicitly; a
previously selected path is shown first even if discovery does not return it,
and **Browse…** can select an installation discovery missed. A missing prior
selection stays selected and paused rather than silently switching to another.
Sync runs only while this app is open, connected, authenticated, and opted in.
It is not a daemon/autostart task and does not edit RetroBat, RetroArch, or
emulator configuration files. The account needs RomM `platforms.read` and
`roms.read` for existing ROM features, plus `me.read`, `assets.read`, and
`assets.write` for network save sync; exporting a save already staged locally
does not make save API requests or require those save scopes.

The only supported profile is source-verified RetroBat 8.2.1 using
RetroArch/Gambatte for Game Boy. It maps visible catalogue `.gb` ROMs to
`gb/<visible ROM stem>.srm`; `.gbc`, Gambatte `.rtc` sidecars, ambiguous names,
save states, memory cards, multi-file ROMs, compressed/converted saves, and
other profiles are excluded. RetroArch settings and launcher overrides are
read to resolve the effective saves folder before sync can be enabled.
Before consent, the preview separately reports mapped game targets and counts
existing save filenames using filesystem metadata only. The walk is bounded to
4096 entries and eight directory levels; it never opens or hashes save contents,
creates a missing saves folder, or makes save API requests. Partial or unavailable
scans are labelled as such rather than shown as a complete zero-file result.

Uploads use immutable local snapshots, per-generation UUID filenames, and
RomM's `overwrite=false&autocleanup=false` request options. These prevent
client-requested replacement/automatic cleanup, but are not a promise that a
server retains every remote version: server-side retention can prune history
(RomM 5.3.1, for example, can enforce a 50-save per-ROM/emulator/user limit).
Incoming content is installed only if that local target has never existed.
Existing local files are kept for review alongside incoming content. A previously
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
cargo test -p rommfs-core --lib save_sync::path::tests::windows_save_read_handle_denies_concurrent_write_and_delete -- --exact
cargo test -p rommfs-core --lib save_sync::path::tests::windows_directory_guard_blocks_rename_until_released -- --exact
cargo test -p rommfs-app --no-default-features --lib save_sync_agent::files::tests::windows_publication_does_not_replace_a_file_created_at_the_boundary -- --exact
cargo test -p rommfs-app --no-default-features --lib save_sync_agent::tests::real_filesystem_notification_drives_the_agent_without_waiting_for_fallback_scan -- --exact
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

- Single-file ROMs only: multi-file games are skipped and counted in the
  log/status line.
- Catalogue is a mount-time snapshot — no live sync; Stop+Start reloads.
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
  checks are recorded in `.planning/BACKEND-DECISION.md`.
- `.planning/PRD.md` is the source spec this implements (R1–R5).
- Format all crates with `cargo fmt --all`. On Windows, lint the full workspace
  with `cargo clippy --workspace --all-targets`; on other platforms, lint the
  portable crates and headless app with
  `cargo clippy -p rommfs-core -p rommfs-fixture -p rommfs-cfapi -p rommfs-app --no-default-features --all-targets`.
