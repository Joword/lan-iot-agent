"""OpenAI-compatible Hub MCP tool schemas for LiteLLM tool-calling.

Mirrors Hub ``/mcp/tools`` catalog (apps/hub/src/mcp/mod.rs) so the LLM only
sees skills that Agent can execute via Hub MCP — never HA directly.
"""

from __future__ import annotations

from typing import Any

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
            "description": "Exact HA/Hub entity id, e.g. light.demo_esp32_light",
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
            "description": "Climate entity id, e.g. climate.demo_gree_ac",
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
            "description": "Light entity id, e.g. light.demo_esp32_light",
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
            "description": "Command forwarded to Companion (ping, notify, …)",
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


def hub_tool_schemas() -> list[dict[str, Any]]:
    """OpenAI / LiteLLM ``tools`` list for chat completions."""
    tools: list[dict[str, Any]] = []
    for name, description, parameters in _HUB_TOOL_DEFS:
        tools.append(
            {
                "type": "function",
                "function": {
                    "name": name,
                    "description": description,
                    "parameters": parameters,
                },
            }
        )
    return tools


def hub_tool_names() -> frozenset[str]:
    """Frozen set of Hub MCP tool names exposed to the LLM."""
    return frozenset(name for name, _, _ in _HUB_TOOL_DEFS)


# Allowed MCP tools the LLM may invoke (excludes internal Agent helpers).
ALLOWED_LLM_TOOLS = hub_tool_names()
