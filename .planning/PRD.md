# RomMFS — Windows-first proof of concept

## 1. Goal and scope authority

Build a small Rust desktop application that exposes an existing RomM library as ordinary filesystem paths. An existing frontend such as ES-DE discovers the ROMs; reading an uncached ROM downloads it; subsequent reads use local data; unused data is evicted without removing the ROM from the visible tree.

The application is **not a game launcher or library browser**. Its UI is only for connection/login, mount controls, status, download progress, and logs.

This document defines the complete PoC scope. Reference projects are API documentation, not permission to copy their features. Do not invent additional product requirements, production-readiness milestones, or infrastructure.

### MVP decisions

- **Ship Windows first**, using a folder such as `C:\RomM`, not a new drive-letter implementation.
- **Use WinFsp**, as requested on 2026-10-05. Use **GPUI** for the small desktop window.
- Keep the filesystem/client/cache logic independent of the UI and Windows-only code. Linux is the next target, particularly SteamOS and Batocera, but their deployment and UI integration are **not deliverables for this PoC**. macOS is also deferred.
- One running application, one configured RomM server, one mount. No separate daemon or service.
- Prove the workflow with **single-file ROMs**, downloaded unchanged and in full. Folder-based/multi-file games are outside this PoC; report them as unsupported rather than pretending a generated ZIP is the original game file.
- Use a **14-day inactivity threshold** as the default, following the proposed cache behavior. Keep it as a simple implementation setting; tests inject a clock/shorter duration. Do not build a cache-settings screen.

## 2. Backend decision: a small experiment, not a research project

Use the `winfsp` Rust bindings and a read-only Windows volume. This supersedes
all earlier backend selection and optional-feature requirements. Keep the
existing portable `RommFs` facade and GPUI window.

Verify Windows behavior through ordinary OS operations:

1. List/stat directories and files without content downloads; read correct bytes on demand.
2. Reopen cached ROMs and track each open handle to prevent active eviction.
3. Evict private cached bytes while keeping catalogue entries discoverable; re-download on the next read.
4. Reject writes, creation, rename and deletion throughout the volume; stop and remount cleanly.

WinFsp has no persistent hydrated copy in the mount directory. Its runtime is
installed separately; report missing prerequisites without elevation. Pin the
bindings in the lockfile. At the user's request, native Windows build and
runtime validation are deferred and do not gate completion of this migration.
Portable tests and cross-compilation do not prove that mounting works.

## 3. Required user workflow

1. Open the app, enter the RomM URL, username, and password, and connect.
2. Choose an empty mount folder and start the mount.
3. Point ES-DE at the projected tree using its normal configuration. Games appear without pre-downloading their contents.
4. Launch a game. Its first content read starts one download, and the app shows real progress. The requesting process waits for the data.
5. Launch the same game again. Cached data is reused without another content download.
6. After the inactivity threshold, unused local data is removed. The game remains visible and downloads again on its next content read.
7. Stop the mount or quit the app cleanly.

The app does not launch the emulator itself. Initial validation uses a frontend/system whose folder convention matches the projected platform folder. Document that example; do not build an exhaustive platform-mapping or launcher-configuration system. ES-DE uses platform subdirectories under a configurable ROM root. [S5]

## 4. Functional requirements

### R1 — Connection and catalogue

Provide URL, username, and masked password inputs. Use RomM’s existing username/password token flow, with the read scopes actually required by the endpoints used. Send authentication on metadata and download requests. Keep credentials/tokens in memory only; no remembered login or credential-store integration. On expired/rejected authentication, show that sign-in is required and fail affected requests clearly. Automatic token refresh is not required for this PoC. [S6]

Verify the target RomM version’s API contract before implementing it. Record the tested version and the few routes/fields used. Use the instance’s OpenAPI document when available; otherwise use a specific published version’s schema/source and label it. Do not invent response shapes or generate an SDK for the entire API. RomM exposes an instance-specific OpenAPI document, and endpoint pagination varies. [S7]

Load all pages needed for the accessible platforms and ROMs when starting the mount. Keep that catalogue stable for the mount session. Stop/start reloads it; no polling, WebSockets, or live synchronization.

A failed catalogue request must not become a successful empty library. Distinguish login failure, permission failure, server/network failure, and a genuinely empty library in status/logs. Unsupported multi-file entries are skipped with an explanatory log and count.

### R2 — Visible filesystem tree

Expose a platform directory and original filename for each supported ROM, for example:

