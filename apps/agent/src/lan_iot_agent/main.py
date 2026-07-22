"""LanIoT Agent — FastAPI entry point.

Intelligence layer: receives messages from Hub, orchestrates LLM + tools.
Does not connect directly to HA / ESP32 / brand IoT / Companion.
"""

from __future__ import annotations

import logging

from fastapi import FastAPI, HTTPException

from lan_iot_agent.graph.runner import run_chat
from lan_iot_agent.models import ChatRequest, ChatResponse, HealthResponse
from lan_iot_agent.settings import get_settings

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s %(levelname)s [%(name)s] %(message)s",
)
logger = logging.getLogger(__name__)

app = FastAPI(
    title="LanIoT Agent",
    description="Intelligence layer for LanIoT Agent (NLU + tool orchestration)",
    version="0.1.0",
)


@app.get("/health", response_model=HealthResponse)
async def health() -> HealthResponse:
    """Health check — does not depend on Hub or LLM readiness."""
    return HealthResponse(status="ok", service="agent")


@app.post("/v1/chat", response_model=ChatResponse)
async def chat(body: ChatRequest) -> ChatResponse:
    """Run LangGraph: receive → context → LLM → optional tools → reply.

    Missing LLM / Hub must not crash the process; response.status may be
    ``degraded`` or ``error`` with clear ``errors`` entries.
    """
    try:
        result = await run_chat(body)
    except Exception as exc:  # noqa: BLE001 — last-resort guard
        logger.exception("Unhandled chat failure")
        raise HTTPException(
            status_code=500,
            detail={
                "reply": "",
                "status": "error",
                "errors": [f"unhandled agent error: {exc}"],
            },
        ) from exc
    return result


def main() -> None:
    """Run the Agent HTTP server with uvicorn (bind/port from settings)."""
    import uvicorn

    settings = get_settings()
    # Agent must start even if Hub / LLM are not ready yet.
    uvicorn.run(
        "lan_iot_agent.main:app",
        host=settings.agent.bind,
        port=settings.agent.port,
        reload=False,
    )


if __name__ == "__main__":
    main()
