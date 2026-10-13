# Agent guide

Start with this file, then read only the task-specific code and tests below.
The root README is the entry point for people; current implementation facts
belong in `docs/architecture.md`, `docs/development.md`, and the linked
contracts/validation records. `.planning/archived/` contains historical plans,
not current behavior.

## Workspace map

- `crates/rommfs-core`: portable RomM HTTP client, catalogue projection,
  download/cache, save-sync domain and journal, RetroBat setup and metadata.
- `crates/rommfs-fixture`: in-process HTTP server and scripted RomM responses
  used by core and app tests.
- `crates/rommfs-cfapi`: portable mount-root ownership checks plus the
  Windows-only Cloud Files API adapter and security code.
- `crates/rommfs-app`: controller and event state, GPUI window, app session,
  save-sync actor/transfer worker, and Windows desktop integration.

## Task map

- RomM authentication, paging, DTOs, or routes: `crates/rommfs-core/src/romm/`,
  `crates/rommfs-core/tests/romm_client.rs`, `crates/rommfs-core/tests/pairing.rs`, and
  `docs/contracts/romm-api.md`.
- Catalogue projection, filenames, supported layouts: `crates/rommfs-core/src/catalog.rs`,
  `crates/rommfs-core/src/layout.rs`, `crates/rommfs-core/src/tree.rs`, and
  `crates/rommfs-core/tests/catalogue.rs`,
  `crates/rommfs-core/tests/catalogue_observed_shape.rs`, and
  `crates/rommfs-core/tests/tree.rs`.
- Mount ownership, placeholder behavior, hydration, or eviction:
  `crates/rommfs-cfapi/src/root.rs`, `crates/rommfs-cfapi/src/imp.rs`, `crates/rommfs-core/src/fscore.rs`,
  `crates/rommfs-core/src/cache/`, `crates/rommfs-core/tests/fscore.rs`, and
  `crates/rommfs-cfapi/tests/cfapi_native.rs` (Windows).
- Controller state, connection, catalogue refresh, mounting, or setup:
  `crates/rommfs-app/src/controller.rs` dispatches to focused modules in
  `crates/rommfs-app/src/controller/`: `state.rs`, `commands.rs`,
  `connection.rs`, `catalogue.rs`, `mount.rs`, `retrobat_setup.rs`, and
  `save_sync_setup.rs`; auxiliary/install/save-file helpers are in
  `auxiliary.rs`, `installer.rs`, and `save_files.rs`. Tests are grouped in
  `crates/rommfs-app/src/controller/tests.rs` and
  `crates/rommfs-app/tests/controller/`.
- Save reconciliation, transfer, cancellation, or file publication:
  `crates/rommfs-core/src/save_sync/`, `crates/rommfs-app/src/save_sync_agent/`,
  `crates/rommfs-app/src/controller/save_files.rs`,
  `crates/rommfs-core/tests/ordinary_save_api.rs`, and
  `crates/rommfs-app/src/save_sync_agent/tests/`.
- RetroBat save-root resolution or the legacy GB profile path:
  `crates/rommfs-core/src/save_sync/retrobat_paths.rs` and
  `crates/rommfs-core/src/save_sync/legacy_gb_profile.rs`.
- RetroBat install, links, metadata, artwork, or auxiliary files:
  `crates/rommfs-core/src/retrobat/`, relevant controller methods, and RetroBat integration
  tests in `crates/rommfs-core/tests/retrobat_setup.rs`,
  `crates/rommfs-core/tests/retrobat_metadata.rs`, and
  `crates/rommfs-core/tests/retrobat_artwork.rs`. Check the API contract for
  server-side assumptions.
- Window layout or page behavior: `crates/rommfs-app/src/window.rs`, `crates/rommfs-app/src/window/pages.rs`,
  and `crates/rommfs-app/src/window/pages/`;
  follow the GPUI Kit guides before changing GPUI state or visual structure.
- Fixture routes or request assertions: `crates/rommfs-fixture/src/lib.rs` and
  the consuming integration test. Prefer route-specific counters where the
  assertion needs to distinguish scripted responses from wire requests.

## Editing and verification

- Read the owning module and its callers before changing a behavior. Follow the
  existing public API and persistence patterns; avoid broad reshaping when a
  focused change at the shared seam solves the issue.
- Add or update a behavior test when the change affects a contract, lifecycle,
  ownership rule, path mapping, or concurrency boundary. Tests should verify
  observable behavior, not restate implementation details.
- Use `rg` from the repository root for symbol and path discovery. Do not infer
  that a test passed when a filter matched zero tests.
- Keep current behavior facts in `docs/`; do not revise archived plans as if
  they were live requirements. Update the dated validation record only when
  there is new execution evidence.

## Invariants to preserve

- Catalogue listing and enumeration do not download ROM content; hydration
  fetches the identified original file only when bytes are read.
- A mounted tree is a snapshot. Explicit/background refresh updates catalogue
  state separately; remount reuses a loaded snapshot and cache where applicable.
- Catalogue and remembered connection state are scoped to normalized server
  identity and authenticated account where applicable.
- Never replace, delete, or recursively clear unowned user files. Mount cleanup
  acts only on verified RomMFS-owned paths and preserves changed/busy files.
- Save-sync consent is scoped to its identity and selected roots. Do not infer
  consent from a discovered path or from an older, narrower scope.
- Cancellation and account/session changes must drain owned workers; stale
  events must not mutate a newer session.

## Checks and working rules

Run commands from the workspace root. Use the literal paths above when searching;
they are repository-relative.

- Format: `cargo fmt --all --check`.
- Portable/headless tests: `cargo test --locked -p rommfs-core -p rommfs-fixture -p rommfs-cfapi -p rommfs-app --no-default-features -- --test-threads=1`.
- Native-host UI compilation: `cargo check --locked -p rommfs-app --all-targets`.
- Portable lint: `cargo clippy --locked -p rommfs-core -p rommfs-fixture -p rommfs-cfapi -p rommfs-app --no-default-features --all-targets -- -D warnings`.
- Windows workspace tests: `cargo test --locked --workspace -- --test-threads=1`; native acceptance procedure:
  `docs/testing/windows.md`.
- Optional mise aliases: `mise run test:portable`, `mise run check:ui`, and
  Windows-only `mise run test:native` provide the portable suite, host UI
  compilation, and native Windows workspace tests respectively. `test:native`
  fails clearly on non-Windows hosts.
- Keep `--locked` on Cargo checks and run test binaries serially where
  supported to reduce pressure on native filesystem watchers. These commands
  are starting points, not proof of native Windows behavior. Record platform
  limitations in the task handoff.
- Do not edit `.planning/archived/` to make historical plans appear current.
  Update canonical docs when current behavior changes.
- For GPUI work, read `.agents/skills/gpui-kit/SKILL.md` and
  `.agents/skills/gpui-kit-design-guides/SKILL.md` as relevant. For Rust
  ownership or async work, read `.agents/skills/rust-engineer/SKILL.md`.
