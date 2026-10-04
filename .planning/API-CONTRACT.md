# Verified RomM API contract

Instance: local Docker `rommapp/romm:latest` pulled 2026-10-04.
**Tested version: RomM API 5.3.1** (from `/openapi.json` `info.version`; startup log confirms 5.3.1).

Auth token endpoint, response, and catalogue/download responses below were
exercised against the live instance and produced the documented shapes.

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

**Single-file rule (PoC):** a ROM is supported iff exactly one entry exists in
`files` (`files.len()==1`). `has_multiple_files==true` or zero/other counts →
skip, log, count as unsupported.

## Download

`GET /api/roms/{id}/content/{file_name}` (also supports HEAD)
→ 200 `application/octet-stream` body = original bytes, `Content-Length` set,
`Content-Disposition: attachment; filename*=UTF-8''<name>`.
404 JSON `{"detail": ...}` when missing. Verified byte-identical vs source file.

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
