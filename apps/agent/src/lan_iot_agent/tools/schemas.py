"""OpenAI-compatible Hub MCP tool schemas for LiteLLM tool-calling.

Mirrors Hub ``/mcp/tools`` catalog (apps/hub/src/mcp/mod.rs) so the LLM only
sees skills that Agent can execute via Hub MCP — never HA directly.
"""

from __future__ import annotations

import asyncio
import logging
import os
import time
from typing import Any

logger = logging.getLogger(__name__)

_HUB_TOOLS_TTL_SECS = 60.0
_HUB_TOOLS_CACHE: list[dict[str, Any]] | None = None
_HUB_TOOLS_AT = 0.0
_hub_tools_lock = asyncio.Lock()

# JSON Schema fragments (OpenAI ``function.parameters``).
_DEVICES_LIST_PARAMS: dict[str, Any] = {
    "type": "object",
    "properties": {
        "type": {
            "type": "string",
            "description": "Optional domain filter: light|climate|switch|sensor|…",
        },
        "room": {
            "type": "string",
            "description": "Optional substring match on friendly_name / room",
        },
    },
    "additionalProperties": False,
}

_DEVICES_GET_STATE_PARAMS: dict[str, Any] = {
    "type": "object",
    "required": ["entity_id"],
    "properties": {
        "entity_id": {
            "type": "string",
            "description": "Exact HA/Hub entity id, e.g. light.faker_esp32_light",
        },
    },
    "additionalProperties": False,
}

_DEVICES_CONTROL_PARAMS: dict[str, Any] = {
    "type": "object",
    "required": ["entity_id", "action"],
    "properties": {
        "entity_id": {
            "type": "string",
            "description": "Exact entity id from devices.list — never invent",
        },
        "action": {
            "type": "string",
            "description": "HA-style action: turn_on, turn_off, toggle, set_hvac_mode, …",
        },
        "params": {
            "type": "object",
            "description": "Optional action params (e.g. {\"mode\": \"cool\"})",
            "additionalProperties": True,
        },
    },
    "additionalProperties": False,
}

_DEVICES_DESCRIBE_PARAMS: dict[str, Any] = {
    "type": "object",
    "required": ["entity_id"],
    "properties": {
        "entity_id": {
            "type": "string",
            "description": "Exact entity id — returns capabilities and allowed actions/params",
        },
    },
    "additionalProperties": False,
}

_CLIMATE_SET_PARAMS: dict[str, Any] = {
    "type": "object",
    "required": ["entity_id", "temperature"],
    "properties": {
        "entity_id": {
            "type": "string",
            "description": "Climate entity id, e.g. climate.faker_gree_ac",
        },
        "temperature": {
            "type": "number",
            "description": "Target temperature in Celsius",
        },
        "mode": {
            "type": "string",
            "description": "Optional HVAC mode: cool, heat, auto, off, …",
        },
    },
    "additionalProperties": False,
}

_LIGHTS_CONTROL_PARAMS: dict[str, Any] = {
    "type": "object",
    "required": ["entity_id", "on"],
    "properties": {
        "entity_id": {
            "type": "string",
            "description": "Light entity id, e.g. light.faker_esp32_light",
        },
        "on": {
            "type": "boolean",
            "description": "true = turn on, false = turn off",
        },
        "brightness": {
            "type": "integer",
            "minimum": 0,
            "maximum": 255,
            "description": "Optional brightness 0–255",
        },
    },
    "additionalProperties": False,
}

_SCENES_RUN_PARAMS: dict[str, Any] = {
    "type": "object",
    "required": ["scene_id"],
    "properties": {
        "scene_id": {
            "type": "string",
            "description": "Hub scene id, e.g. sleep_mode or away_mode",
        },
    },
    "additionalProperties": False,
}

_COMPANION_COMMAND_PARAMS: dict[str, Any] = {
    "type": "object",
    "required": ["device_id", "command"],
    "properties": {
        "device_id": {
            "type": "string",
            "description": "Companion id, e.g. companion.demo_pc",
        },
        "command": {
            "type": "string",
            "description": "Command forwarded to Companion (ping, notify, lock, …)",
        },
        "title": {
            "type": "string",
            "description": "Optional notify title",
        },
        "body": {
            "type": "string",
            "description": "Optional notify body / message",
        },
    },
    "additionalProperties": False,
}

# Catalog entries: name → (description, parameters schema)
_HUB_TOOL_DEFS: tuple[tuple[str, str, dict[str, Any]], ...] = (
    (
        "devices.list",
        "List devices from the Hub registry. Call this first when entity_ids "
        "are unknown or the user asks what devices exist.",
        _DEVICES_LIST_PARAMS,
    ),
    (
        "devices.get_state",
        "Get current state of one device by exact entity_id.",
        _DEVICES_GET_STATE_PARAMS,
    ),
    (
        "devices.describe",
        "Describe a device's capabilities and allowed actions/params before controlling it.",
        _DEVICES_DESCRIBE_PARAMS,
    ),
    (
        "devices.control",
        "Control a device (turn_on, turn_off, toggle, set_hvac_mode, …).",
        _DEVICES_CONTROL_PARAMS,
    ),
    (
        "climate.set",
        "Set climate / AC target temperature (and optional HVAC mode).",
        _CLIMATE_SET_PARAMS,
    ),
    (
        "lights.control",
        "Turn a light on or off with optional brightness.",
        _LIGHTS_CONTROL_PARAMS,
    ),
    (
        "scenes.run",
        "Run a Hub scene by id (sleep_mode, away_mode, …). Prefer this for "
        "sleep/away/场景 requests.",
        _SCENES_RUN_PARAMS,
    ),
    (
        "companion.command",
        "Send a command to a Companion device (phone/PC) via Hub.",
        _COMPANION_COMMAND_PARAMS,
    ),
)


