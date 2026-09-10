"""Thin HTTP client for Hub REST endpoints (planned Hub API)."""

from __future__ import annotations

import asyncio
import logging
from dataclasses import dataclass, field
from typing import Any

import httpx

from lan_iot_agent.settings import HubSettings, get_settings

logger = logging.getLogger(__name__)


@dataclass
class HubCallResult:
    """Normalized Hub HTTP / MCP call outcome."""

    ok: bool
    status_code: int | None = None
    data: Any = None
    error: str | None = None


@dataclass
class HubClient:
    """Thin Hub REST client (health / devices / scenes).

    MCP tools go through ``tools.mcp.McpClient``; REST is for context helpers.
    """

    settings: HubSettings = field(default_factory=lambda: get_settings().hub)

    def _base(self) -> str:
        return self.settings.url.rstrip("/")

    async def _get_with_retries(self, path: str) -> HubCallResult:
        url = f"{self._base()}{path}"
        timeout = self.settings.timeout_seconds
        attempts = max(1, self.settings.max_retries)
        backoff = self.settings.retry_backoff_seconds
        last_error: str | None = None

        # trust_env=False: Hub is on the LAN/loopback. httpx would otherwise take
        # the Windows registry proxy (urllib getproxies) and 502 every call.
        async with httpx.AsyncClient(timeout=timeout, trust_env=False) as client:
            for attempt in range(1, attempts + 1):
                try:
                    response = await client.get(url)
                    if response.status_code >= 500 and attempt < attempts:
                        last_error = f"HTTP {response.status_code}"
                        await asyncio.sleep(backoff * attempt)
                        continue
                    if response.status_code >= 400:
                        return HubCallResult(
                            ok=False,
                            status_code=response.status_code,
                            error=f"Hub {path} returned HTTP {response.status_code}",
                        )
                    data: Any
                    try:
                        data = response.json()
                    except Exception:  # noqa: BLE001
                        data = {"raw": response.text}
                    return HubCallResult(ok=True, status_code=response.status_code, data=data)
                except (httpx.TimeoutException, httpx.TransportError) as exc:
                    last_error = str(exc)
                    logger.info(
                        "Hub GET %s attempt %s/%s failed: %s",
                        path,
                        attempt,
                        attempts,
                        exc,
                    )
                    if attempt < attempts:
                        await asyncio.sleep(backoff * attempt)

        return HubCallResult(
            ok=False,
            error=f"Hub unreachable at {url}: {last_error or 'unknown error'}",
        )

    async def health(self) -> HubCallResult:
        """GET /api/v1/health."""
        return await self._get_with_retries("/api/v1/health")

    async def list_devices(self) -> HubCallResult:
        """GET /api/v1/devices — optional REST fallback for context."""
        return await self._get_with_retries("/api/v1/devices")

    async def list_scenes(self) -> HubCallResult:
        """GET /api/v1/scenes."""
        return await self._get_with_retries("/api/v1/scenes")


def extract_device_list(payload: Any) -> list[dict[str, Any]]:
    """Normalize Hub devices response into a list of dicts."""
    if payload is None:
        return []
    if isinstance(payload, list):
        return [d for d in payload if isinstance(d, dict)]
    if isinstance(payload, dict):
        for key in ("devices", "items", "data"):
            value = payload.get(key)
            if isinstance(value, list):
                return [d for d in value if isinstance(d, dict)]
        # Single device object
        if "entity_id" in payload or "id" in payload or "name" in payload:
            return [payload]
    return []


def extract_scene_ids(payload: Any) -> list[str]:
    """Normalize Hub scenes list into scene id strings."""
    if payload is None:
        return []
    rows: list[Any]
    if isinstance(payload, list):
        rows = payload
    elif isinstance(payload, dict):
        for key in ("scenes", "items", "data"):
            value = payload.get(key)
            if isinstance(value, list):
                rows = value
                break
        else:
            rows = []
    else:
        return []

    ids: list[str] = []
    for row in rows:
        if isinstance(row, str):
            ids.append(row)
        elif isinstance(row, dict):
            sid = row.get("id") or row.get("scene_id") or row.get("name")
            if sid:
                ids.append(str(sid))
    return ids
