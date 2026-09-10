#!/usr/bin/env python3
"""Windows Companion — Hub POSTs here at /command.

Binds 0.0.0.0 by default so Hub on the LAN can reach this PC. On start,
registers with Hub (`POST /api/v1/companions`) unless `--no-register`.
"""

from __future__ import annotations

import argparse
import ctypes
import json
import logging
import os
import platform
import socket
import subprocess
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any

BIND_HOST = "0.0.0.0"
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


def lan_ipv4() -> str:
    """Best-effort LAN IPv4 for Hub to call back (not 0.0.0.0)."""
    override = os.environ.get("COMPANION_ADVERTISE_HOST", "").strip()
    if override:
        return override
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        sock.connect(("8.8.8.8", 80))
        ip = sock.getsockname()[0]
        return ip or "127.0.0.1"
    except OSError:
        return "127.0.0.1"
    finally:
        sock.close()


def advertise_base_url(hub: str, port: int, override: str) -> str:
    """URL Hub should POST to. Local Hub → 127.0.0.1; otherwise this PC's LAN IP."""
    if override.strip():
        return override.strip()
    host = (urllib.parse.urlparse(hub).hostname or "").lower()
    if host in {"127.0.0.1", "localhost", "::1"}:
        return f"http://127.0.0.1:{port}"
    return f"http://{lan_ipv4()}:{port}"


def _json_request(
    method: str,
    url: str,
    body: dict[str, Any] | None = None,
    bearer: str | None = None,
    timeout: float = 5.0,
) -> tuple[int | None, dict[str, Any] | None]:
    """POST/GET JSON. Returns (status, obj) or (None, None) on network failure."""
    data = None if body is None else json.dumps(body).encode("utf-8")
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("Content-Type", "application/json; charset=utf-8")
    if bearer:
        req.add_header("Authorization", f"Bearer {bearer}")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            raw = resp.read().decode("utf-8") or "{}"
            parsed = json.loads(raw)
            payload = parsed if isinstance(parsed, dict) else {"raw": parsed}
            return int(resp.status), payload
    except urllib.error.HTTPError as exc:
        raw = exc.read().decode("utf-8", errors="replace")
        try:
            parsed = json.loads(raw) if raw else {}
        except json.JSONDecodeError:
            parsed = {"raw": raw}
        payload = parsed if isinstance(parsed, dict) else {"raw": parsed}
        return int(exc.code), payload
    except (OSError, urllib.error.URLError, TimeoutError, json.JSONDecodeError) as exc:
        log.warning("Hub request failed %s %s: %s", method, url, exc)
        return None, None


def pair_hub(hub: str) -> str | None:
    """POST /api/v1/auth/pair; return token or None."""
    status, data = _json_request("POST", f"{hub.rstrip('/')}/api/v1/auth/pair", {})
    if status and 200 <= status < 300 and isinstance(data, dict):
        token = data.get("token")
        if isinstance(token, str) and token.strip():
            return token.strip()
    return None


def register_with_hub(
    *,
    hub: str,
    companion_id: str,
    name: str,
    base_url: str,
    kind: str,
    pair: bool,
) -> bool:
    """Register this listener on Hub. Warn and continue if Hub is down."""
    token: str | None = None
    if pair:
        token = pair_hub(hub)
        if not token:
            log.warning("pair failed; registering without Bearer (demo AUTH_REQUIRED=false)")

    body = {
        "id": companion_id,
        "name": name,
        "base_url": base_url,
        "kind": kind,
    }
    url = f"{hub.rstrip('/')}/api/v1/companions"
    status, data = _json_request("POST", url, body, bearer=token)
    if status == 401 and not token:
        token = pair_hub(hub)
        if token:
            status, data = _json_request("POST", url, body, bearer=token)
    if status and 200 <= status < 300:
        log.info("registered on Hub as %s → %s", companion_id, base_url)
        return True
    log.warning(
        "Hub register failed (status=%s body=%s). Listener still up; register from UI or retry.",
        status,
        data,
    )
    return False


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
            "note": "Windows cannot unlock the session without credentials; Hub UI has no Unlock",
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


