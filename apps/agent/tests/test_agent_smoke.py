"""Lightweight tests that do not require a live LLM or Hub."""

from __future__ import annotations

import asyncio
from unittest.mock import AsyncMock, MagicMock, patch

from lan_iot_agent.context.builder import build_context_text, system_prompt
from lan_iot_agent.graph.runner import run_chat
from lan_iot_agent.llm.client import LlmResult, LlmToolCall, parse_tool_calls
from lan_iot_agent.models import ChatContext, ChatRequest
from lan_iot_agent.settings import load_settings
from lan_iot_agent.tools.hub import HubCallResult
from lan_iot_agent.tools.keywords import (
    DEFAULT_CLIMATE_ENTITY,
    DEFAULT_COMPANION_ID,
    PENDING_ACTION_SHUTDOWN_ALL,
    detect_dangerous_intent,
    infer_tools_from_message,
    is_controllable_entity,
)
from lan_iot_agent.tools.schemas import ALLOWED_LLM_TOOLS, hub_tool_schemas


def test_settings_loads() -> None:
    """Settings load from defaults with Hub MCP URL and tool-iteration limits."""
    settings = load_settings()
    assert settings.agent.port == 8000
    assert "http" in settings.hub.url
    assert settings.hub.mcp_url.endswith("/mcp") or "/mcp" in settings.hub.mcp_url
    assert settings.llm.provider
    assert settings.llm.max_tool_iterations >= 1


def test_hub_tool_schemas() -> None:
    """Hub tool schemas expose the allowed LLM tool set with object parameters."""
    tools = hub_tool_schemas()
    names = {t["function"]["name"] for t in tools}
    assert names == set(ALLOWED_LLM_TOOLS)
    assert "devices.list" in names
    assert "companion.command" in names
    for tool in tools:
        assert tool["type"] == "function"
        params = tool["function"]["parameters"]
        assert params["type"] == "object"
        assert "properties" in params


def test_system_prompt_iot_rules() -> None:
    """System prompt mentions IoT tools and requires devices.list for unknown ids."""
    text = system_prompt("Devices:\n[]")
    assert "home IoT" in text or "IoT" in text
    assert "entity" in text.lower()
    assert "devices.list" in text


def test_parse_tool_calls_openai_shape() -> None:
    """parse_tool_calls accepts OpenAI-shaped tool_calls with JSON or dict arguments."""
    message = {
        "role": "assistant",
        "content": None,
        "tool_calls": [
            {
                "id": "call_abc",
                "type": "function",
                "function": {
                    "name": "devices.list",
                    "arguments": '{"type": "light"}',
                },
            },
            {
                "id": "call_def",
                "type": "function",
                "function": {
                    "name": "climate.set",
                    "arguments": {"entity_id": "climate.demo_gree_ac", "temperature": 26},
                },
            },
        ],
    }
    calls = parse_tool_calls(message)
    assert len(calls) == 2
    assert calls[0].name == "devices.list"
    assert calls[0].arguments == {"type": "light"}
    assert calls[1].name == "climate.set"
    assert calls[1].arguments["temperature"] == 26


def test_parse_tool_calls_object_and_legacy() -> None:
    """parse_tool_calls accepts object attributes and legacy function_call."""
    fn = MagicMock()
    fn.name = "scenes.run"
    fn.arguments = '{"scene_id": "sleep_mode"}'
    tc = MagicMock()
    tc.id = "call_1"
    tc.function = fn
    msg = MagicMock()
    msg.tool_calls = [tc]
    msg.content = None
    msg.function_call = None
    calls = parse_tool_calls(msg)
    assert len(calls) == 1
    assert calls[0].name == "scenes.run"
    assert calls[0].arguments["scene_id"] == "sleep_mode"

    legacy = {"role": "assistant", "function_call": {"name": "lights.control", "arguments": "{}"}}
    legacy_calls = parse_tool_calls(legacy)
    assert len(legacy_calls) == 1
    assert legacy_calls[0].name == "lights.control"


def test_context_builder() -> None:
    """Context builder summarizes devices and scenes into the prompt snapshot."""
    built = build_context_text(
        message="turn on the light",
        devices=[{"entity_id": "light.a", "name": "A", "state": "off"}],
        scenes=["sleep_mode"],
    )
    assert built["device_count"] == 1
    assert "light.a" in built["summary"]
    assert "sleep_mode" in built["summary"]


