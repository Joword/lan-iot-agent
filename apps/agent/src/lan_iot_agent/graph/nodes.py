"""LangGraph node functions for the Agent pipeline."""

from __future__ import annotations

import json
import logging
from typing import Any

from lan_iot_agent.context.builder import build_context_text, history_as_messages, system_prompt
from lan_iot_agent.graph.state import AgentState
from lan_iot_agent.llm.client import complete, echo_stub, tool_result_message
from lan_iot_agent.settings import get_settings
from lan_iot_agent.tools.hub import HubClient, extract_scene_ids
from lan_iot_agent.tools.keywords import (
    PENDING_ACTION_SHUTDOWN_ALL,
    detect_dangerous_intent,
    entity_id_of,
    infer_tools_from_message,
    is_controllable_entity,
    shutdown_all_confirmation_reply,
)
from lan_iot_agent.tools.capabilities import (
    CONTROL_TOOLS,
    describe_payload,
    entity_id_of_tool,
    validate_against_describe,
)
from lan_iot_agent.tools.mcp import McpClient, extract_devices_from_mcp
from lan_iot_agent.tools.schemas import ALLOWED_LLM_TOOLS, hub_tool_schemas

logger = logging.getLogger(__name__)


def receive_message(state: AgentState) -> AgentState:
    """Normalize the inbound message and reset per-turn tool / LLM fields."""
    message = (state.get("message") or "").strip()
    errors = list(state.get("errors") or [])
    if not message:
        errors.append("empty message")
    return {
        **state,
        "message": message,
        "errors": errors,
        "pending_tools": [],
        "tool_results": [],
        "llm_messages": [],
        "tool_iterations": 0,
        "awaiting_tool_followup": False,
        "used_llm": False,
        "used_tools": False,
        "status": "ok",
        "requires_confirmation": False,
        "pending_action": None,
        "meta": dict(state.get("meta") or {}),
    }


def _shutdown_all_confirmed(state: AgentState, dangerous: str | None) -> bool:
    """True when client confirmed shutdown_all (confirm + optional pending_action match)."""
    if not state.get("confirm"):
        return False
    requested = state.get("request_pending_action")
    if requested is not None and requested != PENDING_ACTION_SHUTDOWN_ALL:
        return False
    # confirm + explicit pending_action, or confirm + dangerous phrasing in message
    if requested == PENDING_ACTION_SHUTDOWN_ALL:
        return True
    return dangerous == PENDING_ACTION_SHUTDOWN_ALL


async def build_context(state: AgentState) -> AgentState:
    """Build prompt context; fetch Hub devices/scenes when body omits them."""
    devices = list(state.get("request_devices") or [])
    scenes = list(state.get("request_scenes") or [])
    errors = list(state.get("errors") or [])
    meta = dict(state.get("meta") or {})

    if not devices:
        client = McpClient()
        try:
            outcome = await client.list_devices()
            if outcome.ok:
                devices = list(outcome.data.get("devices") or []) if outcome.data else []
                meta["context_source"] = "hub_mcp_devices.list"
                meta["injected_device_count"] = len(devices)
            else:
                errors.append(
                    f"context devices.list via MCP failed: {outcome.error or 'unknown'}"
                )
                meta["context_source"] = "empty"
        except Exception as exc:  # noqa: BLE001
            logger.warning("MCP devices.list for context failed: %s", exc)
            errors.append(f"context devices.list crashed: {exc}")
            meta["context_source"] = "empty"
    else:
        meta["context_source"] = "request_body"

    if not scenes:
        try:
            hub = HubClient()
            scene_result = await hub.list_scenes()
            if scene_result.ok:
                scenes = extract_scene_ids(scene_result.data)
                meta["scenes_source"] = "hub_rest"
            else:
                # Demo ids always known locally even if Hub scenes route is down.
                scenes = ["sleep_mode", "away_mode"]
                meta["scenes_source"] = "builtin_demo"
                if scene_result.error:
                    errors.append(f"context scenes list: {scene_result.error}")
        except Exception as exc:  # noqa: BLE001
            logger.warning("Hub scenes list failed: %s", exc)
            scenes = ["sleep_mode", "away_mode"]
            meta["scenes_source"] = "builtin_demo"
            errors.append(f"context scenes crashed: {exc}")
    else:
        meta["scenes_source"] = "request_body"

    built = build_context_text(
        message=state.get("message") or "",
        devices=devices,
        scenes=scenes,
        conversation_history=state.get("conversation_history") or [],
    )
    status = state.get("status") or "ok"
    if errors and status == "ok" and meta.get("context_source") == "empty":
        status = "degraded"

    return {
        **state,
        "request_devices": devices,
        "request_scenes": scenes,
        "context_summary": built["summary"],
        "context_snapshot": built["snapshot"],
        "errors": errors,
        "status": status,
        "meta": {
            **meta,
            "device_count": built["device_count"],
            "scene_count": built["scene_count"],
        },
    }


