"""P0/P1 smoke check for LanIoT Agent FastAPI app."""

from __future__ import annotations

import asyncio

from lan_iot_agent.graph.runner import run_chat
from lan_iot_agent.main import app
from lan_iot_agent.models import ChatContext, ChatRequest
from lan_iot_agent.tools.keywords import infer_tools_from_message


def main() -> None:
    """Assert routes exist, keyword inference works, and a stub chat returns a reply."""
    paths = sorted(
        p for r in app.routes if isinstance((p := getattr(r, "path", None)), str)
    )
    print("routes:", paths)
    assert "/health" in paths
    assert "/v1/chat" in paths

    tools = infer_tools_from_message("turn on light.demo")
    assert tools, "keyword stub should infer lights.control / devices.control"
    print("keyword tools:", tools)

    async def _chat() -> None:
        result = await run_chat(
            ChatRequest(
                message="hello",
                context=ChatContext(
                    devices=[{"entity_id": "light.demo", "name": "Demo", "state": "on"}]
                ),
            )
        )
        assert result.reply
        assert result.status in {"ok", "degraded", "error"}
        print("chat status:", result.status)
        print("chat reply:", result.reply[:300])
        print("errors:", result.errors)

    asyncio.run(_chat())
    print("agent-ok")


if __name__ == "__main__":
    main()
