# Save-sync P1–P5 acceptance checklist

## Implemented contracts

- [x] **P1 — Discovery and consent:** Windows discovers fixed/removable `X:\RetroBat` roots, then running RetroBat/EmulationStation process paths; candidates are read-only validated and path-sorted.
- [x] Prior explicit selection is remembered; if it disappears, sync stays paused and does not switch installations without user action.
- [x] Save sync is off by default. Server, authenticated account, install, effective saves root, profile, mapped game target count, and bounded existing-file supported/skipped inventory are shown before opt-in.
- [x] Consent and debounce are scoped to server/account/install/effective root; credentials are never persisted.
- [x] **P2 — Profile and validation:** only RetroBat 8.2.1 RetroArch/Gambatte Game Boy `.gb` → `gb/<visible stem>.srm` is supported; `.gbc`, RTC, ambiguous/unsupported entries, multi-file titles, save states, memory cards, and conversions are excluded.
- [x] Effective saves path is resolved from settings/launcher configuration without modifying emulator files. Account needs `platforms.read` and `roms.read` for ROM features, plus `me.read`, `assets.read`, and `assets.write` to sync saves.
- [x] Debounce defaults to 5 seconds; only whole seconds 1–3600 are accepted; invalid input is not persisted.
- [x] **P3 — Durable uploads:** immutable per-generation snapshots are journaled privately; one sequential worker uploads UUID names with `overwrite=false&autocleanup=false`, verifies readback bytes, and retries interrupted work after restart.
- [x] Dirty, uncaptured generations and unfinished snapshots count as pending outbound; they cannot display as up-to-date.
- [x] Server-side retention can still prune remote history; client request flags are not a remote retention guarantee.
- [x] **P4 — Reconciliation:** inventory is scoped to authenticated account/ROM/slot; owned UUIDs are acknowledged only after byte verification. Conflicts preserve the existing local file and keep incoming bytes staged; no local winner is selected.
- [x] Automatic first install is allowed only at a never-before-existing target. A tracked local delete requests no remote delete and is never automatically restored.
- [x] Pending incoming review is durable and bounded. Export is account/mapping/profile scoped, local-only, separate-file, no-clobber, and does not clear review or conflict state.
- [x] Review/export remain available while paused or disabled; stale session events and picker results are rejected.
- [x] **P5 — UI and evidence:** controls support keyboard focus/activation with visible focus; queue/auth/failure/retry/reconciliation states are event-backed, not inferred from historical transfer logs.
- [x] Upload retry failures stay attached to their game/revision when another game's upload succeeds, and clear after that revision is verified.
- [x] README distinguishes portable tests/fixture contract from native Windows and live-server verification.

## Portable verification run on this host

- `cargo fmt --all -- --check` — passed.
- `cargo test -p rommfs-core -p rommfs-fixture -p rommfs-app --no-default-features -- --test-threads=1` — passed: app 51 unit + 13 integration; core 52 unit + 60 integration.
- `cargo clippy -p rommfs-core -p rommfs-fixture -p rommfs-app --no-default-features --all-targets -- -D warnings` — passed.
- `cargo check -p rommfs-app --config 'patch.crates-io.xattr.path="/tmp/opencode/rommfs-xattr-check/xattr-patched"'` — passed with a temporary external Linux `xattr` patch mapping upstream `ENOATTR` to `ENODATA`.
- `cargo clippy -p rommfs-app --all-targets --config 'patch.crates-io.xattr.path="/tmp/opencode/rommfs-xattr-check/xattr-patched"' -- -D warnings` — passed with that temporary external patch.
- `cargo fmt --all -- --check`, `git diff --check`, and a check that `Cargo.lock` contains no `/tmp/opencode` path — passed; the committed lock change keeps registry `xattr` metadata.
- Cargo still reports the pre-existing unused `anyhow` manifest dependency warning in `rommfs-core`.

## Windows and live checks — NOT RUN

On Windows, run in PowerShell:

```powershell
cargo test -p rommfs-core --lib save_sync::path::tests::windows_save_read_handle_denies_concurrent_write_and_delete -- --exact
cargo test -p rommfs-core --lib save_sync::path::tests::windows_directory_guard_blocks_rename_until_released -- --exact
cargo test -p rommfs-app --no-default-features --lib save_sync_agent::files::tests::windows_publication_does_not_replace_a_file_created_at_the_boundary -- --exact
cargo test -p rommfs-app --no-default-features --lib save_sync_agent::tests::real_filesystem_notification_drives_the_agent_without_waiting_for_fallback_scan -- --exact
```

- Back up a disposable Game Boy save in two RetroBat installs using one test RomM account with save scopes.
- Discover/verify both installs and previews; enable A and confirm its UUID save appears on RomM.
- Put different test bytes in B before opting in; verify B's local save is unchanged, incoming review remains pending, and **Export…** writes a separate `.rommfs-incoming` copy.
- Disable/restart and verify pending review stays exportable without save API requests; restore the backed-up files.
- Native filesystem/picker and live RomM acceptance remain unverified until these steps are run on Windows against a live server.
- `cargo check -p rommfs-app --target x86_64-pc-windows-gnu --no-default-features` — blocked: this Linux host lacks `x86_64-w64-mingw32-gcc` and a cross-target zlib sysroot.

## Source evidence (not native/live evidence)

- RetroBat 8.2.1: `9761d47af5902410f020164163a7b27a6facc38b`; RetroArch v1.21.0: `baee906ef35b99283f9a1a060a2ce5ac86159b63`.
- Gambatte upstream: `d9d6cd06382d1ced30de34d56d3609452323dab1`; RomM API 5.2.0: `42e8043372522089a3bd724e8f4a0635aa1c6bf9`.
- Workspace fixture models RomM 5.3.1 source ref `95599dadbe93c8f7b8a8647148fafbe293df3167`; fixture coverage is not a live-server result. RomM 5.2.0 OpenAPI was reviewed read-only; no live login, inventory, or upload requests were sent.
