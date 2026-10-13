# RomM API contract and source-backed extensions

Instance: local Docker `rommapp/romm:latest` pulled 2026-10-04.
**Tested version: RomM API 5.3.1** (from `/openapi.json` `info.version`; startup log confirms 5.3.1).

Auth token endpoint, response, and catalogue/download responses below were
exercised against the live instance and produced the documented shapes.

The original password/single-file checks below are live-instance evidence.
Pairing, nested original files, metadata and artwork were subsequently inspected
at RomM revision `95599dadbe93c8f7b8a8647148fafbe293df3167` and exercised with
local HTTP fixtures. They have **not** been live-validated against that instance.
Do not interpret a fixture pass as server compatibility or Windows evidence.

## Authentication (R1)

`POST /api/token` — `application/x-www-form-urlencoded` body:

| field | value |
| --- | --- |
| `grant_type` | `password` |
| `username` | user |
| `password` | user |
| `scope` | space-separated scopes; we request `platforms.read roms.read` |

Response 200 JSON `TokenResponse`:
`{access_token, token_type: "bearer", expires: <secs>, refresh_token, refresh_expires}`

Send `Authorization: Bearer <access_token>` on all catalogue/download calls.
Endpoints also accept HTTP Basic (documented fallback; we use Bearer per PRD).
Insufficient scope → HTTP 403 `{"detail": "Insufficient scope"}`.
Bad credentials → HTTP 401.

Observed scope list on admin user includes `platforms.read`, `roms.read`,
`roms.user.read`, `tasks.run`, etc.

## Catalogue

`GET /api/platforms` → JSON array `PlatformSchema`. Fields used:
`id:int`, `slug`, `fs_slug`, `name`, `custom_name:string|null`, `rom_count:int`.

`GET /api/roms` → `CustomLimitOffsetPage_SimpleRomSchema_`:
`{items:[SimpleRomSchema], total:int|null, limit:int, offset:int, ...}`.
Query params used: `limit` (page size, int), `offset`, `with_files=true`,
`platform_ids` (repeatable int list — optional filter). Page until accumulated
items reach `total` or a short/empty page arrives.

`SimpleRomSchema` fields used: `id`, `platform_fs_slug`, `platform_slug`,
`fs_name` (original filename incl. extension), `fs_size_bytes`,
`has_simple_single_file`, `has_nested_single_file`, `has_multiple_files`,
`missing_from_fs`, `is_physical`, `updated_at`, `files: [RomFileSchema]`.

`RomFileSchema` fields used: `id`, `file_name`, `file_size_bytes`,
`last_modified:string|null`, `crc_hash|md5_hash|sha1_hash:string|null`
(version/cache-invalidation metadata), `is_top_level`.

The former single-file restriction is superseded. Each original file is keyed
by `(rom_id, file_id)`. `file_path`, `file_name`, `is_top_level`, `category` and
the ROM's `fs_path`/`fs_name` determine its relative path. Nested paths are
validated before projection; zero records and unsafe/out-of-platform paths are
reported rather than converted into an empty successful download.

## Download

`GET /api/roms/{id}/content/{file_name}` (also supports HEAD)
→ 200 `application/octet-stream` body = original bytes, `Content-Length` set,
`Content-Disposition: attachment; filename*=UTF-8''<name>`.
404 JSON `{"detail": ...}` when missing. Verified byte-identical vs source file.

That ROM-level route can generate ZIPs for nested/multi-file ROMs. Current
hydration uses **`GET /api/roms/{file_id}/files/content/{file_name}`** instead:
`file_id` identifies a `RomFile`, despite appearing beneath `/api/roms`.
Only the original basename is URL-encoded in this route. Local nested paths
come from the catalogue, not from an invented nested content URL.

## Browser pairing and remembered sessions (source-backed)

- `POST /api/auth/device/init`, JSON with `client_device_identifier`, `name`,
  `client`, `platform`, `client_version`, `requested_scopes` → 201 JSON with
  `device_code`, `user_code`, `verification_path`,
  `verification_path_complete`, `expires_in`, `interval`.
- `POST /api/auth/device/token`, JSON `{device_code}` → 200 JSON with
  `access_token`, `device_id`, `scopes`, optional `expires_at`.
  HTTP 400 detail values: `authorization_pending`, `slow_down`,
  `access_denied`, `expired_token`; 429 slows polling.
- The browser opens the same-origin `/pair/device?...` approval page.
- Client tokens have **no refresh-token route**. Rejected sessions require
  pairing again; password OAuth token refresh semantics do not apply to them.
- `GET /api/users/me` verifies the numeric account before cached metadata is
  restored. Snapshots are scoped by normalized server and account, not username.
- Requested scopes: `platforms.read roms.read me.read assets.read assets.write firmware.read`.
  Missing optional save permissions pause saves rather than granting consent.
- Only paired secrets are stored in Windows Credential Manager. Preferences are
  nonsecret SQLite data. Passwords are not persisted; Linux has no plaintext
  remembered-secret fallback.

