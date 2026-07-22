"""Keyword → Hub MCP tool stubs when LLM is unavailable."""

from __future__ import annotations

import re
from typing import Any

# P4 demo climate entity (HA MQTT faker); used when context has no climate.* device.
DEFAULT_CLIMATE_ENTITY = "climate.demo_gree_ac"
_DEFAULT_CLIMATE_TEMP = 26.0

# P4 faker Xiaomi light — preferred when message mentions 小米 / xiaomi and no entity given.
DEFAULT_XIAOMI_LIGHT = "light.faker_xiaomi_bulb"

# P6 demo Companion (Hub HTTP); used when context has no companion.* device.
DEFAULT_COMPANION_ID = "companion.demo_pc"
_DEFAULT_COMPANION_COMMAND = "ping"

_LIST_DEVICES_RE = re.compile(
    r"(list\s+devices|show\s+devices|设备列表|有哪些设备|列出设备|what\s+devices)",
    re.IGNORECASE,
)

# turn on light.demo / 打开 light.bedroom_lamp
_TURN_ON_RE = re.compile(
    r"(?:turn\s+on|switch\s+on|打开|开启)\s+([a-zA-Z_][\w]*\.[\w]+)",
    re.IGNORECASE,
)
_TURN_OFF_RE = re.compile(
    r"(?:turn\s+off|switch\s+off|关闭|关掉)\s+([a-zA-Z_][\w]*\.[\w]+)",
    re.IGNORECASE,
)

# lights.control style: "light.demo on" / "light.demo off"
_LIGHT_ON_OFF_RE = re.compile(
    r"\b(light\.[\w]+)\s+(on|off)\b",
    re.IGNORECASE,
)

# climate.set: set climate.living to 26 / 把 climate.living 调到 26
_CLIMATE_SET_RE = re.compile(
    r"(?:set|调到|设置)\s*(climate\.[\w]+)\s*(?:to|到|=)?\s*(\d{1,2}(?:\.\d+)?)",
    re.IGNORECASE,
)
_CLIMATE_SET_RE_CN = re.compile(
    r"(climate\.[\w]+).*?(?:调到|设置(?:为|成)?|温度)\s*(\d{1,2}(?:\.\d+)?)",
    re.IGNORECASE,
)

# Vague climate intent (no entity_id): temperature / 调到 / 空调 / cool / climate
_CLIMATE_INTENT_RE = re.compile(
    r"(temperature|temp\b|调到|空调|cool|climate|制冷|温度)",
    re.IGNORECASE,
)
# Optional setpoint in vague climate phrases: 调到 26 / to 24 / 26度
_TEMP_VALUE_RE = re.compile(
    r"(?:调到|设置(?:为|成)?|set(?:\s+(?:to|temperature|temp))?|to|=|温度)\s*"
    r"(\d{1,2}(?:\.\d+)?)"
    r"|(\d{1,2}(?:\.\d+)?)\s*(?:度|°\s*[cC]|celsius)",
    re.IGNORECASE,
)
_COOL_MODE_RE = re.compile(r"\b(cool|制冷)\b", re.IGNORECASE)
_CLIMATE_ON_RE = re.compile(
    r"(?:turn\s+on|switch\s+on|打开|开启).*(?:空调|climate|ac\b)"
    r"|(?:空调|climate).*(?:打开|开启|turn\s+on)",
    re.IGNORECASE,
)

# devices.control generic: control entity_id action
_CONTROL_RE = re.compile(
    r"(?:control|控制)\s+([a-zA-Z_][\w]*\.[\w]+)\s+([a-zA-Z_][\w]*)",
    re.IGNORECASE,
)

# entity_id + turn_on/turn_off as bare action words nearby
_ENTITY_ACTION_RE = re.compile(
    r"\b([a-zA-Z_][\w]*\.[\w]+)\b.*?\b(turn_on|turn_off|toggle)\b"
    r"|\b(turn_on|turn_off|toggle)\b.*?\b([a-zA-Z_][\w]*\.[\w]+)\b",
    re.IGNORECASE,
)

