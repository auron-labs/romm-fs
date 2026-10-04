# RomMFS

Windows-first proof of concept that mounts a [RomM](https://romm.app)
library as ordinary filesystem paths (e.g. `C:\RomM\nes\Example Game.nes`)
so frontends like ES-DE discover ROMs like a normal ROM folder — while ROM
bytes only download the first time something actually reads them.

Built as a Rust workspace; mounts through Windows **ProjFS** via the
[`fsk`](https://crates.io/crates/fsk) crate and shows one small **GPUI**
window.

## Workspace

| crate | role |
|---|---|
| `rommfs-core` | RomM HTTP client, catalogue→inode tree, sanitization, SQLite cache index, atomic download manager, eviction policy, `RommFs` facade. No Windows APIs — all unit tests run headless. |
| `rommfs-fixture` | In-process HTTP fixture server implementing the verified RomM 5.3.1 contract for tests (route hit counters prove no-download-on-listing, single-flight downloads). |
| `rommfs-fsk` | Windows-only ProjFS adapter (`#[cfg(windows)]`): read-only veto of mutation notifications, `PrjDeleteFile`-backed hydrated eviction, mount-root safety. |
| `rommfs-app` | The GPUI window + headless controller (connect → catalogue → mount, live download progress, bounded log). |

## Requirements

- Windows 10 20H1+ (tested on Windows Server 2022) with the **Client-ProjFS**
  optional feature enabled:
  ```powershell
  Enable-WindowsOptionalFeature -Online -FeatureName Client-ProjFS
  ```
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
Read-only ROM files appear per platform; reading a file downloads it once
into a private cache (`%LOCALAPPDATA%\rommfs\cache\<server>`); later reads
are local. Entries unused for 14 days are evicted (bin + ProjFS-hydrated
copy); a ROM with an open handle is never evicted.

Closing the window stops the mount — there is no tray/background process,
and no credentials are ever persisted or logged.

## Test

Portable core, fixture, and headless app-controller tests run on Linux,
macOS, and Windows:

```bash
cargo test -p rommfs-core -p rommfs-fixture -p rommfs-app --no-default-features
```

The app and ProjFS adapter require Windows. On Windows, run the workspace
tests (including the native ProjFS mount test) with:

```powershell
cargo test --workspace
```

The native test (`projfs_native.rs`) mounts a real `RommFs` on a temp root
and proves enumeration never downloads, first read downloads once
byte-exact, warm reads are cached, and del/ren/write attempts are vetoed.

### Live-RomM harness

`testing/romm-harness/` runs a real RomM in Docker with three deterministic
placeholder ROMs (`nes`, `snes`, `gb` — committed, sha256-verifiable):

```bash
cd testing/romm-harness && docker compose up -d   # see its README
cargo test -p rommfs-fsk --test live_romm -- --ignored
```

The E2E mounts the live server through real ProjFS and byte-compares every
projected ROM against the committed sources.

## Frontend setup (example: ES-DE)

Point the frontend's ROM directory at the mount root, e.g.
`C:\RomM` — platform folders appear as `C:\RomM\nes`, `C:\RomM\snes`, …
ES-DE's scraped-folder config maps each to its system (`nes`, `snes`,
`gb`, …) the same way it would a local ROM library. Scraping/listing is
instant (metadata only); the first launch of a game downloads its file once.

## Observed limitations (PoC scope)

- Single-file ROMs only: multi-file games are skipped and counted in the
  log/status line.
- Catalogue is a mount-time snapshot — no live sync; Stop+Start reloads.
- Creating brand-new files inside the root cannot be vetoed (ProjFS
  `PRJ_NOTIFY_NEW_FILE_CREATED` is post-only); the tree stays read-only
  for projected entries — deletes/renames/writes on them are rejected.
- After unmount, ProjFS leaves its virtualization-root reparse tag and
  hydrated files behind. The next mount clears the owned root's tag and
  clean ProjFS placeholders, preserving local and modified files. ROMs are
  re-projected lazily from the private cache without re-downloading.
- Server identity includes the configured URL's base path. When upgrading
  a mount configured with a base path, choose a new empty mount folder;
  older root markers used only the host and cannot safely identify that
  server. The old folder's files remain available.
- Tokens live in memory for the session only; nothing is stored between
  runs except the content cache and the mount-root marker.
- Windows-only mount backend (`rommfs-fsk` is a `cfg(windows)` target dep);
  core/fixture crates stay portable for tests.

## Development notes

- `fsk = 0.0.9` pinned via committed `Cargo.lock` — feasibility rationale in
  `.planning/BACKEND-DECISION.md`.
- `.planning/PRD.md` is the source spec this implements (R1–R5).
- Format all crates with `cargo fmt --all`. On Windows, lint the full workspace
  with `cargo clippy --workspace --all-targets`; on other platforms, lint the
  portable crates and headless app with
  `cargo clippy -p rommfs-core -p rommfs-fixture -p rommfs-app --no-default-features --all-targets`.