Source: [device authentication endpoints](https://github.com/rommapp/romm/blob/95599dadbe93c8f7b8a8647148fafbe293df3167/backend/endpoints/device_auth.py).

## Descriptive metadata and artwork (source-backed)

The paginated ROM response supplies `name`, `summary`, `metadatum` (genres,
developers, publishers, first release timestamp, player count and average
rating), `path_cover_large`, `path_cover_small`, and `merged_screenshots`.
No per-game detail request is needed. Ratings on a 0–100 scale become 0–1
frontend ratings; release timestamps become `YYYYMMDDT000000`.

Only server-local `/assets/romm/resources/...` paths supplied by the catalogue
are fetched. External metadata-provider URLs and traversal are rejected before
network access. The resource path/query version is cached with the ROM update
timestamp. Artwork has four workers and a 32 MiB per-resource limit. Unsupported
raster signatures and unavailable images are visible, nonfatal failures.
Listing, XML generation and artwork requests never request ROM content.

Sources: [ROM schema](https://github.com/rommapp/romm/blob/95599dadbe93c8f7b8a8647148fafbe293df3167/backend/endpoints/responses/rom.py),
[resource prefix](https://github.com/rommapp/romm/blob/95599dadbe93c8f7b8a8647148fafbe293df3167/backend/config/__init__.py),
[resource serving](https://github.com/rommapp/romm/blob/95599dadbe93c8f7b8a8647148fafbe293df3167/docker/nginx/templates/default.conf.template).

## File destinations and remaining limits

| Content | API/source | Generic folders | RetroBat | Launchable metadata |
| --- | --- | --- | --- | --- |
| Game/hack/demo/translation originals | ROM `files`, individual original route | Platform, preserved nested path | Installed system folder; extension checked | Only frontend-supported game files |
| Switch update/DLC `.nsp` | ROM file category `update`/`dlc`, original route | `_library/<category>/<rom_id>/...` | Flat `switchupdates` | No |
| Other auxiliary categories | ROM file category, original route | `_library/<category>/<rom_id>/...` | Specific unsupported-destination diagnostic | No |
| Covers/screenshots | Catalogue resource paths | Not published | Local `.rommfs-artwork` and gamelist references | Media references only |
| RetroArch `.cht` cheats | ROM-file category `cheat`, original route | Retained `_library` auxiliary | Explicit materialization into effective `cheats/retroarch/<platform>/<relative>` | No |
| BIOS/keys/firmware | `GET /api/firmware`, `/api/firmware/{id}/content/{file_name}`; `firmware.read` | No external automatic placement | Explicit materialization: effective BIOS folder; Switch portable Eden/Citron keys or NAND registered directory | No |
| Save files | `/api/saves` inventory/upload/content; see below | Opt-in save-data folder | Opt-in effective saves folder or custom root | No |

Current save-client routes are `GET /api/saves?rom_id=&slot=`, multipart
`POST /api/saves?rom_id=&slot=&emulator=&overwrite=false&autocleanup=false`
with field `saveFile`, and `GET /api/saves/{save_id}/content?optimistic=false`.
Inventory returns numeric save/ROM/user IDs, filename/size, timestamps, emulator,
slot and missing-file status. See `crates/rommfs-core/src/romm/saves.rs`.

Save uploads sanitize/rewrite filenames, including a timestamp tag when `slot`
is supplied. A local journal is not enough to preserve remote relative paths.
Ordinary-file slots use `rommfs-file-v2:<canonical relative path>` (forward
slashes). RomM stores the slot unchanged, independently of sanitized/timestamped
filenames. The database limits slots to 255 characters; the client rejects
overlong/unsafe paths rather than truncating. Uploaded UUID `.bin` filenames
identify revisions, not destinations. Clean-client fixtures reconstruct nested,
companion, extensionless, empty and >1 MiB files from slots and content alone.
`GET /api/saves` without a game/slot filter returns the current user's saves,
enabling one account-wide discovery request rather than one request per game.
Per-file reconciliation still filters by game and slot. Legacy GB slots are
recognized for a single known GB filename; migration retains queued revisions
and baselines, but requires new broader-scope consent.

`save_target` and `save_target_layout` in the ROM catalogue supply file/folder
association rules. Unknown/split layouts and ambiguous title matches require
explicit local mapping. No save format is parsed. Source:
[save endpoints](https://github.com/rommapp/romm/blob/95599dadbe93c8f7b8a8647148fafbe293df3167/backend/endpoints/saves.py),
[slot limits](https://github.com/rommapp/romm/blob/95599dadbe93c8f7b8a8647148fafbe293df3167/backend/models/assets.py).

Firmware DTOs retain ID, platform ID, filename, byte size, missing flag and
update timestamp. Server `file_path` is not a trusted local destination.
Only unpacked Switch `.nca` files and key files are placed in portable emulator
folders; firmware archives and unknown cheat formats get explicit diagnostics.
Ownership/version publication never overwrites an unowned or modified file;
changed server auxiliary versions require explicit removal of the unchanged
owned local file before rematerialization. Firmware inventory failures do not
prevent independently supported cheats from being installed. Sources:
[firmware endpoints](https://github.com/rommapp/romm/blob/95599dadbe93c8f7b8a8647148fafbe293df3167/backend/endpoints/firmware.py),
[firmware schema](https://github.com/rommapp/romm/blob/95599dadbe93c8f7b8a8647148fafbe293df3167/backend/endpoints/responses/firmware.py),
[RetroArch cheat destination](https://github.com/RetroBat-Official/emulatorlauncher/blob/af2dc472fecfaadf851ecf0e92d1e4eb7c519b31/emulatorLauncher/Generators/LibRetro.Generator.cs).

## Library scan (test-harness only, not app scope)

Scans are triggered by the web UI via socket.io `scan` event; there is no REST
scan endpoint (`POST /api/tasks/run/{name}` rejects scan tasks). Harness recipe:

1. `POST /api/login` with HTTP Basic credentials → sets `romm_session` cookie.
2. socket.io connect to `/ws/socket.io` (websocket transport) with the cookie.
3. `emit('scan', {platforms: [], type: 'quick'})`, await `scan:done` event.

First admin user: `POST /api/users` `{username,email,password,role:"admin"}`
works unauthenticated while no users exist.

Library layout inside container: `/romm/library/roms/<platform_fs_slug>/<file>`
(and `/romm/library/bios/`). Platform `fs_slug` for nes/snes/gb is the
directory name (e.g. `nes`, `snes`, `gb`).