# scenes.run — sleep / away / 场景
_SLEEP_SCENE_RE = re.compile(
    r"(sleep(?:\s*mode)?|sleep_mode|睡眠模式|睡觉|晚安)",
    re.IGNORECASE,
)
_AWAY_SCENE_RE = re.compile(
    r"(away(?:\s*mode)?|away_mode|离家(?:模式)?|外出模式|出门)",
    re.IGNORECASE,
)
# Explicit: run scene sleep_mode / 运行场景 away_mode / 场景 sleep_mode
_EXPLICIT_SCENE_RE = re.compile(
    r"(?:run\s+scene|activate\s+scene|场景|运行场景|执行场景)\s*[:=]?\s*"
    r"(sleep_mode|away_mode|[\w\-]+)",
    re.IGNORECASE,
)
# Bare scene ids
_SCENE_ID_RE = re.compile(r"\b(sleep_mode|away_mode)\b", re.IGNORECASE)

# companion.command — Companion / 电脑 / ping companion / notify
_COMPANION_PING_RE = re.compile(
    r"(?:ping\s+(?:companion|电脑|pc)|(?:companion|电脑|pc)\s+ping)",
    re.IGNORECASE,
)
_COMPANION_NOTIFY_RE = re.compile(
    r"(?:notify(?:\s+(?:companion|电脑|pc))?|(?:companion|电脑|pc)\s+notify|"
    r"通知)",
    re.IGNORECASE,
)
_COMPANION_INTENT_RE = re.compile(
    r"(companion|电脑)",
    re.IGNORECASE,
)
# Explicit companion id in message: companion.demo_pc
_COMPANION_ID_RE = re.compile(r"\b(companion\.[\w]+)\b", re.IGNORECASE)

# P5 dangerous mass-off intents — require confirm:true before MCP turn_off fan-out.
PENDING_ACTION_SHUTDOWN_ALL = "shutdown_all"
_SHUTDOWN_ALL_RE = re.compile(
    r"(关闭所有|全部关闭|turn\s+off\s+all|shut\s+everything|"
    r"shut\s+down\s+everything|turn\s+everything\s+off|"
    r"switch\s+off\s+all|all\s+off)",
    re.IGNORECASE,
)

# Domains safe to mass turn_off (exclude sensors / trackers / etc.).
_CONTROLLABLE_DOMAINS = frozenset(
    {
        "light",
        "switch",
        "climate",
        "fan",
        "cover",
        "media_player",
        "lock",
        "vacuum",
        "humidifier",
        "water_heater",
        "input_boolean",
        "remote",
        "siren",
        "valve",
        "air_humidifier",
    }
)


def detect_dangerous_intent(message: str) -> str | None:
    """Return a pending_action id when the message is a dangerous mass action."""
    text = (message or "").strip()
    if not text:
        return None
    if _SHUTDOWN_ALL_RE.search(text):
        return PENDING_ACTION_SHUTDOWN_ALL
    return None


def is_controllable_entity(entity_id: str | None) -> bool:
    """True when entity_id domain supports a best-effort turn_off."""
    if not entity_id or "." not in entity_id:
        return False
    domain = entity_id.split(".", 1)[0].lower()
    return domain in _CONTROLLABLE_DOMAINS


def shutdown_all_confirmation_reply() -> str:
    """User-facing text asking for an explicit confirm round-trip before mass-off."""
    return (
        "This will turn off all controllable devices. "
        "Re-send the same request with confirm=true "
        f'(and pending_action="{PENDING_ACTION_SHUTDOWN_ALL}") to proceed.'
    )


def entity_id_of(device: Any) -> str | None:
    """Best-effort entity_id from a Hub/device snapshot dict."""
    if not isinstance(device, dict):
        return None
    for key in ("entity_id", "id", "entityId"):
        val = device.get(key)
        if isinstance(val, str) and "." in val:
            return val
    return None


# Back-compat alias used inside this module / older call sites.
_entity_id_of = entity_id_of