def _pending_from_llm_tool_calls(tool_calls: list[Any]) -> list[dict[str, Any]]:
    """Map LlmToolCall objects to pending MCP tool dicts (drop unknown names)."""
    pending: list[dict[str, Any]] = []
    for tc in tool_calls:
        name = getattr(tc, "name", None) or (tc.get("name") if isinstance(tc, dict) else None)
        if not name or name not in ALLOWED_LLM_TOOLS:
            continue
        arguments = getattr(tc, "arguments", None)
        if arguments is None and isinstance(tc, dict):
            arguments = tc.get("arguments")
        call_id = getattr(tc, "id", None) or (tc.get("id") if isinstance(tc, dict) else None)
        pending.append(
            {
                "name": str(name),
                "arguments": dict(arguments or {}),
                "tool_call_id": str(call_id or f"call_{len(pending)}"),
            }
        )
    return pending


def llm_reasoning(state: AgentState) -> AgentState:
    """Call LiteLLM with Hub tool schemas; parse tool_calls or fall back to keywords.

    When LLM is up: trust tool_calls / final text (no keyword double-fire).
    When LLM is down: keyword stubs still populate pending_tools for MCP.
    Follow-up turns reuse ``llm_messages`` after Hub MCP results were appended.
    """
    message = state.get("message") or ""
    summary = state.get("context_summary") or ""
    errors = list(state.get("errors") or [])
    meta = dict(state.get("meta") or {})
    settings = get_settings().llm
    max_iters = int(state.get("max_tool_iterations") or settings.max_tool_iterations or 5)
    iterations = int(state.get("tool_iterations") or 0)

    dangerous = detect_dangerous_intent(message)
    requested_pending = state.get("request_pending_action")
    # Confirmed shutdown via pending_action alone (UI re-POST with confirm).
    wants_shutdown = dangerous == PENDING_ACTION_SHUTDOWN_ALL or (
        bool(state.get("confirm")) and requested_pending == PENDING_ACTION_SHUTDOWN_ALL
    )

    if wants_shutdown and not _shutdown_all_confirmed(state, dangerous):
        meta["dangerous_intent"] = PENDING_ACTION_SHUTDOWN_ALL
        meta["confirm_required"] = True
        return {
            **state,
            "reply": shutdown_all_confirmation_reply(),
            "pending_tools": [],
            "errors": errors,
            "used_llm": False,
            "status": "ok",
            "requires_confirmation": True,
            "pending_action": PENDING_ACTION_SHUTDOWN_ALL,
            "awaiting_tool_followup": False,
            "meta": meta,
        }

    if wants_shutdown and _shutdown_all_confirmed(state, dangerous):
        meta["dangerous_intent"] = PENDING_ACTION_SHUTDOWN_ALL
        meta["confirm_accepted"] = True
        return {
            **state,
            "reply": "[stub] Confirmed — shutting down controllable devices via Hub MCP",
            "pending_tools": [
                {"name": "_internal.shutdown_all", "arguments": {}},
            ],
            "errors": errors,
            "used_llm": False,
            "status": "ok",
            "requires_confirmation": False,
            "pending_action": None,
            "awaiting_tool_followup": False,
            "meta": meta,
        }

    # Keyword tools — only used when LLM is unavailable.
    keyword_pending: list[dict[str, Any]] = infer_tools_from_message(
        message,
        devices=state.get("request_devices"),
    )

    llm_messages = list(state.get("llm_messages") or [])
    if not llm_messages:
        history_msgs = history_as_messages(state.get("conversation_history") or [])
        llm_messages = [
            {"role": "system", "content": system_prompt(summary)},
            *history_msgs,
            {"role": "user", "content": message},
        ]

    tools = hub_tool_schemas()
    result = complete(llm_messages, settings=settings, tools=tools, tool_choice="auto")

    if result.stub or (not result.text and not result.tool_calls):
        # LLM down / empty → keyword fallback.
        used_llm = False
        if result.error:
            errors.append(f"LLM unavailable: {result.error}")
        pending = keyword_pending
        if pending:
            names = ", ".join(t["name"] for t in pending)
            reply = f"[stub] No LLM — running Hub MCP tool(s): {names}"
        else:
            reply = echo_stub(message, summary)
        meta["llm_stub"] = True
        meta["llm_provider"] = result.provider
        meta["llm_model"] = result.model
        meta["tool_source"] = "keywords" if pending else "none"
        return {
            **state,
            "reply": reply,
            "pending_tools": pending,
            "llm_messages": llm_messages,
            "tool_iterations": iterations,
            "max_tool_iterations": max_iters,
            "awaiting_tool_followup": False,
            "errors": errors,
            "used_llm": used_llm,
            "status": "degraded",
            "requires_confirmation": False,
            "pending_action": None,
            "meta": meta,
        }

    # Successful LLM turn (text and/or tool_calls).
    used_llm = True
    if result.used_fallback and result.error:
        errors.append(f"primary LLM failed, used fallback: {result.error}")
        meta["llm_fallback"] = True
    meta["llm_provider"] = result.provider
    meta["llm_model"] = result.model
    meta["llm_stub"] = False
    status = "degraded" if result.used_fallback and result.error else (state.get("status") or "ok")
    if errors and status == "ok":
        status = "degraded"

    assistant = result.assistant_message or {
        "role": "assistant",
        "content": result.text or None,
    }
    llm_messages = llm_messages + [assistant]

    pending = _pending_from_llm_tool_calls(result.tool_calls)
    if pending:
        # Cap iterations: if already at max, refuse more tools and ask for text next.
        if iterations >= max_iters:
            errors.append(f"tool loop hit max_tool_iterations={max_iters}")
            meta["tool_loop_capped"] = True
            reply = (
                result.text
                or "I reached the tool-call limit before finishing. "
                "Please rephrase or try a simpler request."
            )
            return {
                **state,
                "reply": reply,
                "pending_tools": [],
                "llm_messages": llm_messages,
                "tool_iterations": iterations,
                "max_tool_iterations": max_iters,
                "awaiting_tool_followup": False,
                "errors": errors,
                "used_llm": used_llm,
                "status": "degraded",
                "requires_confirmation": False,
                "pending_action": None,
                "meta": {**meta, "tool_source": "llm_capped"},
            }

        meta["tool_source"] = "llm"
        meta["llm_tool_call_count"] = len(pending)
        return {
            **state,
            "reply": result.text or "",
            "pending_tools": pending,
            "llm_messages": llm_messages,
            "tool_iterations": iterations + 1,
            "max_tool_iterations": max_iters,
            "awaiting_tool_followup": True,
            "errors": errors,
            "used_llm": used_llm,
            "status": status,
            "requires_confirmation": False,
            "pending_action": None,
            "meta": meta,
        }

    # Final text — no more tools (or only unknown tool names were filtered out).
    if result.tool_calls and not pending:
        errors.append("LLM requested unknown tools; ignored")
        meta["ignored_tool_calls"] = [getattr(tc, "name", "?") for tc in result.tool_calls]
    reply_text = (result.text or "").strip()
    if not reply_text and state.get("used_tools"):
        # Prefer summarizing prior MCP lines if the model returned nothing.
        last_lines = meta.get("last_tool_lines") or []
        reply_text = "\n".join(str(x) for x in last_lines) if last_lines else ""
    if not reply_text:
        reply_text = result.text or "Done."
    meta["tool_source"] = "llm_text"
    return {
        **state,
        "reply": reply_text,
        "pending_tools": [],
        "llm_messages": llm_messages,
        "tool_iterations": iterations,
        "max_tool_iterations": max_iters,
        "awaiting_tool_followup": False,
        "errors": errors,
        "used_llm": used_llm,
        "status": status,
        "requires_confirmation": False,
        "pending_action": None,
        "meta": meta,
    }