```text
C:\RomM\
  nes\
    Example Game.nes
  snes\
    Another Game.sfc
```

Use the filesystem/platform identifier and original filename supplied by the verified RomM contract, not an artwork title or invented extension. Return correct logical sizes and file/directory types from metadata.

Listing, lookup, and stat must not download ROM contents. Implement directory continuation correctly: small enumeration buffers must not lose or duplicate entries. Paths and identities must stay stable across repeated enumeration/remounts of an unchanged catalogue.

Validate remote path components before using them locally. Reject traversal/absolute paths. Handle Windows-invalid names, reserved names, and case-insensitive collisions deterministically while preserving the file extension; log any necessary visible-name adjustment. Do not silently overwrite or merge different ROMs. Derive private cache paths from stable identities, not untrusted response filenames.

Projected ROM entries are read-only. No writes, renames, deletions, or uploads are supported. Verify this through ordinary Windows filesystem operations; returning an error from the Rust trait alone is not proof. Existing files in a chosen mount folder are never permission to upload or purge user data; refuse nonempty roots. Only app-owned, identified cached ROM data is eligible for cleanup.

### R3 — Download on first content read

Trigger downloading on the first actual content read—not directory enumeration or a metadata-only open. A zero-length read or a read at EOF needs no download.

Download the **complete original file** into a private temporary file outside the projected tree. Stream to disk; do not buffer the entire ROM in memory. Check successful transfer completion and expected length, then atomically publish the completed cache entry. Never serve a partial file as complete, or substitute an HTML/error response for ROM bytes. RomM’s ordinary content endpoint supports authenticated downloads; multi-file downloads can instead be generated ZIPs, which this PoC does not expose as original ROMs. [S8]

Reads wait for completion, then return the requested offset/length from the cached file. Correct random access, short reads at EOF, and reopening matter; do not implement HTTP range streaming, partial-play, resume, extraction, or a block cache.

Concurrent requests for the same uncached ROM share one in-flight download. Do not hold a global catalogue/cache lock across network I/O. Other metadata operations and the UI must remain responsive.

On a failed/truncated download, disk-write failure, or connection/stall timeout: fail waiting reads, report the cause, and do not mark the entry ready. A later read may retry from the start. Do not retry forever. Use connection/no-progress timeouts rather than a short total timeout that necessarily rejects large but progressing downloads. Quit/stop must release pending work rather than hang indefinitely.

**Known limitation:** scanners, antivirus software, or a frontend reading headers can trigger downloads too. This is data-read detection, not “game launch” detection. Do not add process allowlists or launcher hooks to hide that distinction.

### R4 — Cache reuse and inactivity eviction

Use a small SQLite index for cache identity, completion state, and last use; keep transient downloads and active-use tracking in memory. Completed downloads and last-use records survive app restarts. Ignore/remove incomplete app-owned downloads on restart rather than treating them as complete.

Scope cache identities to the server and ROM/file identity. Use available version/hash metadata to invalidate known changed content when reloading the catalogue. Do not build change detection beyond the supplied metadata or claim detection of remote edits that leave it unchanged.

Record file access/open activity, including warm Windows accesses that may bypass the provider’s `read` method. Do not use last-download time as last-use time. Keep access tracking conservative where the backend cannot distinguish metadata opens; listing/stat still must not initiate content downloads.

Periodically evict entries older than the threshold, but never active downloads or files still in use. Coordinate eviction with new access so a check-then-delete race cannot remove a newly acquired cache entry. When access/lock state is uncertain, defer eviction. A busy file or failed cleanup is logged and retried on a later sweep, not force-deleted.

**Windows-specific requirement:** WinFsp reads directly from the private cache. Eviction must reclaim that cached file safely while the catalogue continues exposing the same path.

Remove only identified private cache files while holding the existing eviction guard. WinFsp owns no hydrated disk copy and requires no provider deletion API. Per-open guards protect active ROMs; the host owns file contexts and dispatcher lifetime through shutdown. Never retain borrowed callback buffers or pointers for later use. [S9–S10]

The private cache is the only persistent ROM storage. Successful eviction must reclaim it; the subsequent read must re-download correct bytes.

### R5 — One small GPUI window

Use one functional window containing only:

| Area | Content |
| --- | --- |
| Connection | Server URL, username, masked password, Connect action, actual connection/error state. |
| Mount | Mount-folder input, Start/Stop action, mounted path and actual mount state. |
| Downloads | Active filename, bytes received/total, progress indicator, completion/failure state. Use indeterminate progress if the total is genuinely unknown. |
| Logs | Scrollable, bounded, selectable/copyable diagnostic text with timestamps, severity, operation, and relevant ROM/path/error context. |

