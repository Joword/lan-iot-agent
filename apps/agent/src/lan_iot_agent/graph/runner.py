"""Compile and run the LangGraph stub pipeline."""

from __future__ import annotations

import logging
from typing import Any

from lan_iot_agent.graph.nodes import (
    after_tools,
    build_context,
    execute_tools,
    llm_reasoning,
    receive_message,
    reply_node,
    should_run_tools,
)
from lan_iot_agent.graph.state import AgentState
from lan_iot_agent.models import ChatContext, ChatRequest, ChatResponse
from lan_iot_agent.settings import get_settings

logger = logging.getLogger(__name__)


def _build_graph():
    # Namespace package; pyright may not resolve without the Agent venv.
    from langgraph.graph import END, START, StateGraph

    graph = StateGraph(AgentState)
    graph.add_node("receive_message", receive_message)
    graph.add_node("build_context", build_context)
    graph.add_node("llm_reasoning", llm_reasoning)
    graph.add_node("execute_tools", execute_tools)
    graph.add_node("reply", reply_node)

    graph.add_edge(START, "receive_message")
    graph.add_edge("receive_message", "build_context")
    graph.add_edge("build_context", "llm_reasoning")
    graph.add_conditional_edges(
        "llm_reasoning",
        should_run_tools,
        {
            "execute_tools": "execute_tools",
            "reply": "reply",
        },
    )
    # After MCP: either loop back to LLM (tool follow-up) or finalize.
    graph.add_conditional_edges(
        "execute_tools",
        after_tools,
        {
            "llm_reasoning": "llm_reasoning",
            "reply": "reply",
        },
    )
    graph.add_edge("reply", END)
    return graph.compile()


# Lazy singleton kept in a mutable cache (mutated, never reassigned) so it stays
# a module-level UPPER_CASE constant for pylint without tripping basedpyright's
# reportConstantRedefinition.
_GRAPH_CACHE: dict[str, Any] = {}


def get_graph() -> Any:
    """Return the compiled LangGraph singleton (lazy-built on first use)."""
    graph = _GRAPH_CACHE.get("graph")
    if graph is None:
        graph = _build_graph()
        _GRAPH_CACHE["graph"] = graph
    return graph


def _initial_state(request: ChatRequest) -> AgentState:
    """Map a ChatRequest into the initial AgentState for graph / sequential run."""
    ctx: ChatContext = request.context or ChatContext()
    devices = list(ctx.devices or [])
    max_iters = get_settings().llm.max_tool_iterations
    return {
        "message": request.message,
        "request_devices": devices,
        "request_scenes": list(ctx.scenes or []),
        "conversation_history": list(ctx.conversation_history or []),
        "confirm": bool(request.confirm),
        "request_pending_action": request.pending_action,
        "errors": [],
        "pending_tools": [],
        "tool_results": [],
        "llm_messages": [],
        "tool_iterations": 0,
        "max_tool_iterations": max_iters,
        "awaiting_tool_followup": False,
        "used_llm": False,
        "used_tools": False,
        "status": "ok",
        "requires_confirmation": False,
        "pending_action": None,
        "meta": {},
    }


async def _run_sequential(state: AgentState) -> AgentState:
    """Fallback if LangGraph import/compile fails — same node order with tool loop."""
    state = receive_message(state)
    state = await build_context(state)
    for _ in range(int(state.get("max_tool_iterations") or 5) + 1):
        state = llm_reasoning(state)
        if should_run_tools(state) != "execute_tools":
            break
        state = await execute_tools(state)
        if after_tools(state) != "llm_reasoning":
            break
    return reply_node(state)


async def run_chat(request: ChatRequest) -> ChatResponse:
    """Execute receive → context → llm ↔ tools → reply.

    Response always includes ``reply`` (string) for Hub WS → Agent clients.
    """
    state = _initial_state(request)
    try:
        graph = get_graph()
        # ainvoke returns a mapping compatible with AgentState keys.
        final: AgentState = await graph.ainvoke(state)
    except Exception as exc:  # noqa: BLE001
        logger.warning("LangGraph path failed (%s); using sequential fallback", exc)
        final = await _run_sequential(state)
        errors = list(final.get("errors") or [])
        errors.append(f"graph fallback: {exc}")
        final["errors"] = errors
        if final.get("status") == "ok":
            final["status"] = "degraded"

    status = str(final.get("status") or "ok")
    errors = [str(e) for e in (final.get("errors") or [])]
    requires_confirmation = bool(final.get("requires_confirmation"))
    pending_action = final.get("pending_action")
    if pending_action is not None:
        pending_action = str(pending_action)

    if status == "error" and not (final.get("reply") or "").strip():
        return ChatResponse(
            reply="Agent failed to produce a reply.",
            status="error",
            errors=errors or ["unknown error"],
            used_llm=bool(final.get("used_llm")),
            used_tools=bool(final.get("used_tools")),
            meta=dict(final.get("meta") or {}),
            requires_confirmation=requires_confirmation,
            pending_action=pending_action,
        )

    return ChatResponse(
        reply=str(final.get("reply") or ""),
        status=status if status in {"ok", "degraded", "error"} else "degraded",
        errors=errors,
        used_llm=bool(final.get("used_llm")),
        used_tools=bool(final.get("used_tools")),
        meta=dict(final.get("meta") or {}),
        requires_confirmation=requires_confirmation,
        pending_action=pending_action,
    )
