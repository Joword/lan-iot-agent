"""Validate Hub control tools against ``devices.describe`` capabilities."""

from __future__ import annotations

from typing import Any

CONTROL_TOOLS = frozenset({"devices.control", "climate.set", "lights.control"})


def describe_payload(raw: Any) -> dict[str, Any] | None:
    """Unwrap Hub ``/mcp/call`` envelope to the describe document."""
    if not isinstance(raw, dict):
        return None
    inner = raw.get("data")
    if isinstance(inner, dict) and (
        "capabilities" in inner or "summary" in inner or "entity_id" in inner
    ):
        return inner
    if "capabilities" in raw or "summary" in raw:
        return raw
    return raw


def entity_id_of_tool(_name: str, arguments: dict[str, Any]) -> str | None:
    """Return the target entity_id for a control-shaped tool call."""
    raw = arguments.get("entity_id")
    if raw is None:
        return None
    entity_id = str(raw).strip()
    return entity_id or None


def planned_actions(name: str, arguments: dict[str, Any]) -> list[str]:
    """Map a Hub MCP tool call to the adapter action(s) it will perform."""
    if name == "devices.control":
        action = str(arguments.get("action") or "").strip()
        return [action] if action else []
    if name == "climate.set":
        actions = ["set_temperature"]
        if arguments.get("mode") not in (None, ""):
            actions.append("set_hvac_mode")
        return actions
    if name == "lights.control":
        on = arguments.get("on")
        if arguments.get("brightness") is not None and on is not False:
            return ["set_brightness"]
        if on is False:
            return ["turn_off"]
        return ["turn_on"]
    return []


def allowed_actions(describe: dict[str, Any] | None) -> set[str]:
    """Flatten action names from a ``devices.describe`` payload."""
    if not isinstance(describe, dict):
        return set()
    summary = describe.get("summary")
    if isinstance(summary, dict):
        raw = summary.get("actions")
        if isinstance(raw, list):
            return {str(a) for a in raw if str(a).strip()}
    names: set[str] = set()
    caps = describe.get("capabilities")
    if isinstance(caps, list):
        for cap in caps:
            if not isinstance(cap, dict):
                continue
            for action in cap.get("actions") or []:
                if isinstance(action, dict) and action.get("name"):
                    names.add(str(action["name"]))
    return names


def _param_specs(describe: dict[str, Any], action: str) -> list[dict[str, Any]]:
    caps = describe.get("capabilities")
    if not isinstance(caps, list):
        return []
    specs: list[dict[str, Any]] = []
    for cap in caps:
        if not isinstance(cap, dict):
            continue
        for item in cap.get("actions") or []:
            if isinstance(item, dict) and str(item.get("name") or "") == action:
                params = item.get("params") or []
                if isinstance(params, list):
                    specs.extend(p for p in params if isinstance(p, dict))
    return specs


def _params_for_action(name: str, action: str, arguments: dict[str, Any]) -> dict[str, Any]:
    if name == "devices.control":
        raw = arguments.get("params")
        return dict(raw) if isinstance(raw, dict) else {}
    if name == "climate.set" and action == "set_temperature":
        return {"temperature": arguments.get("temperature")}
    if name == "climate.set" and action == "set_hvac_mode":
        return {"mode": arguments.get("mode")}
    if name == "lights.control" and action == "set_brightness":
        return {"brightness": arguments.get("brightness")}
    return {}


def _as_float(value: Any) -> float | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, (int, float)):
        return float(value)
    if isinstance(value, str) and value.strip():
        try:
            return float(value)
        except ValueError:
            return None
    return None


def validate_against_describe(
    name: str,
    arguments: dict[str, Any],
    describe: dict[str, Any] | None,
) -> str | None:
    """Return an error string if the tool call is not allowed by capabilities."""
    if name not in CONTROL_TOOLS:
        return None
    entity_id = entity_id_of_tool(name, arguments)
    if not entity_id:
        return f"{name} requires entity_id"
    if not isinstance(describe, dict):
        return f"devices.describe failed for {entity_id}"
    allowed = allowed_actions(describe)
    if not allowed:
        return f"device {entity_id} has no controllable actions"
    for action in planned_actions(name, arguments):
        if action not in allowed:
            return (
                f"action '{action}' is not allowed on {entity_id} "
                f"(allowed: {sorted(allowed)})"
            )
        params = _params_for_action(name, action, arguments)
        for spec in _param_specs(describe, action):
            pname = str(spec.get("name") or "")
            if not pname:
                continue
            value = params.get(pname)
            if spec.get("required") and value is None:
                return f"params.{pname} required for {action} on {entity_id}"
            if value is None:
                continue
            number = _as_float(value)
            minimum = spec.get("minimum")
            maximum = spec.get("maximum")
            if number is not None and isinstance(minimum, (int, float)) and number < float(minimum):
                return f"params.{pname}={value} is below minimum {minimum}"
            if number is not None and isinstance(maximum, (int, float)) and number > float(maximum):
                return f"params.{pname}={value} is above maximum {maximum}"
            enum = spec.get("enum") or spec.get("r#enum")
            if isinstance(enum, list) and enum and str(value) not in {str(x) for x in enum}:
                return f"params.{pname}={value} is not in {enum}"
    return None
