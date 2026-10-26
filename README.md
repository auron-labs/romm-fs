# RomMFS

RomMFS mounts a RomM library as ordinary filesystem paths on Windows. ROM files
appear as Cloud Files placeholders and download their original bytes when read.
It supports Generic folders and a RetroBat layout. The project is under
active development; see [current validation and remaining acceptance](docs/validation/2026-10-12.md).

## Quick start

Requirements and detailed setup are in the [user guide](docs/user-guide.md).
For a local Windows build, install Rust 1.89 or later with the MSVC toolchain
and Windows SDK, then run:

```powershell
cargo run -p rommfs-app
```

Connect to a reachable RomM server, choose an empty local NTFS mount root, and
start the games folder. The first read downloads a file; later reads use cached
bytes. The mount is read-only.

## Testing wizard

On a Windows 10/11 desktop, clone this repository and double-click
`test-wizard.cmd` in its root. The wizard guides the remaining human checks;
Rust is not required unless you choose its optional automated checks/build.
It does not automate or prove the full Windows acceptance procedure.

At each prompt, `S` skips and `Q` saves a partial ZIP and quits; run the launcher
again to resume. `C` asks permission to capture the full desktop, then waits four
seconds so you can switch to the app. Evidence stays local under
`%LOCALAPPDATA%\rommfs\test-wizard`; send the resulting ZIP only after reviewing
its logs and screenshots for private information. Nothing is uploaded, and a
missing or skipped check is never a pass. For help, use `test-wizard.cmd -Help` or
`powershell -NoProfile -File .\scripts\windows-test-wizard.ps1 -Help`.

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

Planning proposals and superseded decisions are retained under `.scratch/archived/`.