def should_run_tools(state: AgentState) -> str:
    """Route after LLM: ``execute_tools`` when pending_tools is non-empty, else ``reply``."""
    if state.get("pending_tools"):
        return "execute_tools"
    return "reply"


def after_tools(state: AgentState) -> str:
    """After MCP execution: loop back to LLM if awaiting a follow-up turn."""
    if not state.get("awaiting_tool_followup"):
        return "reply"
    if not state.get("used_llm"):
        return "reply"
    iterations = int(state.get("tool_iterations") or 0)
    max_iters = int(state.get("max_tool_iterations") or 5)
    if iterations > max_iters:
        return "reply"
    return "llm_reasoning"


def _format_tool_reply(name: str, outcome_data: Any, outcome_ok: bool, error: str | None) -> str:
    if not outcome_ok:
        return f"{name} failed: {error or 'unknown error'}"
    if name == "devices.list":
        devices = extract_devices_from_mcp(outcome_data)
        if not devices and isinstance(outcome_data, dict):
            devices = extract_devices_from_mcp(outcome_data.get("data"))
        preview = json.dumps(devices[:20], ensure_ascii=False, default=str)
        return f"devices.list → {len(devices)} device(s): {preview}"
    if name == "scenes.run":
        payload = outcome_data
        if isinstance(outcome_data, dict) and "data" in outcome_data:
            payload = outcome_data.get("data")
        scene_id = None
        if isinstance(payload, dict):
            scene_id = payload.get("scene_id") or payload.get("id")
        label = scene_id or "?"
        return f"scenes.run → {label}: {json.dumps(payload, ensure_ascii=False, default=str)[:600]}"
    return f"{name} → {json.dumps(outcome_data, ensure_ascii=False, default=str)[:800]}"