def test_keyword_tools() -> None:
    """Keyword inference maps list/control/climate/companion phrases to Hub tools."""
    assert any(t["name"] == "devices.list" for t in infer_tools_from_message("list devices"))
    tools = infer_tools_from_message("turn on light.demo")
    assert tools
    assert tools[0]["name"] in {"lights.control", "devices.control"}
    climate = infer_tools_from_message("set climate.living to 26")
    assert climate and climate[0]["name"] == "climate.set"
    assert climate[0]["arguments"]["temperature"] == 26.0
    # Vague climate → demo entity when context empty
    ac = infer_tools_from_message("空调调到24")
    assert ac and ac[0]["name"] == "climate.set"
    assert ac[0]["arguments"]["entity_id"] == DEFAULT_CLIMATE_ENTITY
    assert ac[0]["arguments"]["temperature"] == 24.0
    # Prefer climate entity from context
    preferred = infer_tools_from_message(
        "set temperature to 22",
        devices=[{"entity_id": "climate.living", "name": "客厅空调"}],
    )
    assert preferred and preferred[0]["arguments"]["entity_id"] == "climate.living"
    cool = infer_tools_from_message("cool")
    assert cool and cool[0]["name"] == "devices.control"
    assert cool[0]["arguments"]["entity_id"] == DEFAULT_CLIMATE_ENTITY
    sleep = infer_tools_from_message("sleep mode")
    assert sleep and sleep[0]["name"] == "scenes.run"
    assert sleep[0]["arguments"]["scene_id"] == "sleep_mode"
    away = infer_tools_from_message("离家模式")
    assert away and away[0]["arguments"]["scene_id"] == "away_mode"
    scene = infer_tools_from_message("运行场景 away_mode")
    assert scene and scene[0]["name"] == "scenes.run"
    # Companion stubs → companion.command (demo_pc or context)
    for phrase, command in (
        ("ping companion", "ping"),
        ("companion", "ping"),
        ("电脑", "ping"),
        ("notify", "notify"),
        ("notify companion", "notify"),
    ):
        tools = infer_tools_from_message(phrase)
        assert tools and tools[0]["name"] == "companion.command"
        assert tools[0]["arguments"]["device_id"] == DEFAULT_COMPANION_ID
        assert tools[0]["arguments"]["command"] == command
    preferred_companion = infer_tools_from_message(
        "ping companion",
        devices=[{"id": "companion.living_pc", "kind": "companion"}],
    )
    assert preferred_companion
    assert preferred_companion[0]["arguments"]["device_id"] == "companion.living_pc"
    explicit = infer_tools_from_message("notify companion.office_pc")
    assert explicit and explicit[0]["arguments"]["device_id"] == "companion.office_pc"
    assert explicit[0]["arguments"]["command"] == "notify"


def test_dangerous_intent_detection() -> None:
    """Mass-off phrases map to shutdown_all; sensors are not controllable."""
    for phrase in ("关闭所有", "全部关闭", "turn off all", "shut everything", "TURN OFF ALL"):
        assert detect_dangerous_intent(phrase) == PENDING_ACTION_SHUTDOWN_ALL
    assert detect_dangerous_intent("turn off light.demo") is None
    assert is_controllable_entity("light.demo")
    assert is_controllable_entity("climate.demo_gree_ac")
    assert not is_controllable_entity("sensor.temp")
    assert not is_controllable_entity("binary_sensor.door")


def test_chat_degrades_without_llm() -> None:
    """Chat still returns a reply when the LLM is unavailable (stub / keyword path)."""
    async def _run() -> None:
        result = await run_chat(
            ChatRequest(
                message="hello from test",
                context=ChatContext(
                    devices=[
                        {"entity_id": "climate.living", "name": "客厅空调", "state": "cool"}
                    ]
                ),
            )
        )
        assert result.reply
        assert result.status in {"ok", "degraded", "error"}
        assert isinstance(result.errors, list)
        assert result.requires_confirmation is False
        assert result.pending_action is None

    asyncio.run(_run())


def test_shutdown_all_requires_confirmation() -> None:
    """Dangerous mass-off requires confirm before any tools run."""
    async def _run() -> None:
        result = await run_chat(
            ChatRequest(
                message="关闭所有",
                context=ChatContext(devices=[{"entity_id": "light.demo", "name": "Demo"}]),
            )
        )
        assert result.requires_confirmation is True
        assert result.pending_action == PENDING_ACTION_SHUTDOWN_ALL
        assert result.used_tools is False
        assert "confirm" in result.reply.lower() or "确认" in result.reply

    asyncio.run(_run())