def resolve_climate_entity(devices: list[Any] | None = None) -> str:
    """Prefer a climate.* entity from context; else P4 faker ``climate.demo_gree_ac``."""
    for device in devices or []:
        entity_id = entity_id_of(device)
        if entity_id and entity_id.lower().startswith("climate."):
            return entity_id
        if isinstance(device, dict):
            domain = str(device.get("domain") or device.get("type") or "").lower()
            if domain in {"climate", "ac"} and entity_id:
                return entity_id
    return DEFAULT_CLIMATE_ENTITY


def resolve_xiaomi_light(devices: list[Any] | None = None) -> str:
    """Prefer a Xiaomi/faker light from context; else ``light.faker_xiaomi_bulb``."""
    for device in devices or []:
        entity_id = entity_id_of(device)
        if not entity_id or not entity_id.lower().startswith("light."):
            continue
        blob = entity_id.lower()
        brand = ""
        if isinstance(device, dict):
            brand = str(device.get("brand") or "").lower()
            name = str(device.get("friendly_name") or device.get("name") or "").lower()
            blob = f"{blob} {brand} {name}"
        if "xiaomi" in blob or "faker_xiaomi" in blob:
            return entity_id
    return DEFAULT_XIAOMI_LIGHT


_XIAOMI_INTENT_RE = re.compile(r"(xiaomi|小米|米家)", re.IGNORECASE)


def resolve_companion_id(devices: list[Any] | None = None) -> str:
    """Prefer a companion.* id from context; else P6 demo ``companion.demo_pc``."""
    for device in devices or []:
        entity_id = entity_id_of(device)
        if entity_id and entity_id.lower().startswith("companion."):
            return entity_id
        if isinstance(device, dict):
            for key in ("device_id", "id", "companion_id"):
                val = device.get(key)
                if isinstance(val, str) and val.lower().startswith("companion."):
                    return val
            kind = str(
                device.get("kind")
                or device.get("type")
                or device.get("domain")
                or ""
            ).lower()
            if kind in {"companion", "pc", "phone"} and entity_id:
                return entity_id
    return DEFAULT_COMPANION_ID


def _extract_temperature(text: str) -> float | None:
    if m := _TEMP_VALUE_RE.search(text):
        raw = m.group(1) or m.group(2)
        if raw is not None:
            return float(raw)
    return None


