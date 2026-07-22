#!/usr/bin/env python3
"""Windows Companion — Hub POSTs here at /command on port 9876.

Production-ready demo listener (stdlib only): health, ping, notify, lock stub.
Register on Hub as companion.demo_pc → http://127.0.0.1:9876
"""

from __future__ import annotations

import json
import logging
import platform
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any

HOST = "127.0.0.1"
PORT = 9876

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s %(levelname)s %(message)s",
)
log = logging.getLogger("companion.windows")

# Last notify / lock for Hub to inspect via ping response.
_STATE: dict[str, Any] = {
    "last_command": None,
    "last_notify": None,
    "locked": False,
    "started_at": time.time(),
}


def handle_command(command: str, payload: dict[str, Any]) -> dict[str, Any]:
    """Dispatch Companion commands. Unknown → accepted=false with hint."""
    cmd = (command or "").strip().lower()
    _STATE["last_command"] = cmd

    if cmd in ("", "ping", "health"):
        return {
            "ok": True,
            "accepted": True,
            "command": cmd or "ping",
            "service": "companion.windows",
            "host": platform.node(),
            "platform": platform.platform(),
            "uptime_secs": round(time.time() - float(_STATE["started_at"]), 1),
            "locked": bool(_STATE["locked"]),
            "last_notify": _STATE["last_notify"],
        }

    if cmd in ("notify", "notification", "toast"):
        meta_obj = payload.get("meta")
        meta: dict[str, Any] = meta_obj if isinstance(meta_obj, dict) else {}
        title = str(payload.get("title") or meta.get("title") or "LanIoT")
        body = str(
            payload.get("body") or payload.get("message") or meta.get("body") or ""
        )
        _STATE["last_notify"] = {"title": title, "body": body, "at": time.time()}
        log.info("NOTIFY %s — %s", title, body)
        return {
            "ok": True,
            "accepted": True,
            "command": "notify",
            "delivered": True,
            "title": title,
            "body": body,
        }

    if cmd in ("lock", "lock_screen"):
        _STATE["locked"] = True
        log.info("LOCK stub accepted (no OS lock in demo)")
        return {
            "ok": True,
            "accepted": True,
            "command": "lock",
            "locked": True,
            "note": "stub — replace with WinAPI LockWorkStation in production",
        }

    if cmd in ("unlock",):
        _STATE["locked"] = False
        return {"ok": True, "accepted": True, "command": "unlock", "locked": False}

    if cmd in ("echo",):
        return {"ok": True, "accepted": True, "command": "echo", "payload": payload}

    return {
        "ok": False,
        "accepted": False,
        "command": cmd,
        "error": "unknown_command",
        "supported": ["ping", "notify", "lock", "unlock", "echo"],
    }


class CompanionHandler(BaseHTTPRequestHandler):
    """HTTP handler for Companion health checks and Hub /command posts."""

    def log_message(self, format: str, *args: Any) -> None:  # pylint: disable=redefined-builtin
        """Route stdlib access logs through the companion logger."""
        log.info("%s - %s", self.address_string(), format % args)

    def _send_json(self, status: int, body: dict[str, Any]) -> None:
        """Write a JSON response with Content-Type and Content-Length."""
        raw = json.dumps(body).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def do_GET(self) -> None:  # pylint: disable=invalid-name
        """Serve health / root JSON; 404 for other paths."""
        if self.path.rstrip("/") in ("", "/", "/health"):
            self._send_json(
                200,
                {
                    "ok": True,
                    "service": "companion.windows",
                    "port": PORT,
                    "locked": bool(_STATE["locked"]),
                },
            )
            return
        self._send_json(404, {"ok": False, "error": "not_found"})

    def do_POST(self) -> None:  # pylint: disable=invalid-name
        """Accept Hub ``POST /command`` JSON bodies and dispatch commands."""
        path = self.path.split("?", 1)[0].rstrip("/") or "/"
        if path != "/command":
            self._send_json(404, {"ok": False, "error": "not_found"})
            return

        length = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(length) if length > 0 else b""
        try:
            payload = json.loads(raw.decode("utf-8") or "{}")
            if not isinstance(payload, dict):
                payload = {}
        except json.JSONDecodeError:
            self._send_json(400, {"ok": False, "error": "invalid_json"})
            return

        command = str(payload.get("command", ""))
        log.info("command received: %r", command)
        result = handle_command(command, payload)
        status = 200 if result.get("ok") else 400
        self._send_json(status, result)


def main() -> None:
    """Start the ThreadingHTTPServer Companion listener on HOST:PORT."""
    server = ThreadingHTTPServer((HOST, PORT), CompanionHandler)
    log.info("Companion Windows listening on http://%s:%s/command", HOST, PORT)
    log.info("Supported: ping | notify | lock | unlock | echo")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        log.info("shutting down")
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
