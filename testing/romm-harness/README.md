# Local RomM test harness

Deterministic, self-contained RomM 5.x server for manual and end-to-end
testing of RomMFS. Requires Docker (Linux containers).

```bash
cd testing/romm-harness
docker compose up -d
# wait for http://localhost:8080 to respond, then create the first admin user:
curl -X POST http://localhost:8080/api/users \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","email":"admin@example.com","password":"admin123","role":"admin"}'
```

The compose file mounts `./library` at `/romm/library`, so the placeholder
ROMs under `library/roms/{nes,snes,gb}/` are picked up by RomM's library
scan. Trigger a scan over the socket.io API (no REST endpoint exists):

```bash
# 1) log in (HTTP Basic) -> romm_session cookie
curl -c cookies.txt -X POST http://localhost:8080/api/login -u admin:admin123
# 2) open a socket.io websocket to /ws/socket.io with that Cookie and
#    emit event 'scan' with payload {"platforms":[],"type":"quick"}
```

After the scan: `GET /api/platforms` lists nes/snes/gb; `GET /api/roms`
lists the three single-file ROMs. Credentials here are local-only test
values — never reuse them elsewhere.
