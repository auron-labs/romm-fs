# Development guide

This guide is for contributors and agents making changes in the workspace.
For product setup, see the [user guide](user-guide.md). For module ownership
and behavior, see [architecture](architecture.md).

## Build and checks

Use Rust 1.89 or later. Building the Windows desktop app and CFAPI adapter
requires the MSVC Rust target, Visual Studio C++ build tools, and Windows SDK.
Portable core, fixture, and headless app checks run on Linux, macOS, and Windows.

From the repository root:

```sh
cargo fmt --all --check
cargo test --locked -p rommfs-core -p rommfs-fixture -p rommfs-cfapi -p rommfs-app --no-default-features -- --test-threads=1
cargo clippy --locked -p rommfs-core -p rommfs-fixture -p rommfs-cfapi -p rommfs-app --no-default-features --all-targets -- -D warnings
cargo check --locked -p rommfs-app --all-targets
```

The optional mise aliases are `mise run test:portable` for the headless suite,
`mise run check:ui` for host-native UI compilation, and `mise run test:native`
for the Windows workspace suite. `test:native` refuses to run on non-Windows
hosts. The direct equivalents are shown above and below; mise is not required.

On Windows, `cargo test --locked --workspace -- --test-threads=1` exercises UI
and native CFAPI tests. The native acceptance procedure, including live and
manual cases, is in [Windows testing](testing/windows.md). Compilation and
portable fixtures do not establish native runtime behavior.

## GPUI MCP automation

For local agent-driven UI checks, run the opt-in debug build:

```powershell
mise x -- mbx run --locked -p rommfs-app --features mcp
```

Append `-- --mcp-background` to open the automation window without requesting
desktop focus. This flag applies only to the opt-in debug build.

Quit any existing RomMFS instance first: single-instance activation otherwise
reopens that process. The bridge advertises application ID `rommfs` through
the MCP server's private local discovery directory. It lives with the window,
is absent from default builds, and is never installed in release builds.
The password field is redacted from semantic frame observation.

The bridge and its required `gpui-pre` patch are pinned to the same
[`gpui-mcp` revision](https://github.com/themixednuts/gpui-mcp/tree/eb248ccf43847797b9067c47eeef1dd188dc03a6).
Cargo applies that framework patch to every UI build, including builds with
the bridge feature disabled. The MCP dependencies require Rust 1.96 or later;
the repository's mise toolchain satisfies this requirement.

Install the matching server with `mise x -- mbx install --git
https://github.com/themixednuts/gpui-mcp --rev
eb248ccf43847797b9067c47eeef1dd188dc03a6 --locked gpui-mcp-server`, then register
its `gpui-mcp` executable as a stdio MCP server in the agent client. The
`--app-id rommfs` server argument limits discovery to RomMFS. Agent actions
operate on the actual application and retain its current account, settings,
and save-sync consent; use test roots/accounts for scenarios that change data.
Pointer and keyboard actions are injected into GPUI rather than the global
desktop input stream. Native dialogs and tray UI need separate coverage.

## Where to change and verify

| Change | Primary code | Useful tests/docs |
|---|---|---|
| RomM auth, pagination, response parsing | `crates/rommfs-core/src/romm/` | `crates/rommfs-core/tests/romm_client.rs`, `crates/rommfs-core/tests/pairing.rs`; [API contract](contracts/romm-api.md) |
| Catalogue/path projection | `crates/rommfs-core/src/catalogue.rs`, `crates/rommfs-core/src/layout.rs`, `crates/rommfs-core/src/tree.rs` | `crates/rommfs-core/tests/catalogue.rs`, `crates/rommfs-core/tests/catalogue_observed_shape.rs`, `crates/rommfs-core/tests/tree.rs` |
| Cache/download/hydration | `crates/rommfs-core/src/cache/`, `crates/rommfs-core/src/download.rs`, `crates/rommfs-core/src/filesystem.rs`, `crates/rommfs-cfapi/src/` | `crates/rommfs-core/tests/cache_index.rs`, `crates/rommfs-core/tests/download.rs`, `crates/rommfs-core/tests/filesystem.rs`, `crates/rommfs-cfapi/tests/cfapi_native.rs` on Windows |
| Connection/controller behavior | `crates/rommfs-app/src/controller.rs` dispatches to `crates/rommfs-app/src/controller/state.rs`, `crates/rommfs-app/src/controller/commands.rs`, `crates/rommfs-app/src/controller/connection.rs`, `crates/rommfs-app/src/controller/catalogue.rs`, `crates/rommfs-app/src/controller/mount.rs`, `crates/rommfs-app/src/controller/retrobat_setup.rs`, and `crates/rommfs-app/src/controller/save_sync_setup.rs`; helpers are `crates/rommfs-app/src/controller/auxiliary.rs`, `crates/rommfs-app/src/controller/installer.rs`, and `crates/rommfs-app/src/controller/save_files.rs` | `crates/rommfs-app/src/controller/tests.rs`, `crates/rommfs-app/tests/controller.rs`, and `crates/rommfs-app/tests/controller/` |
| Save mapping/journal/transfer | `crates/rommfs-core/src/save_sync/`, `crates/rommfs-core/src/save_sync/retrobat_paths.rs`, `crates/rommfs-core/src/save_sync/legacy_gb_profile.rs`, and `crates/rommfs-app/src/save_sync_agent/` | `crates/rommfs-core/tests/save_file_rules.rs`, `crates/rommfs-core/tests/ordinary_save_api.rs`, and `crates/rommfs-app/src/save_sync_agent/tests/` |
| RetroBat setup and metadata | `crates/rommfs-core/src/retrobat/`, `crates/rommfs-app/src/retrobat_discovery_windows.rs`, `crates/rommfs-app/src/controller/` | `crates/rommfs-core/tests/retrobat_setup.rs`, `crates/rommfs-core/tests/retrobat_metadata.rs`, `crates/rommfs-core/tests/retrobat_artwork.rs` |
| UI and navigation | `crates/rommfs-app/src/window.rs`, `crates/rommfs-app/src/window/actions.rs`, `crates/rommfs-app/src/window/folder_controls.rs`, `crates/rommfs-app/src/window/pages/` | `crates/rommfs-app/tests/controller.rs` and `crates/rommfs-app/tests/controller/`; [GPUI Kit guides](../.agents/skills/gpui-kit/SKILL.md) |
| Fixture behavior | `crates/rommfs-fixture/src/lib.rs` | Add/adjust assertions in the consuming integration test |

Tests are organized partly as integration tests and partly inline under the
module they exercise. `cargo test --locked -p <crate> <filter>` can narrow a run; use
`-- --exact` only with a fully qualified test name and check that the command
reports a test actually ran.

## Evidence and current behavior

The [architecture guide](architecture.md) documents observed ownership and
flow. The [API contract](contracts/romm-api.md) distinguishes live checks from
source/fixture-backed behavior. The dated [validation record](validation/2026-10-12.md)
reports what was executed on Linux and Windows Server 2022 and lists remaining
Windows 10/11 and manual acceptance.