Display real worker events, not timers or fabricated progress. HTTP completion is not proof that an emulator launched successfully. Run network/filesystem work off the UI thread.

Log connection, catalogue, mount, download, eviction, and failure transitions. Preserve useful paths, IDs, status codes, and error causes for debugging. Never log passwords, tokens, cookies, or authentication headers; do not build a generalized redaction framework or suppress ordinary diagnostics.

Closing the app stops its workers and mount. No tray process, background service, autostart, notification system, theme settings, or persistent log-management subsystem.

## 5. Implementation boundaries

Use reusable Rust library modules for the RomM client, catalogue/tree, download/cache behavior, and filesystem adapter. GPUI calls that library and consumes a simple event channel; it must not contain its own download or eviction rules.

Core tests must run without a GPU, GPUI window, native mount, real RomM server, or user credentials. Isolate Windows-specific code and dependencies behind target-specific boundaries so the core remains usable on Linux. Do not introduce a second generic filesystem abstraction over `winfsp`, a plugin architecture, dependency-injection framework, network control API, or multiple application services.

Keep the mount root and private cache separate. Mount only into an empty ordinary directory, either unclaimed or owned by this server, never over an existing ROM library. Store ownership beside the mount path; old nonempty managed roots are refused. Do not recursively clear an arbitrary folder to make mounting succeed. Reusing managed roots must not mix content from different servers.

WinFsp requires its installed Windows runtime; check/report availability and document installation. Do not silently elevate or install drivers. The GPUI window displays download progress.

## 6. Tests: required behavior, not a coverage target

Use a tiny local HTTP fixture server and exercise the real RomM client/cache implementation against it. Fixture request counts are useful to prove “no download on listing” and “one download for concurrent reads”; asserting arbitrary internal mock-call sequences is not.

The following are **behavioral cases**, not a required number of test functions. Combine or parameterize related cases. Each test must identify the production failure it prevents.

| Case | Required evidence |
| --- | --- |
| Authentication/catalogue | Actual client authenticates with the verified request shape; authenticated requests work; rejected credentials/permissions and failed pages produce errors, not a mounted empty library. Multiple pages produce the complete expected catalogue. Captured app logs contain no test credentials. |
| Tree without downloads | Lookup/stat and repeated small-buffer enumeration return expected paths, types, sizes, and entries without requesting ROM bodies. Include Windows name collisions/traversal and confirm different ROMs cannot alias the same file/cache path. |
| Byte correctness | Through the real filesystem-facing implementation, cold reads return exact fixture bytes at sequential and non-sequential offsets, with correct EOF behavior. No `.part` content becomes visible. |
| Single-flight/responsiveness | Concurrent reads of the same ROM cause one content transfer and both receive correct bytes. A deliberately held download does not block an unrelated metadata request. Use synchronization barriers, not timing guesses. |
| Failure and retry | A truncated body, HTTP error, and injected disk-write failure each fail reads without a ready cache entry. A subsequent successful transfer works. A stalled transfer releases waiters through the configured failure path. |
| Warm cache/restart | Reopening completed data, including after reconstructing the app’s cache state, does not re-download it. Incomplete files are not reused. Different server identities and known content-version changes cannot reuse the old bytes. |
| Eviction policy | With an injected clock, inactive data is evicted, recent/open/downloading data is retained, new access racing a sweep is protected, failed cleanup is deferred, and a later read downloads again while the virtual entry stays present. |
| UI behavior | Drive the real application/controller event path: login/mount failure cannot display success; transfer events update progress; terminal failure clears downloading state; stop/quit requests reach the worker lifecycle. Do not test a duplicate state machine that the UI does not use. |

### Native Windows integration test — mocks are not a substitute

Provide an explicitly runnable test using a temporary WinFsp mount and the local fixture server. Exercise **ordinary OS file operations**, not direct trait calls, to verify:

- Enumeration/stat do not request content; cold, warm, seeked, and memory-mapped reads return correct bytes.
- A warm reopen updates access tracking even when Windows supplies cached bytes. An open file survives an eviction attempt.
- Expired cached data is actually reclaimed from the private cache, the path remains enumerable, and reading it causes a new successful transfer. Check physical allocation/cache state, not just the displayed logical file size.
- Projected ROM modification/rename/delete are rejected, and stopping/restarting the provider does not make managed cached data incorrect or the root unusable.

