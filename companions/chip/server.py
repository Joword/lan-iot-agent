#!/usr/bin/env python3
"""R&D chip hook — same Hub HTTP as Companion, for later firmware work.

This is not a household device class. It exists so future MCU / ESP
research can plug into Hub scan + adopt + `/command` without a new
northbound API. Swap this process for real silicon when that work starts.

Binds 0.0.0.0:9878. GET /health (`lan_iot: true`, `kind=chip`);
POST /command: ping | turn_on | turn_off | toggle.
"""

from __future__ import annotations

import argparse
import json
import logging
import os
import socket
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
log = logging.getLogger("lan.chip")

BIND_HOST = "0.0.0.0"
PORT = 9878
IDENTITY: dict[str, Any] = {
    "id": "chip.lan_esp32",
    "name": "R&D chip hook",
    "kind": "chip",
}
_STATE: dict[str, Any] = {"on": False, "started_at": time.time()}


def lan_ipv4() -> str:
    """Best-effort LAN IPv4 for logs (not 0.0.0.0)."""
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        sock.connect(("8.8.8.8", 80))
        return sock.getsockname()[0] or "127.0.0.1"
    except OSError:
        return "127.0.0.1"
    finally:
        sock.close()


def handle_command(command: str) -> dict[str, Any]:
    """Dispatch chip commands. Unknown → accepted false."""
    cmd = (command or "").strip().lower()
    if cmd in ("", "ping", "health"):
        return {
            "ok": True,
            "accepted": True,
            "command": cmd or "ping",
            "on": bool(_STATE["on"]),
            "id": IDENTITY["id"],
        }
    if cmd in ("turn_on", "on"):
        _STATE["on"] = True
        log.info("ON")
        return {"ok": True, "accepted": True, "command": "turn_on", "on": True}
    if cmd in ("turn_off", "off"):
        _STATE["on"] = False
        log.info("OFF")
        return {"ok": True, "accepted": True, "command": "turn_off", "on": False}
    if cmd == "toggle":
        _STATE["on"] = not bool(_STATE["on"])
        log.info("TOGGLE on=%s", _STATE["on"])
        return {
            "ok": True,
            "accepted": True,
            "command": "toggle",
            "on": bool(_STATE["on"]),
        }
    return {
        "ok": False,
        "accepted": False,
        "command": cmd,
        "error": "unknown_command",
        "supported": ["ping", "turn_on", "turn_off", "toggle"],
    }


class ChipHandler(BaseHTTPRequestHandler):
    """Health + command for a LAN MCU stub."""

    def log_message(self, format: str, *args: Any) -> None:  # pylint: disable=redefined-builtin
        """Route stdlib access logs through the chip logger."""
        log.info("%s - %s", self.address_string(), format % args)

    def _send_json(self, status: int, body: dict[str, Any]) -> None:  # pylint: disable=missing-function-docstring
        """Write a JSON HTTP body (Content-Type and Content-Length)."""
        raw = json.dumps(body).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def do_GET(self) -> None:  # pylint: disable=invalid-name,missing-function-docstring
        """Serve GET /health (and /) so Hub LAN scan can identify this chip."""
        if self.path.rstrip("/") in ("", "/", "/health"):
            self._send_json(
                200,
                {
                    "ok": True,
                    "lan_iot": True,
                    "service": "lan-iot-chip",
                    "kind": IDENTITY["kind"],
                    "id": IDENTITY["id"],
                    "name": IDENTITY["name"],
                    "port": PORT,
                    "on": bool(_STATE["on"]),
                    "commands": ["ping", "turn_on", "turn_off", "toggle"],
                    "uptime_secs": round(time.time() - float(_STATE["started_at"]), 1),
                },
            )
            return
        self._send_json(404, {"ok": False, "error": "not_found"})

    def do_POST(self) -> None:  # pylint: disable=invalid-name,missing-function-docstring
        """Serve POST /command from Hub (ping, on, off, toggle)."""
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
        result = handle_command(str(payload.get("command", "")))
        self._send_json(200 if result.get("ok") else 400, result)


def main() -> None:
    """Listen on 0.0.0.0 so the Hub can scan this chip on the LAN."""
    global PORT, BIND_HOST  # pylint: disable=global-statement
    parser = argparse.ArgumentParser(
        description="LanIoT R&D chip hook (not a home device)"
    )
    parser.add_argument("--host", default=os.environ.get("CHIP_BIND_HOST", BIND_HOST))
    parser.add_argument("--port", type=int, default=int(os.environ.get("CHIP_PORT", str(PORT))))
    parser.add_argument("--id", default=os.environ.get("CHIP_ID", IDENTITY["id"]))
    parser.add_argument("--name", default=os.environ.get("CHIP_NAME", IDENTITY["name"]))
    args = parser.parse_args()
    BIND_HOST = args.host
    PORT = args.port
    IDENTITY["id"] = args.id
    IDENTITY["name"] = args.name

    server = ThreadingHTTPServer((BIND_HOST, PORT), ChipHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    log.info("R&D chip hook %s listening http://%s:%s", IDENTITY["id"], BIND_HOST, PORT)
    log.info("Hub scan should see http://%s:%s/health", lan_ipv4(), PORT)
    try:
        while thread.is_alive():
            thread.join(timeout=0.5)
    except KeyboardInterrupt:
        log.info("shutting down")
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()
