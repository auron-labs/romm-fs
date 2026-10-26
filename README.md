# RomMFS

RomMFS makes your RomM games available as a Windows folder. Connect your
server, choose a folder, and use your games in RetroBat or another emulator.
Game files download when opened and are cached for later use.

- **RomM** stores your library on the server.
- **RomMFS** connects that library to this PC and keeps the games folder available.
- **RetroBat or your emulator** browses and plays the games.

Artwork updates and optional save sync run alongside the games folder. Save
sync is off until you enable it. The project is under active development.

## Quick start

Requirements and detailed setup are in the [user guide](docs/user-guide.md).
For a local Windows build, install Rust 1.89 or later with the MSVC toolchain
and Windows SDK, then run:

```powershell
cargo run --locked -p rommfs-app
```

1. Enter your RomM server address and choose **Sign in with browser…**.
   Approve the connection in RomM.
2. Choose an empty folder on a local NTFS drive for your games.
3. For RetroBat, follow **Settings → RetroBat** to connect your installation.
   See the [RetroBat setup guide](docs/user-guide.md#set-up-retrobat).
4. On **Overview**, choose **Make games available**. When the status becomes
   **Available**, open RetroBat or point your emulator at the games folder.

Keep RomMFS running while you play. The first read downloads a game file;
later reads use cached bytes. Game files in this folder are read-only.
See [everyday use](docs/user-guide.md#everyday-use) for saves, updates, and
what the status messages mean.

## Workspace

| Crate | Responsibility |
|---|---|
| `rommfs-core` | RomM client, catalogue, cache, save logic, and RetroBat setup/metadata. |
| `rommfs-fixture` | In-process HTTP fixture used by portable tests. |
| `rommfs-cfapi` | Windows Cloud Files adapter and portable mount-root ownership checks. |
| `rommfs-app` | GPUI desktop UI, controller, session, and save-sync worker. |

## Project docs

- [User guide](docs/user-guide.md): requirements, Generic folders, RetroBat, save sync, and limitations.
- [Development guide](docs/development.md): build and check commands, crate/task map, and where tests live.
- [Architecture](docs/architecture.md): current crate boundaries, data flow, and invariants.
- [RomM API contract](docs/contracts/romm-api.md): verified and source-backed API behavior, with evidence boundaries.
- [Windows testing](docs/testing/windows.md): native acceptance procedure and report format.
- [Dated validation record](docs/validation/2026-10-12.md): implementation status and recorded Linux/Windows evidence.

## Testing wizard

Contributors can run `test-wizard.cmd` on Windows for guided checks and local
evidence collection. See [Windows testing](docs/testing/windows.md) for the
procedure, resume/skip controls, and screenshot privacy information.
