"""Hub MCP HTTP client — JSON-RPC + /mcp/call convenience.

Contract (Hub):
  POST {mcp_url}           JSON-RPC tools/call | tools/list | ping
  POST {mcp_url}/call      {"name","arguments"}
  GET  {mcp_url}/tools     tool catalog

Env: HUB_MCP_URL or MCP_URL (default http://127.0.0.1:3000/mcp).
"""

from __future__ import annotations

import asyncio
import itertools
import logging
from dataclasses import dataclass, field
from typing import Any

import httpx

from lan_iot_agent.settings import HubSettings, get_settings
from lan_iot_agent.tools.hub import HubCallResult, extract_device_list

logger = logging.getLogger(__name__)

KNOWN_TOOLS = (
    "devices.list",
    "devices.get_state",
    "devices.control",
    "climate.set",
    "lights.control",
    "scenes.run",
    "companion.command",
)

_rpc_id = itertools.count(1)


def _normalize_mcp_base(url: str) -> str:
    return url.rstrip("/")


@dataclass
class McpClient:
    """httpx client for Hub MCP with retries."""

    settings: HubSettings = field(default_factory=lambda: get_settings().hub)

    @property
    def mcp_url(self) -> str:
        """Normalized Hub MCP base URL (no trailing slash)."""
        return _normalize_mcp_base(self.settings.mcp_url)

    def _call_url(self) -> str:
        return f"{self.mcp_url}/call"

    def _tools_url(self) -> str:
        return f"{self.mcp_url}/tools"

    async def _post_json(
        self,
        url: str,
        payload: dict[str, Any],
        *,
        accept_bad_request: bool = False,
    ) -> HubCallResult:
        timeout = self.settings.timeout_seconds
        attempts = max(1, self.settings.max_retries)
        backoff = self.settings.retry_backoff_seconds
        last_error: str | None = None

        async with httpx.AsyncClient(timeout=timeout) as client:
            for attempt in range(1, attempts + 1):
                try:
                    response = await client.post(url, json=payload)
                    if response.status_code >= 500 and attempt < attempts:
                        last_error = f"HTTP {response.status_code}"
                        await asyncio.sleep(backoff * attempt)
                        continue
                    if response.status_code >= 400 and not (
                        accept_bad_request and response.status_code == 400
                    ):
                        body = response.text[:500]
                        return HubCallResult(
                            ok=False,
                            status_code=response.status_code,
                            error=f"MCP POST {url} → HTTP {response.status_code}: {body}",
                        )
                    try:
                        data = response.json()
                    except Exception:  # noqa: BLE001
                        data = {"raw": response.text}
                    return HubCallResult(
                        ok=response.status_code < 400,
                        status_code=response.status_code,
                        data=data,
                        error=None
                        if response.status_code < 400
                        else self._error_from_body(data),
                    )
                except (httpx.TimeoutException, httpx.TransportError) as exc:
                    last_error = str(exc)
                    logger.info(
                        "MCP POST %s attempt %s/%s failed: %s",
                        url,
                        attempt,
                        attempts,
                        exc,
                    )
                    if attempt < attempts:
                        await asyncio.sleep(backoff * attempt)

        return HubCallResult(
            ok=False,
            error=f"Hub MCP unreachable at {url}: {last_error or 'unknown error'}",
        )

    async def _get_json(self, url: str) -> HubCallResult:
        timeout = self.settings.timeout_seconds
        attempts = max(1, self.settings.max_retries)
        backoff = self.settings.retry_backoff_seconds
        last_error: str | None = None

        async with httpx.AsyncClient(timeout=timeout) as client:
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
                            error=f"MCP GET {url} → HTTP {response.status_code}",
                        )
                    return HubCallResult(
                        ok=True,
                        status_code=response.status_code,
                        data=response.json(),
                    )
                except (httpx.TimeoutException, httpx.TransportError) as exc:
                    last_error = str(exc)
                    if attempt < attempts:
                        await asyncio.sleep(backoff * attempt)

        return HubCallResult(
            ok=False,
            error=f"Hub MCP unreachable at {url}: {last_error or 'unknown error'}",
        )

    @staticmethod
    def _error_from_body(data: Any) -> str:
        if isinstance(data, dict):
            if err := data.get("error"):
                if isinstance(err, dict):
                    return str(err.get("message") or err)
                return str(err)
            if msg := data.get("message"):
                return str(msg)
        return "MCP call failed"

    async def list_tools(self) -> HubCallResult:
        """GET /mcp/tools — fall back to JSON-RPC tools/list."""
        result = await self._get_json(self._tools_url())
        if result.ok:
            return result

        rpc = await self._post_json(
            self.mcp_url,
            {
                "jsonrpc": "2.0",
                "id": next(_rpc_id),
                "method": "tools/list",
            },
        )
        if not rpc.ok or not isinstance(rpc.data, dict):
            return HubCallResult(
                ok=False,
                error=rpc.error or result.error or "tools/list failed",
                status_code=rpc.status_code or result.status_code,
            )
        if rpc.data.get("error"):
            return HubCallResult(
                ok=False,
                status_code=rpc.status_code,
                data=rpc.data,
                error=self._error_from_body(rpc.data),
            )
        return HubCallResult(
            ok=True,
            status_code=rpc.status_code,
            data=rpc.data.get("result") or rpc.data,
        )

    async def call_tool(
        self,
        name: str,
        arguments: dict[str, Any] | None = None,
    ) -> HubCallResult:
        """Call a Hub MCP tool via POST /mcp/call, then JSON-RPC tools/call."""
        arguments = dict(arguments or {})
        name = name.strip()
        logger.debug("MCP call_tool name=%s args=%s url=%s", name, arguments, self.mcp_url)

        # Preferred convenience endpoint.
        convenience = await self._post_json(
            self._call_url(),
            {"name": name, "arguments": arguments},
            accept_bad_request=True,
        )
        if convenience.ok and isinstance(convenience.data, dict):
            if convenience.data.get("ok") is False:
                return HubCallResult(
                    ok=False,
                    status_code=convenience.status_code,
                    data=convenience.data,
                    error=str(
                        convenience.data.get("error")
                        or self._error_from_body(convenience.data)
                    ),
                )
            return HubCallResult(
                ok=True,
                status_code=convenience.status_code,
                data=convenience.data,
            )

        # JSON-RPC tools/call on base /mcp
        rpc = await self._post_json(
            self.mcp_url,
            {
                "jsonrpc": "2.0",
                "id": next(_rpc_id),
                "method": "tools/call",
                "params": {"name": name, "arguments": arguments},
            },
        )
        if not rpc.ok or not isinstance(rpc.data, dict):
            # Prefer the more specific convenience error when both failed.
            return HubCallResult(
                ok=False,
                status_code=rpc.status_code or convenience.status_code,
                data=rpc.data or convenience.data,
                error=rpc.error
                or convenience.error
                or f"MCP tool '{name}' failed",
            )

        if rpc.data.get("error"):
            return HubCallResult(
                ok=False,
                status_code=rpc.status_code,
                data=rpc.data,
                error=self._error_from_body(rpc.data),
            )

        result_payload = rpc.data.get("result")
        return HubCallResult(
            ok=True,
            status_code=rpc.status_code,
            data=result_payload if result_payload is not None else rpc.data,
        )

    async def list_devices(
        self,
        *,
        type_filter: str | None = None,
        room: str | None = None,
    ) -> HubCallResult:
        """Call Hub MCP ``devices.list`` and normalize the device array."""
        args: dict[str, Any] = {}
        if type_filter:
            args["type"] = type_filter
        if room:
            args["room"] = room
        result = await self.call_tool("devices.list", args)
        if not result.ok:
            return result
        devices = extract_devices_from_mcp(result.data)
        return HubCallResult(
            ok=True,
            status_code=result.status_code,
            data={
                "devices": devices,
                "source": "hub_mcp",
                "raw": result.data,
            },
        )

    async def run_scene(self, scene_id: str) -> HubCallResult:
        """MCP scenes.run — { scene_id }."""
        return await self.call_tool("scenes.run", {"scene_id": scene_id})


def extract_devices_from_mcp(payload: Any) -> list[dict[str, Any]]:
    """Normalize MCP devices.list result into a list of device dicts."""
    if payload is None:
        return []
    if isinstance(payload, dict):
        # /mcp/call → {ok, tool, data: {devices: [...]}}
        data = payload.get("data")
        if isinstance(data, dict) and "devices" in data:
            return extract_device_list(data)
        if "devices" in payload:
            return extract_device_list(payload)
        # JSON-RPC result already unwrapped to {ok, tool, data}
        if isinstance(data, list):
            return extract_device_list(data)
    return extract_device_list(payload)


# Back-compat alias used by earlier stubs.
McpStubClient = McpClient