Test slow-but-progressing reads as well as immediate fixture responses: success with an in-memory backend alone does not establish the download use case.

This native suite may be separate from ordinary tests. When explicitly invoked, missing prerequisites must be reported as unavailable/failure, never silently treated as a pass. Without Windows access, implement it and report **not run**, while continuing portable implementation.

### Tests expressly forbidden

Do not add tests that only assert constructors/getters/derived traits, enum spelling, hard-coded constants, files or symbols existing, documentation text matching, or that mocks return their configured values. Do not read source files and search for implementation strings as a substitute for executing behavior.

Do not add cosmetic UI snapshots, arbitrary coverage thresholds, minimum test counts, unrelated benchmarks/load/fuzz suites, or duplicate the same rule at every layer. Fixture/schema tests are allowed when they catch actual external API parsing failures. Additional regression tests are allowed only for a concrete bug in the above scope. No framework/dependency behavior tests unrelated to our integration.

## 7. Build order and completion

Implement in four small slices:

1. `winfsp` Windows mount/cache-lifecycle experiment; minimal GPUI compile; record the backend decision or outstanding native verification.
2. Real RomM authentication/catalogue and portable tree/download behavior, with the relevant executable tests.
3. Persistent cache, access tracking, Windows-aware eviction, and native integration harness.
4. Wire the small GPUI window to real operations; run the available integration checks and the frontend smoke test.

Do not stop after writing a plan. Keep any implementation checklist short and map its tasks to R1–R5. Do not expand the project into a new specification or roadmap.

The frontend smoke test uses a user-provided or freely distributable single-file ROM: point ES-DE at the mounted tree, verify discovery without content prefetch, launch it in an existing emulator with visible download progress, launch it again from cache, then exercise shortened-threshold eviction and launch it again. Keep emulator saves, frontend metadata, and artwork outside the read-only ROM tree. Record the tested versions/system and any scan-triggered reads; do not add launcher workarounds to conceal failures.

Deliver the source, lockfile, focused tests, and one short README containing build/run/test commands, Windows prerequisites, the tested RomM contract/version, the frontend setup example, and observed limitations. Run formatting, applicable Clippy checks, and tests supported by the available host. Do not create a release pipeline or cross-platform packaging project.

Final handoff must distinguish **implemented**, **passed**, **failed**, and **not run (reason)**. Actual Windows mounting and the frontend workflow remain runtime validation tasks, deferred by the user for this migration. Missing hardware or credentials may leave those checks outstanding; they must not stop independent implementation or be misrepresented as success.

## 8. Explicit exclusions

No custom game browser/launcher; artwork, metadata export, scraping, or generated gamelists; emulator installation/configuration; save synchronization; BIOS management; game sourcing; DLC/update automation; multi-file reconstruction; archive extraction; range streaming; download pause/resume; prefetch/pinning; cache quotas; remote modifications; live catalogue synchronization; multiple profiles/mounts; OIDC/QR pairing; stored credentials; telemetry; tray/autostart/services; installers/auto-updates; SteamOS/Batocera/macOS deployment work.

Do not implement excluded work “to prepare for later.”

## References — technical evidence, not additional scope

- [S1] [WinFsp Rust bindings](https://docs.rs/winfsp/0.13.1+winfsp-2.1/winfsp/) — API and prerequisites.
- [S2] [WinFsp FileSystemContext](https://docs.rs/winfsp/latest/winfsp/filesystem/trait.FileSystemContext.html).
- [S4] [Official GPUI README](https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md).
- [S5] [ES-DE user guide](https://gitlab.com/es-de/emulationstation-de/-/blob/master/USERGUIDE.md), particularly ROM directory structure and non-standard directories.
- [S6] [RomM API authentication](https://docs.romm.app/latest/developers/api-authentication/).
- [S7] [RomM OpenAPI/versioning](https://docs.romm.app/latest/developers/openapi/).
- [S8] [RomM downloads](https://docs.romm.app/latest/using/downloads/) and [RomM source](https://github.com/rommapp/romm).
- [S9] [WinFsp volume parameters](https://docs.rs/winfsp/latest/winfsp/host/struct.VolumeParams.html).
- [S10] [WinFsp host lifecycle](https://docs.rs/winfsp/latest/winfsp/host/struct.FileSystemHost.html).
- [S11] [WinFsp installation](https://winfsp.dev/rel/).
