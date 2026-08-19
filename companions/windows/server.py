#!/usr/bin/env python3
"""Windows Companion — Hub POSTs here at /command on port 9876.

Demo listener (stdlib only): health, ping, toast/notify, LockWorkStation.
Register on Hub as companion.demo_pc → http://127.0.0.1:9876
"""

from __future__ import annotations

import ctypes
import json
import logging
import os
import platform
import subprocess
import threading
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


def _dry_run() -> bool:
    """Skip OS side effects when COMPANION_DRY_RUN is truthy (tests)."""
    return os.environ.get("COMPANION_DRY_RUN", "").strip().lower() in {
        "1",
        "true",
        "yes",
        "on",
    }


def _show_message_box(title: str, body: str) -> None:
    """Blocking MessageBox on a daemon thread (always available on Windows)."""
    ctypes.windll.user32.MessageBoxW(0, body or title, title, 0x00000040)


def deliver_notify(title: str, body: str) -> dict[str, Any]:
    """Show a Windows toast when possible; otherwise a MessageBox."""
    if _dry_run():
        return {"delivered": True, "via": "dry_run"}
    if os.name != "nt":
        return {"delivered": False, "error": "windows_only"}

    # Toast via PowerShell / WinRT; fall back to MessageBox so notify is never a silent stub.
    ps = (
        "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, "
        "ContentType = WindowsRuntime] | Out-Null; "
        "$xml = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent("
        "[Windows.UI.Notifications.ToastTemplateType]::ToastText02); "
        "$nodes = $xml.GetElementsByTagName('text'); "
        f"$nodes.Item(0).AppendChild($xml.CreateTextNode({json.dumps(title)}))"
        " | Out-Null; "
        f"$nodes.Item(1).AppendChild($xml.CreateTextNode({json.dumps(body)}))"
        " | Out-Null; "
        "$toast = [Windows.UI.Notifications.ToastNotification]::new($xml); "
        "[Windows.UI.Notifications.ToastNotificationManager]::"
        "CreateToastNotifier('LanIoT').Show($toast)"
    )
    try:
        completed = subprocess.run(
            ["powershell", "-NoProfile", "-NonInteractive", "-Command", ps],
            capture_output=True,
            text=True,
            timeout=8,
            check=False,
        )
        if completed.returncode == 0:
            return {"delivered": True, "via": "toast"}
        log.warning("toast powershell failed: %s", completed.stderr[-400:])
    except (OSError, subprocess.TimeoutExpired) as exc:
        log.warning("toast powershell error: %s", exc)

    threading.Thread(
        target=_show_message_box,
        args=(title, body),
        daemon=True,
        name="companion-notify",
    ).start()
    return {"delivered": True, "via": "message_box"}


def lock_workstation() -> dict[str, Any]:
    """Call user32.LockWorkStation. Unlock is not supported without credentials."""
    if _dry_run():
        return {"ok": True, "locked": True, "via": "dry_run"}
    if os.name != "nt":
        return {"ok": False, "error": "windows_only"}
    try:
        ok = bool(ctypes.windll.user32.LockWorkStation())
    except OSError as exc:
        return {"ok": False, "error": str(exc)}
    if not ok:
        return {"ok": False, "error": "LockWorkStation failed"}
    return {"ok": True, "locked": True, "via": "LockWorkStation"}


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
        delivery = deliver_notify(title, body)
        _STATE["last_notify"] = {
            "title": title,
            "body": body,
            "at": time.time(),
            "via": delivery.get("via"),
        }
        log.info("NOTIFY %s — %s via=%s", title, body, delivery.get("via"))
        accepted = bool(delivery.get("delivered"))
        result: dict[str, Any] = {
            "ok": accepted,
            "accepted": accepted,
            "command": "notify",
            "title": title,
            "body": body,
            **delivery,
        }
        if not accepted:
            result["error"] = str(delivery.get("error") or "notify_failed")
        return result

    if cmd in ("lock", "lock_screen"):
        locked = lock_workstation()
        if locked.get("ok"):
            _STATE["locked"] = True
            log.info("LOCK via %s", locked.get("via"))
            return {
                "ok": True,
                "accepted": True,
                "command": "lock",
                "locked": True,
                "via": locked.get("via"),
            }
        log.warning("LOCK failed: %s", locked.get("error"))
        return {
            "ok": False,
            "accepted": False,
            "command": "lock",
            "locked": bool(_STATE["locked"]),
            "error": str(locked.get("error") or "lock_failed"),
        }

    if cmd in ("unlock",):
        return {
            "ok": False,
            "accepted": False,
            "command": "unlock",
            "locked": bool(_STATE["locked"]),
            "error": "unlock_not_supported",
            "note": "Windows cannot unlock the session without credentials",
        }

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
