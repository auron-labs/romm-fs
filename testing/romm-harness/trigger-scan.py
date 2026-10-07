"""Trigger a library scan on the local RomM harness over its socket.io API.

RomM 5.x has no REST scan endpoint; scans are emitted as socket.io events on
/ws/socket.io. This logs in (HTTP Basic), opens a websocket, and emits
`scan` with `{"platforms": [], "type": "quick"}` — the same event the web UI
sends. Run after `docker compose up -d` and first-user creation:

    pip install websocket-client
    python trigger-scan.py

Env overrides: ROMM_URL (default http://127.0.0.1:8080), ROMM_USER,
ROMM_PASS (harness-local admin/admin123 — never reuse elsewhere).
"""
import os
import sys
import time
import urllib.request
import http.cookiejar

import websocket

BASE = os.environ.get("ROMM_URL", "http://127.0.0.1:8080").rstrip("/")
USER = os.environ.get("ROMM_USER", "admin")
PASS = os.environ.get("ROMM_PASS", "admin123")


def session_cookie() -> str:
    jar = http.cookiejar.CookieJar()
    opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))
    import base64

    req = urllib.request.Request(f"{BASE}/api/login", method="POST")
    req.add_header("Authorization", "Basic " + base64.b64encode(f"{USER}:{PASS}".encode()).decode())
    opener.open(req)
    for cookie in jar:
        if cookie.name == "romm_session":
            return cookie.value
    sys.exit("login did not set romm_session — check credentials")


def main() -> None:
    ws_url = f"{BASE.replace('http', 'ws', 1)}/ws/socket.io/?EIO=4&transport=websocket"
    ws = websocket.create_connection(
        ws_url,
        header=[f"Cookie: romm_session={session_cookie()}"],
        timeout=15,
    )
    ws.recv()  # engine.io open packet
    ws.send("40")  # socket.io namespace connect
    ws.recv()
    ws.send('42["scan",{"platforms":[],"type":"quick"}]')
    deadline = time.time() + 120
    while time.time() < deadline:
        try:
            frame = ws.recv()
        except Exception as exc:  # noqa: BLE001
            sys.exit(f"scan did not finish cleanly: {exc}")
        if frame.startswith("2"):
            ws.send("3")
            continue
        if '"scan:done"' in frame:
            print(frame)
            ws.close()
            return
    sys.exit("timed out waiting for scan:done")


if __name__ == "__main__":
    main()
