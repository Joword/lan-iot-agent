"""Context builder — formats device/scene snapshots for the LLM prompt."""

from __future__ import annotations

import json
from typing import Any


def _device_to_dict(device: Any) -> dict[str, Any]:
    if hasattr(device, "model_dump"):
        return device.model_dump(exclude_none=True)
    if isinstance(device, dict):
        return device
    return {"value": str(device)}


def build_context_text(
    *,
    message: str,
    devices: list[Any] | None = None,
    scenes: list[str] | None = None,
    conversation_history: list[dict[str, Any]] | None = None,
) -> dict[str, Any]:
    """Return structured context plus a prompt-ready summary string."""
    device_rows = [_device_to_dict(d) for d in (devices or [])]
    scene_rows = list(scenes or [])
    history = list(conversation_history or [])

    snapshot = {
        "devices": device_rows,
        "scenes": scene_rows,
        "conversation_history": history,
        "user_message": message,
    }

    if device_rows:
        devices_block = json.dumps(device_rows, ensure_ascii=False, indent=2)
    else:
        devices_block = "(no devices in request context)"

    scenes_block = ", ".join(scene_rows) if scene_rows else "(none)"

    history_lines: list[str] = []
    for turn in history[-8:]:
        if not isinstance(turn, dict):
            continue
        role = str(turn.get("role") or "user")
        content = str(turn.get("content") or turn.get("text") or "").strip()
        if content:
            history_lines.append(f"{role}: {content}")
    history_block = "\n".join(history_lines) if history_lines else "(none)"

    summary = (
        f"Devices:\n{devices_block}\n\n"
        f"Scenes: {scenes_block}\n\n"
        f"Recent conversation:\n{history_block}"
    )

    return {
        "snapshot": snapshot,
        "summary": summary,
        "device_count": len(device_rows),
        "scene_count": len(scene_rows),
        "history_messages": history_as_messages(history),
    }


def history_as_messages(
    conversation_history: list[dict[str, Any]] | None,
    *,
    max_turns: int = 8,
) -> list[dict[str, str]]:
    """Convert prior turns into OpenAI-style chat messages (user/assistant only)."""
    out: list[dict[str, str]] = []
    for turn in list(conversation_history or [])[-max_turns:]:
        if not isinstance(turn, dict):
            continue
        role = str(turn.get("role") or "").lower()
        if role not in {"user", "assistant"}:
            continue
        content = str(turn.get("content") or turn.get("text") or "").strip()
        if not content:
            continue
        out.append({"role": role, "content": content})
    return out


def system_prompt(context_summary: str) -> str:
    """Build the system message: IoT tool rules + current registry snapshot."""
    return (
        "You are a home IoT assistant for LanIoT. "
        "Reply in Chinese or English to match the user. "
        "You may ONLY use Hub MCP tools provided in this request "
        "(devices.list, devices.get_state, devices.describe, devices.control, climate.set, "
        "lights.control, scenes.run, companion.command). "
        "Never invent entity_ids or device state — if unsure which device, "
        "call devices.list first and use exact ids from the tool result or "
        "the registry snapshot below. "
        "Call devices.describe before devices.control / climate.set / lights.control "
        "when you are not sure which actions or param ranges the device allows. "
        "Prefer scenes.run for sleep/away/场景; climate.set for AC temperature; "
        "lights.control for lights; companion.command for PC/phone companions "
        "and robots already on the LAN. "
        "Do not claim an action succeeded unless a tool result confirms it. "
        "Keep final answers brief and actionable.\n\n"
        f"Current registry snapshot:\n{context_summary}"
    )
