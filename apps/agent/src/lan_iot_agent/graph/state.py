"""LangGraph agent state."""

from __future__ import annotations

from typing import Any, TypedDict


class AgentState(TypedDict, total=False):
    """Mutable LangGraph state shared across receive → LLM ↔ tools → reply."""

    message: str
    request_devices: list[Any]
    request_scenes: list[str]
    conversation_history: list[dict[str, Any]]
    # P5 danger-confirm fields from ChatRequest
    confirm: bool
    request_pending_action: str | None
    context_summary: str
    context_snapshot: dict[str, Any]
    pending_tools: list[dict[str, Any]]
    tool_results: list[dict[str, Any]]
    # Multi-turn LLM ↔ MCP tool loop
    llm_messages: list[dict[str, Any]]
    tool_iterations: int
    max_tool_iterations: int
    awaiting_tool_followup: bool
    reply: str
    errors: list[str]
    used_llm: bool
    used_tools: bool
    status: str
    meta: dict[str, Any]
    requires_confirmation: bool
    pending_action: str | None