def infer_tools_from_message(
    message: str,
    devices: list[Any] | None = None,
) -> list[dict[str, Any]]:
    """Return pending MCP tool calls inferred from simple keywords.

    Used when LLM is down (and as a cheap hint even when LLM is up).
    ``devices`` (chat context / Hub snapshot) preferred for climate entity_id.
    """
    text = (message or "").strip()
    if not text:
        return []

    pending: list[dict[str, Any]] = []
    seen: set[str] = set()

    def _add(name: str, arguments: dict[str, Any]) -> None:
        key = f"{name}:{sorted(arguments.items())}"
        if key in seen:
            return
        seen.add(key)
        pending.append({"name": name, "arguments": arguments})

    if _LIST_DEVICES_RE.search(text):
        _add("devices.list", {})

    # Scenes before device toggles so "sleep mode turn off lights" still runs scene.
    if m := _EXPLICIT_SCENE_RE.search(text):
        scene_id = m.group(1).lower()
        if scene_id in {"sleep", "sleepmode"}:
            scene_id = "sleep_mode"
        elif scene_id in {"away", "awaymode"}:
            scene_id = "away_mode"
        _add("scenes.run", {"scene_id": scene_id})
    elif m := _SCENE_ID_RE.search(text):
        _add("scenes.run", {"scene_id": m.group(1).lower()})
    elif _SLEEP_SCENE_RE.search(text):
        _add("scenes.run", {"scene_id": "sleep_mode"})
    elif _AWAY_SCENE_RE.search(text):
        _add("scenes.run", {"scene_id": "away_mode"})
    elif re.search(r"场景", text) and not pending:
        # Bare「场景」with no id → default demo sleep_mode (Hub ships sleep/away).
        _add("scenes.run", {"scene_id": "sleep_mode"})

    if m := _TURN_ON_RE.search(text):
        entity_id = m.group(1)
        if entity_id.startswith("light."):
            _add("lights.control", {"entity_id": entity_id, "on": True})
        else:
            _add("devices.control", {"entity_id": entity_id, "action": "turn_on", "params": {}})

    if m := _TURN_OFF_RE.search(text):
        entity_id = m.group(1)
        if entity_id.startswith("light."):
            _add("lights.control", {"entity_id": entity_id, "on": False})
        else:
            _add("devices.control", {"entity_id": entity_id, "action": "turn_off", "params": {}})

    if m := _LIGHT_ON_OFF_RE.search(text):
        entity_id, on_off = m.group(1), m.group(2).lower()
        _add("lights.control", {"entity_id": entity_id, "on": on_off == "on"})

    # Vague Xiaomi / 小米 without entity_id → faker Xiaomi bulb (swap later).
    if _XIAOMI_INTENT_RE.search(text) and not any(
        t.get("name") == "lights.control" for t in pending
    ):
        on = not bool(
            re.search(r"(关掉|关闭|turn\s+off|switch\s+off)", text, re.IGNORECASE)
        )
        _add(
            "lights.control",
            {"entity_id": resolve_xiaomi_light(devices), "on": on},
        )

    climate_added = False
    for pattern in (_CLIMATE_SET_RE, _CLIMATE_SET_RE_CN):
        if m := pattern.search(text):
            _add(
                "climate.set",
                {
                    "entity_id": m.group(1),
                    "temperature": float(m.group(2)),
                },
            )
            climate_added = True
            break

    # Vague climate: prefer context climate entity, else demo_gree_ac.
    if not climate_added and _CLIMATE_INTENT_RE.search(text):
        entity_id = resolve_climate_entity(devices)
        temp = _extract_temperature(text)
        if temp is not None:
            _add("climate.set", {"entity_id": entity_id, "temperature": temp})
        elif _CLIMATE_ON_RE.search(text):
            _add(
                "devices.control",
                {"entity_id": entity_id, "action": "turn_on", "params": {}},
            )
        elif _COOL_MODE_RE.search(text):
            _add(
                "devices.control",
                {
                    "entity_id": entity_id,
                    "action": "set_hvac_mode",
                    "params": {"mode": "cool"},
                },
            )
        else:
            # Bare 空调 / climate / temperature → set demo setpoint via climate.set
            _add(
                "climate.set",
                {"entity_id": entity_id, "temperature": _DEFAULT_CLIMATE_TEMP},
            )

    if m := _CONTROL_RE.search(text):
        _add(
            "devices.control",
            {
                "entity_id": m.group(1),
                "action": m.group(2),
                "params": {},
            },
        )

    if m := _ENTITY_ACTION_RE.search(text):
        if m.group(1) and m.group(2):
            entity_id, action = m.group(1), m.group(2)
        else:
            action, entity_id = m.group(3), m.group(4)
        _add(
            "devices.control",
            {"entity_id": entity_id, "action": action.lower(), "params": {}},
        )

    # Companion stubs (after device toggles so light.* phrases stay lights.control).
    companion_command: str | None = None
    if _COMPANION_PING_RE.search(text):
        companion_command = "ping"
    elif _COMPANION_NOTIFY_RE.search(text):
        companion_command = "notify"
    elif _COMPANION_INTENT_RE.search(text):
        companion_command = _DEFAULT_COMPANION_COMMAND

    if companion_command is not None:
        if m := _COMPANION_ID_RE.search(text):
            device_id = m.group(1)
        else:
            device_id = resolve_companion_id(devices)
        _add(
            "companion.command",
            {"device_id": device_id, "command": companion_command},
        )

    return pending


def wants_device_list(message: str) -> bool:
    """True when the user message looks like a request to list devices."""
    return bool(_LIST_DEVICES_RE.search(message or ""))