def _tool_result_content(name: str, outcome_data: Any, outcome_ok: bool, error: str | None) -> str:
    """Compact JSON string fed back to the LLM as a tool message."""
    payload: dict[str, Any] = {"ok": outcome_ok, "tool": name}
    if outcome_ok:
        payload["data"] = outcome_data
    else:
        payload["error"] = error or "unknown error"
    try:
        return json.dumps(payload, ensure_ascii=False, default=str)[:4000]
    except TypeError:
        return str(payload)[:4000]


async def _execute_shutdown_all(
    client: McpClient,
    *,
    errors: list[str],
    results: list[dict[str, Any]],
    meta: dict[str, Any],
) -> str:
    """devices.list then best-effort devices.control turn_off for controllable entities."""
    list_outcome = await client.list_devices()
    results.append(
        {
            "name": "devices.list",
            "ok": list_outcome.ok,
            "data": list_outcome.data,
            "error": list_outcome.error,
        }
    )
    if not list_outcome.ok:
        err = list_outcome.error or "devices.list failed"
        errors.append(err)
        return f"shutdown_all aborted: could not list devices ({err})"

    devices = list((list_outcome.data or {}).get("devices") or [])
    targets: list[str] = []
    seen: set[str] = set()
    for device in devices:
        entity_id = entity_id_of(device)
        if not entity_id or entity_id in seen:
            continue
        if not is_controllable_entity(entity_id):
            continue
        seen.add(entity_id)
        targets.append(entity_id)

    meta["shutdown_all_targets"] = targets
    if not targets:
        return "shutdown_all: no controllable devices found to turn off."

    ok_ids: list[str] = []
    fail_ids: list[str] = []
    for entity_id in targets:
        try:
            outcome = await client.call_tool(
                "devices.control",
                {"entity_id": entity_id, "action": "turn_off", "params": {}},
            )
            entry = {
                "name": "devices.control",
                "ok": outcome.ok,
                "data": outcome.data,
                "error": outcome.error,
                "entity_id": entity_id,
                "action": "turn_off",
            }
            results.append(entry)
            if outcome.ok:
                ok_ids.append(entity_id)
            else:
                fail_ids.append(entity_id)
                if outcome.error:
                    errors.append(f"{entity_id}: {outcome.error}")
        except Exception as exc:  # noqa: BLE001
            logger.exception("shutdown_all turn_off failed for %s", entity_id)
            fail_ids.append(entity_id)
            errors.append(f"{entity_id} crashed: {exc}")
            results.append(
                {
                    "name": "devices.control",
                    "ok": False,
                    "error": str(exc),
                    "entity_id": entity_id,
                    "action": "turn_off",
                }
            )

    meta["shutdown_all_ok"] = ok_ids
    meta["shutdown_all_failed"] = fail_ids
    lines = [
        f"shutdown_all complete: {len(ok_ids)} ok, {len(fail_ids)} failed "
        f"(of {len(targets)} controllable).",
    ]
    if ok_ids:
        lines.append("off: " + ", ".join(ok_ids[:40]))
    if fail_ids:
        lines.append("failed: " + ", ".join(fail_ids[:40]))
    return "\n".join(lines)