def open_firewall(port: int) -> bool:
    """netsh inbound allow for TCP port. Needs an elevated shell; no-op on non-Windows."""
    if os.name != "nt":
        log.warning("--open-firewall is Windows-only")
        return False
    cmd = [
        "netsh",
        "advfirewall",
        "firewall",
        "add",
        "rule",
        "name=LanIoT Companion",
        "dir=in",
        "action=allow",
        "protocol=TCP",
        f"localport={port}",
    ]
    try:
        completed = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=15,
            check=False,
        )
    except OSError as exc:
        log.warning("firewall netsh error: %s", exc)
        return False
    if completed.returncode == 0:
        log.info("firewall: inbound TCP %s allowed (LanIoT Companion)", port)
        return True
    detail = (completed.stderr or completed.stdout or "").strip()
    log.warning("firewall rule failed (run as Administrator): %s", detail)
    return False


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    """CLI: Hub URL, companion id, bind host/port, optional skip-register."""
    parser = argparse.ArgumentParser(description="LanIoT Windows Companion")
    parser.add_argument(
        "--hub",
        default=os.environ.get("HUB_URL", "http://127.0.0.1:3000"),
        help="Hub base URL (env HUB_URL)",
    )
    parser.add_argument(
        "--id",
        default=os.environ.get("COMPANION_ID", "companion.demo_pc"),
        help="Companion id registered on Hub (env COMPANION_ID)",
    )
    parser.add_argument(
        "--name",
        default=os.environ.get("COMPANION_NAME", "Demo PC"),
        help="Display name (env COMPANION_NAME)",
    )
    parser.add_argument(
        "--kind",
        default=os.environ.get("COMPANION_KIND", "pc"),
        help="Companion kind (env COMPANION_KIND)",
    )
    parser.add_argument(
        "--host",
        default=os.environ.get("COMPANION_BIND_HOST", BIND_HOST),
        help="Bind address (default 0.0.0.0 so Hub on LAN can connect)",
    )
    parser.add_argument(
        "--port",
        type=int,
        default=int(os.environ.get("COMPANION_PORT", str(PORT))),
        help="Listen port (env COMPANION_PORT)",
    )
    parser.add_argument(
        "--base-url",
        default=os.environ.get("COMPANION_BASE_URL", ""),
        help="URL Hub should POST to (local Hub → http://127.0.0.1:<port>)",
    )
    parser.add_argument(
        "--no-register",
        action="store_true",
        help="Do not POST /api/v1/companions on start",
    )
    parser.add_argument(
        "--pair",
        action="store_true",
        help="POST /api/v1/auth/pair before register (needed if AUTH_REQUIRED=true)",
    )
    parser.add_argument(
        "--open-firewall",
        action="store_true",
        help="Add a Windows inbound allow rule for the listen port (needs Administrator)",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> None:
    """Start the ThreadingHTTPServer Companion listener, then register on Hub."""
    global BIND_HOST, PORT  # pylint: disable=global-statement
    args = parse_args(argv)
    BIND_HOST = args.host
    PORT = args.port

    advertise = advertise_base_url(args.hub, PORT, args.base_url)
    server = ThreadingHTTPServer((BIND_HOST, PORT), CompanionHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True, name="companion-http")
    thread.start()
    log.info("Companion Windows listening on http://%s:%s/command", BIND_HOST, PORT)
    log.info("Advertise base_url=%s", advertise)
    log.info("Supported: ping | notify | lock | unlock | echo")
    log.info(
        "If Hub cannot reach this PC: allow inbound TCP %s, or pass --open-firewall "
        "(Administrator). No tray app — leave this console running.",
        PORT,
    )
    if "127.0.0.1" in advertise or "localhost" in advertise:
        log.info(
            "Hub in Docker cannot call 127.0.0.1 on the host — pass "
            "--base-url http://host.docker.internal:%s (or this PC's LAN IP)",
            PORT,
        )
    if args.open_firewall:
        open_firewall(PORT)

    if not args.no_register:
        register_with_hub(
            hub=args.hub,
            companion_id=args.id,
            name=args.name,
            base_url=advertise,
            kind=args.kind,
            pair=bool(args.pair),
        )

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