def _openai_tool(name: str, description: str, parameters: dict[str, Any]) -> dict[str, Any]:
    """One OpenAI/LiteLLM function-tool descriptor."""
    return {
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters,
        },
    }


def _static_tool_schemas() -> list[dict[str, Any]]:
    """Built-in catalog — used when Hub is down or GET /mcp/tools fails."""
    return [
        _openai_tool(name, description, parameters)
        for name, description, parameters in _HUB_TOOL_DEFS
    ]


def tools_from_hub_catalog(payload: Any) -> list[dict[str, Any]]:
    """Convert Hub ``GET /mcp/tools`` (or JSON-RPC tools/list) into OpenAI tools."""
    raw: Any = payload
    if isinstance(payload, dict):
        if isinstance(payload.get("tools"), list):
            raw = payload["tools"]
        elif isinstance(payload.get("result"), dict) and isinstance(
            payload["result"].get("tools"), list
        ):
            raw = payload["result"]["tools"]
        elif isinstance(payload.get("data"), dict) and isinstance(
            payload["data"].get("tools"), list
        ):
            raw = payload["data"]["tools"]
    if not isinstance(raw, list):
        return []
    out: list[dict[str, Any]] = []
    for item in raw:
        if not isinstance(item, dict):
            continue
        name = str(item.get("name") or "").strip()
        if not name:
            continue
        description = str(item.get("description") or name)
        parameters = (
            item.get("inputSchema")
            or item.get("input_schema")
            or item.get("parameters")
            or {"type": "object", "properties": {}}
        )
        if not isinstance(parameters, dict):
            parameters = {"type": "object", "properties": {}}
        out.append(_openai_tool(name, description, parameters))
    return out


def hub_tool_schemas() -> list[dict[str, Any]]:
    """OpenAI / LiteLLM ``tools`` list (Hub catalog when refreshed, else static)."""
    return list(_HUB_TOOLS_CACHE or _static_tool_schemas())


def hub_tool_names() -> frozenset[str]:
    """Frozen set of static Hub MCP tool names (fallback allow-list)."""
    return frozenset(name for name, _, _ in _HUB_TOOL_DEFS)


def allowed_llm_tools() -> frozenset[str]:
    """Tool names the LLM may invoke — live Hub catalog if cached."""
    return frozenset(t["function"]["name"] for t in hub_tool_schemas())


def refresh_enabled() -> bool:
    """False when ``HUB_TOOL_CATALOG_REFRESH`` is off (tests, air-gapped)."""
    raw = os.environ.get("HUB_TOOL_CATALOG_REFRESH")
    if raw is None:
        return True
    return raw.strip().lower() not in {"0", "false", "no", "off"}


def reset_hub_tool_cache() -> None:
    """Drop the cached catalog (test isolation; next refresh re-fetches)."""
    global _HUB_TOOLS_CACHE, _HUB_TOOLS_AT
    _HUB_TOOLS_CACHE = None
    _HUB_TOOLS_AT = 0.0


async def refresh_hub_tool_schemas(*, force: bool = False) -> list[dict[str, Any]]:
    """GET Hub /mcp/tools and cache as OpenAI tools. Static fallback on failure."""
    global _HUB_TOOLS_CACHE, _HUB_TOOLS_AT
    now = time.monotonic()
    if (
        not force
        and _HUB_TOOLS_CACHE is not None
        and (now - _HUB_TOOLS_AT) < _HUB_TOOLS_TTL_SECS
    ):
        return list(_HUB_TOOLS_CACHE)
    async with _hub_tools_lock:
        now = time.monotonic()
        if (
            not force
            and _HUB_TOOLS_CACHE is not None
            and (now - _HUB_TOOLS_AT) < _HUB_TOOLS_TTL_SECS
        ):
            return list(_HUB_TOOLS_CACHE)
        converted: list[dict[str, Any]] = []
        if refresh_enabled():
            try:
                import httpx
                from lan_iot_agent.settings import get_settings

                base = str(get_settings().hub.mcp_url).rstrip("/")
                url = f"{base}/tools"
                # trust_env=False: Hub is on the LAN/loopback. httpx would
                # otherwise take the Windows registry proxy (urllib
                # getproxies) and 502 every call.
                async with httpx.AsyncClient(timeout=2.0, trust_env=False) as client:
                    response = await client.get(url)
                if response.status_code == 200:
                    converted = tools_from_hub_catalog(response.json())
            except Exception as exc:  # noqa: BLE001 — Hub down is normal offline
                logger.debug("Hub tool catalog refresh failed: %s", exc)
        if converted:
            _HUB_TOOLS_CACHE = converted
            _HUB_TOOLS_AT = now
            logger.info(
                "Hub MCP tool catalog (%d): %s",
                len(converted),
                ", ".join(sorted(t["function"]["name"] for t in converted)),
            )
            return list(converted)
        # Cache the static catalog too, so a down Hub costs one GET per TTL
        # instead of one per turn (refresh runs inside build_context).
        _HUB_TOOLS_CACHE = _static_tool_schemas()
        _HUB_TOOLS_AT = now
        return list(_HUB_TOOLS_CACHE)


# Allowed MCP tools the LLM may invoke (static fallback; live set via allowed_llm_tools()).
ALLOWED_LLM_TOOLS = hub_tool_names()