def test_shutdown_all_confirmed_calls_mcp() -> None:
    """Confirmed shutdown_all turns off controllable entities via Hub MCP."""
    async def _run() -> None:
        list_result = HubCallResult(
            ok=True,
            status_code=200,
            data={
                "devices": [
                    {"entity_id": "light.demo", "name": "Demo"},
                    {"entity_id": "sensor.temp", "name": "Temp"},
                    {"entity_id": "climate.demo_gree_ac", "name": "AC"},
                ]
            },
        )
        control_result = HubCallResult(ok=True, status_code=200, data={"ok": True})

        with (
            patch(
                "lan_iot_agent.graph.nodes.McpClient.list_devices",
                new_callable=AsyncMock,
                return_value=list_result,
            ) as list_mock,
            patch(
                "lan_iot_agent.graph.nodes.McpClient.call_tool",
                new_callable=AsyncMock,
                return_value=control_result,
            ) as call_mock,
        ):
            result = await run_chat(
                ChatRequest(
                    message="turn off all",
                    confirm=True,
                    pending_action=PENDING_ACTION_SHUTDOWN_ALL,
                    context=ChatContext(devices=[{"entity_id": "light.demo"}]),
                )
            )

        assert result.requires_confirmation is False
        assert result.used_tools is True
        assert "shutdown_all" in result.reply
        list_mock.assert_awaited()
        # Only controllable entities (light + climate), not sensor
        turn_offs = [
            c
            for c in call_mock.await_args_list
            if c.args and c.args[0] == "devices.control"
        ]
        assert len(turn_offs) == 2
        entity_ids = {c.args[1]["entity_id"] for c in turn_offs}
        assert entity_ids == {"light.demo", "climate.demo_gree_ac"}

    asyncio.run(_run())


def test_llm_tool_call_loop_mocked() -> None:
    """Mock LiteLLM tool_calls → Hub MCP → final text (no live Ollama)."""

    async def _run() -> None:
        list_payload = HubCallResult(
            ok=True,
            status_code=200,
            data={
                "ok": True,
                "tool": "devices.list",
                "data": {
                    "devices": [
                        {"entity_id": "light.demo", "name": "Demo", "state": "off"},
                    ],
                    "count": 1,
                },
            },
        )

        call_1 = LlmResult(
            text="",
            provider="mock",
            model="mock",
            tool_calls=[
                LlmToolCall(id="call_list", name="devices.list", arguments={}),
            ],
            assistant_message={
                "role": "assistant",
                "content": None,
                "tool_calls": [
                    {
                        "id": "call_list",
                        "type": "function",
                        "function": {"name": "devices.list", "arguments": "{}"},
                    }
                ],
            },
        )
        call_2 = LlmResult(
            text="You have 1 light: light.demo (off).",
            provider="mock",
            model="mock",
            tool_calls=[],
            assistant_message={
                "role": "assistant",
                "content": "You have 1 light: light.demo (off).",
            },
        )
        complete_mock = MagicMock(side_effect=[call_1, call_2])

        with (
            patch("lan_iot_agent.graph.nodes.complete", complete_mock),
            patch(
                "lan_iot_agent.graph.nodes.McpClient.call_tool",
                new_callable=AsyncMock,
                return_value=list_payload,
            ) as mcp_mock,
        ):
            result = await run_chat(
                ChatRequest(
                    message="what lights do I have?",
                    context=ChatContext(
                        devices=[{"entity_id": "light.demo", "name": "Demo"}],
                        scenes=["sleep_mode"],
                    ),
                )
            )

        assert result.used_llm is True
        assert result.used_tools is True
        assert "light.demo" in result.reply
        assert result.meta.get("tool_source") in {"llm", "llm_text"}
        assert complete_mock.call_count == 2
        first_kwargs = complete_mock.call_args_list[0]
        assert first_kwargs.kwargs.get("tools") is not None
        assert any(
            t["function"]["name"] == "devices.list"
            for t in first_kwargs.kwargs["tools"]
        )
        mcp_mock.assert_awaited()
        assert mcp_mock.await_args is not None
        assert mcp_mock.await_args.args[0] == "devices.list"

    asyncio.run(_run())


def test_keyword_list_devices_still_works_when_llm_stub() -> None:
    """When LLM stubs, keyword path still runs devices.list through MCP."""
    async def _run() -> None:
        list_payload = HubCallResult(
            ok=True,
            status_code=200,
            data={
                "ok": True,
                "tool": "devices.list",
                "data": {"devices": [{"entity_id": "light.a"}], "count": 1},
            },
        )
        stub = LlmResult(
            text="",
            provider="ollama",
            model="qwen",
            stub=True,
            error="connection refused",
        )
        with (
            patch("lan_iot_agent.graph.nodes.complete", return_value=stub),
            patch(
                "lan_iot_agent.graph.nodes.McpClient.call_tool",
                new_callable=AsyncMock,
                return_value=list_payload,
            ) as mcp_mock,
        ):
            result = await run_chat(
                ChatRequest(
                    message="list devices",
                    context=ChatContext(devices=[{"entity_id": "light.a"}], scenes=[]),
                )
            )

        assert result.used_llm is False
        assert result.used_tools is True
        assert result.meta.get("tool_source") == "keywords"
        assert "devices.list" in result.reply or "light.a" in result.reply
        mcp_mock.assert_awaited_with("devices.list", {})

    asyncio.run(_run())