async def execute_tools(state: AgentState) -> AgentState:
    """Run pending Hub MCP tools; feed results into llm_messages for follow-up turns."""
    pending = list(state.get("pending_tools") or [])
    errors = list(state.get("errors") or [])
    results: list[dict[str, Any]] = list(state.get("tool_results") or [])
    meta = dict(state.get("meta") or {})
    reply = state.get("reply") or ""
    used_tools = False
    client = McpClient()
    tool_lines: list[str] = []
    llm_messages = list(state.get("llm_messages") or [])
    awaiting = bool(state.get("awaiting_tool_followup"))

    for tool in pending:
        name = str(tool.get("name") or "")
        arguments = tool.get("arguments") or {}
        tool_call_id = str(tool.get("tool_call_id") or f"call_{len(results)}")

        if name == "_internal.shutdown_all":
            try:
                summary = await _execute_shutdown_all(
                    client, errors=errors, results=results, meta=meta
                )
                used_tools = True
                tool_lines.append(summary)
            except Exception as exc:  # noqa: BLE001
                logger.exception("shutdown_all failed")
                errors.append(f"shutdown_all crashed: {exc}")
                results.append({"name": name, "ok": False, "error": str(exc)})
                tool_lines.append(f"shutdown_all crashed: {exc}")
            continue

        arguments = arguments if isinstance(arguments, dict) else {}
        if name in CONTROL_TOOLS:
            entity_id = entity_id_of_tool(name, arguments)
            describe_doc: dict[str, Any] | None = None
            if entity_id:
                described = await client.call_tool(
                    "devices.describe", {"entity_id": entity_id}
                )
                if described.ok:
                    describe_doc = describe_payload(described.data)
            rejected = validate_against_describe(name, arguments, describe_doc)
            if rejected:
                used_tools = True
                entry = {
                    "name": name,
                    "ok": False,
                    "error": rejected,
                    "tool_call_id": tool_call_id,
                }
                results.append(entry)
                errors.append(rejected)
                tool_lines.append(f"{name} rejected: {rejected}")
                if awaiting:
                    llm_messages.append(
                        tool_result_message(
                            tool_call_id,
                            _tool_result_content(name, None, False, rejected),
                        )
                    )
                continue

        try:
            outcome = await client.call_tool(
                name, arguments if isinstance(arguments, dict) else {}
            )
            entry = {
                "name": name,
                "ok": outcome.ok,
                "data": outcome.data,
                "error": outcome.error,
                "tool_call_id": tool_call_id,
            }
            results.append(entry)
            used_tools = True
            line = _format_tool_reply(name, outcome.data, outcome.ok, outcome.error)
            tool_lines.append(line)
            if awaiting:
                llm_messages.append(
                    tool_result_message(
                        tool_call_id,
                        _tool_result_content(name, outcome.data, outcome.ok, outcome.error),
                    )
                )
            if not outcome.ok and outcome.error:
                errors.append(outcome.error)
            elif outcome.ok and name == "devices.list":
                devices = extract_devices_from_mcp(outcome.data)
                meta["hub_devices"] = devices
            elif outcome.ok and name == "scenes.run":
                meta["last_scene"] = (
                    arguments.get("scene_id") if isinstance(arguments, dict) else None
                )
        except Exception as exc:  # noqa: BLE001 — tools must not crash the graph
            logger.exception("Tool %s failed", name)
            errors.append(f"tool {name} crashed: {exc}")
            results.append(
                {
                    "name": name,
                    "ok": False,
                    "error": str(exc),
                    "tool_call_id": tool_call_id,
                }
            )
            tool_lines.append(f"{name} crashed: {exc}")
            if awaiting:
                llm_messages.append(
                    tool_result_message(
                        tool_call_id,
                        _tool_result_content(name, None, False, str(exc)),
                    )
                )

    # Keyword / stub path: surface tool lines in the reply.
    # LLM path with follow-up: keep reply empty-ish until final LLM text.
    if tool_lines and not awaiting:
        if not state.get("used_llm"):
            reply = (reply + "\n\n" + "\n".join(tool_lines)).strip()
        else:
            reply = (reply + "\n\n[tools]\n" + "\n".join(tool_lines)).strip()
    elif tool_lines and awaiting:
        meta["last_tool_lines"] = tool_lines

    status = state.get("status") or "ok"
    if errors and status == "ok":
        status = "degraded"

    return {
        **state,
        "reply": reply,
        "tool_results": results,
        "pending_tools": [],
        "llm_messages": llm_messages,
        "used_tools": used_tools or bool(state.get("used_tools")),
        "errors": errors,
        "status": status,
        "meta": meta,
    }


def reply_node(state: AgentState) -> AgentState:
    """Finalize the user-facing reply string and clear tool-followup flags."""
    reply = (state.get("reply") or "").strip()
    errors = list(state.get("errors") or [])
    status = state.get("status") or "ok"
    # If we capped mid-loop with empty reply but have tool lines, surface them.
    if not reply:
        last_lines = (state.get("meta") or {}).get("last_tool_lines")
        if last_lines:
            reply = "\n".join(str(x) for x in last_lines)
    if not reply:
        reply = "I could not generate a reply."
        errors.append("empty reply")
        status = "error"
    return {
        **state,
        "reply": reply,
        "errors": errors,
        "status": status,
        "awaiting_tool_followup": False,
    }
