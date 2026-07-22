"""Pydantic request/response models for the Agent HTTP API."""

from __future__ import annotations

from typing import Any

from pydantic import BaseModel, Field


class DeviceSnapshot(BaseModel):
    """Minimal device row injected into LLM context (Hub registry shape)."""

    entity_id: str | None = None
    id: str | None = None
    name: str | None = None
    state: str | None = None
    temp: float | int | None = None
    attributes: dict[str, Any] = Field(default_factory=dict)

    model_config = {"extra": "allow"}


class ChatContext(BaseModel):
    """Optional chat extras: devices, scenes, and short conversation history."""

    devices: list[DeviceSnapshot | dict[str, Any]] = Field(default_factory=list)
    scenes: list[str] = Field(default_factory=list)
    conversation_history: list[dict[str, Any]] = Field(default_factory=list)

    model_config = {"extra": "allow"}


class ChatRequest(BaseModel):
    """POST /v1/chat body from Hub / UI (message + optional context / confirm)."""

    message: str = Field(..., min_length=1, description="User message from Hub / UI")
    context: ChatContext | None = None
    # P5 danger-confirm: client must re-POST with confirm=true after requires_confirmation.
    confirm: bool = Field(
        default=False,
        description="When true, allow a previously pending dangerous action (e.g. shutdown_all)",
    )
    pending_action: str | None = Field(
        default=None,
        description="Optional action id from a prior requires_confirmation response",
    )


class HealthResponse(BaseModel):
    """GET /health payload."""

    status: str
    service: str


class ChatResponse(BaseModel):
    """Chat result returned to Hub WS / HTTP clients."""

    reply: str
    status: str = Field(description="ok | degraded | error")
    errors: list[str] = Field(default_factory=list)
    used_llm: bool = False
    used_tools: bool = False
    meta: dict[str, Any] = Field(default_factory=dict)
    # P5: set when a dangerous intent needs an explicit confirm round-trip.
    requires_confirmation: bool = False
    pending_action: str | None = None
