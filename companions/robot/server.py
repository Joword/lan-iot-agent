#!/usr/bin/env python3
"""LAN robot listener — same Hub HTTP as Companion, for a robot already on the network.

This is a product path, not a vendor SDK. Hub scan + adopt + POST /command
stay the same; replace this process with the robot's own HTTP (or a thin
bridge) when the chassis is real.

Binds 0.0.0.0:9879. GET /health (`lan_iot: true`, `kind=robot`);
POST /command: ping | stop | dock | start.
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
log = logging.getLogger("lan.robot")

BIND_HOST = "0.0.0.0"
PORT = 9879
IDENTITY: dict[str, Any] = {
    "id": "robot.lan_demo",
    "name": "LAN robot",
    "kind": "robot",
}
_STATE: dict[str, Any] = {"mode": "idle", "started_at": time.time()}
SUPPORTED = ["ping", "stop", "dock", "start"]


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


def _ok(command: str) -> dict[str, Any]:
    return {
        "ok": True,
        "accepted": True,
        "command": command,
        "mode": str(_STATE["mode"]),
        "id": IDENTITY["id"],
    }


def handle_command(command: str) -> dict[str, Any]:
    """Dispatch robot commands. Unknown → accepted false. Stop always wins."""
    cmd = (command or "").strip().lower()
    if cmd in ("", "ping", "health"):
        return _ok(cmd or "ping")
    if cmd in ("stop", "halt", "estop"):
        _STATE["mode"] = "stopped"
        log.info("STOP")
        return _ok("stop")
    if cmd in ("dock", "home", "return"):
        _STATE["mode"] = "docking"
        log.info("DOCK")
        return _ok("dock")
    if cmd in ("start", "go", "clean"):
        _STATE["mode"] = "running"
        log.info("START")
        return _ok("start")
    return {
        "ok": False,
        "accepted": False,
        "command": cmd,
        "error": "unknown_command",
        "supported": SUPPORTED,
    }


class RobotHandler(BaseHTTPRequestHandler):
    """Health + command for a robot already on the LAN."""

    def log_message(self, format: str, *args: Any) -> None:  # pylint: disable=redefined-builtin
        """Route stdlib access logs through the robot logger."""
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
        """Identify this robot for Hub LAN scan (`lan_iot: true`)."""
        if self.path.rstrip("/") in ("", "/", "/health"):
            self._send_json(
                200,
                {
                    "ok": True,
                    "lan_iot": True,
                    "service": "lan-iot-robot",
                    "kind": IDENTITY["kind"],
                    "id": IDENTITY["id"],
                    "name": IDENTITY["name"],
                    "port": PORT,
                    "mode": str(_STATE["mode"]),
                    "commands": SUPPORTED,
                    "uptime_secs": round(time.time() - float(_STATE["started_at"]), 1),
                },
            )
            return
        self._send_json(404, {"ok": False, "error": "not_found"})

    def do_POST(self) -> None:  # pylint: disable=invalid-name
        """Accept Hub ``POST /command`` (ping / stop / dock / start)."""
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
    """Listen on 0.0.0.0 so the Hub can scan this robot on the LAN."""
    global PORT, BIND_HOST  # pylint: disable=global-statement
    parser = argparse.ArgumentParser(
        description="LanIoT LAN robot listener (already on the network)"
    )
    parser.add_argument("--host", default=os.environ.get("ROBOT_BIND_HOST", BIND_HOST))
    parser.add_argument(
        "--port", type=int, default=int(os.environ.get("ROBOT_PORT", str(PORT)))
    )
    parser.add_argument("--id", default=os.environ.get("ROBOT_ID", IDENTITY["id"]))
    parser.add_argument("--name", default=os.environ.get("ROBOT_NAME", IDENTITY["name"]))
    args = parser.parse_args()
    BIND_HOST = args.host
    PORT = args.port
    IDENTITY["id"] = args.id
    IDENTITY["name"] = args.name

    server = ThreadingHTTPServer((BIND_HOST, PORT), RobotHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    log.info("LAN robot %s listening http://%s:%s", IDENTITY["id"], BIND_HOST, PORT)
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
