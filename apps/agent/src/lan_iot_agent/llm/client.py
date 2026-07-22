"""LiteLLM wrapper — local Ollama default with optional cloud fallback + tools."""

from __future__ import annotations

import json
import logging
from dataclasses import dataclass, field
from typing import Any

from lan_iot_agent.settings import LlmSettings, get_settings

logger = logging.getLogger(__name__)


@dataclass
class LlmToolCall:
    """Normalized tool call from a chat completion."""

    id: str
    name: str
    arguments: dict[str, Any]


@dataclass
class LlmResult:
    """Outcome of a primary (or fallback) LiteLLM completion attempt."""

    text: str
    provider: str
    model: str
    used_fallback: bool = False
    error: str | None = None
    stub: bool = False
    tool_calls: list[LlmToolCall] = field(default_factory=list)
    # OpenAI-shaped assistant message (for multi-turn tool loops).
    assistant_message: dict[str, Any] | None = None


def _litellm_model_name(provider: str, model: str) -> str:
    provider = provider.lower().strip()
    if provider == "ollama":
        if model.startswith("ollama/"):
            return model
        return f"ollama/{model}"
    if provider in {"openai", "anthropic"}:
        return model
    # Allow raw LiteLLM model strings (e.g. "openai/gpt-4o-mini").
    return model


def _api_base(settings: LlmSettings, provider: str) -> str | None:
    provider = provider.lower().strip()
    if provider == "ollama":
        return settings.ollama_base_url
    if provider == "openai":
        return settings.openai_base_url
    if provider == "anthropic":
        return settings.anthropic_base_url
    return None


def _as_dict(obj: Any) -> dict[str, Any]:
    if obj is None:
        return {}
    if isinstance(obj, dict):
        return obj
    if hasattr(obj, "model_dump"):
        try:
            dumped = obj.model_dump()
            if isinstance(dumped, dict):
                return dumped
        except Exception:  # noqa: BLE001
            pass
    if hasattr(obj, "dict"):
        try:
            dumped = obj.dict()
            if isinstance(dumped, dict):
                return dumped
        except Exception:  # noqa: BLE001
            pass
    out: dict[str, Any] = {}
    for key in ("content", "role", "tool_calls", "function_call", "name"):
        if hasattr(obj, key):
            out[key] = getattr(obj, key)
    return out


def _parse_arguments(raw: Any) -> dict[str, Any]:
    """Parse tool-call arguments from JSON string, dict, or empty."""
    if raw is None:
        return {}
    if isinstance(raw, dict):
        return dict(raw)
    if isinstance(raw, str):
        text = raw.strip()
        if not text:
            return {}
        try:
            parsed = json.loads(text)
        except json.JSONDecodeError:
            logger.warning("tool_call arguments not valid JSON: %s", text[:200])
            return {"_raw": text}
        if isinstance(parsed, dict):
            return parsed
        return {"value": parsed}
    return {"_raw": str(raw)}


def parse_tool_calls(message: Any) -> list[LlmToolCall]:
    """Robustly extract tool_calls from a LiteLLM / OpenAI message object or dict.

    Handles:
    - ``message.tool_calls`` list of objects or dicts
    - legacy ``function_call`` single object
    - missing / null content with tool_calls only
    """
    msg = _as_dict(message)
    calls: list[LlmToolCall] = []

    raw_calls = msg.get("tool_calls")
    if raw_calls is None and hasattr(message, "tool_calls"):
        raw_calls = getattr(message, "tool_calls")

    if isinstance(raw_calls, list):
        for idx, item in enumerate(raw_calls):
            call = _normalize_one_tool_call(item, idx)
            if call is not None:
                calls.append(call)

    # Legacy single function_call
    if not calls:
        fc = msg.get("function_call")
        if fc is None and hasattr(message, "function_call"):
            fc = getattr(message, "function_call")
        if fc is not None:
            call = _normalize_one_tool_call(
                {"type": "function", "id": "call_0", "function": fc},
                0,
            )
            if call is not None:
                calls.append(call)

    return calls


def _normalize_one_tool_call(item: Any, index: int) -> LlmToolCall | None:
    data = _as_dict(item)
    # Shape A: {id, type, function: {name, arguments}}
    fn = data.get("function")
    if fn is None and hasattr(item, "function"):
        fn = getattr(item, "function")
    fn_dict = _as_dict(fn) if fn is not None else {}

    name = (
        fn_dict.get("name")
        or data.get("name")
        or getattr(item, "name", None)
    )
    if not name:
        return None
    name = str(name).strip()
    if not name:
        return None

    args_raw = (
        fn_dict.get("arguments")
        if "arguments" in fn_dict
        else data.get("arguments")
    )
    if args_raw is None and fn is not None and hasattr(fn, "arguments"):
        args_raw = getattr(fn, "arguments")

    call_id = data.get("id") or getattr(item, "id", None) or f"call_{index}"
    return LlmToolCall(
        id=str(call_id),
        name=name,
        arguments=_parse_arguments(args_raw),
    )


def assistant_message_dict(
    *,
    content: str | None,
    tool_calls: list[LlmToolCall],
) -> dict[str, Any]:
    """Build an OpenAI-compatible assistant message for the next completion turn."""
    msg: dict[str, Any] = {"role": "assistant", "content": content or None}
    if tool_calls:
        msg["tool_calls"] = [
            {
                "id": tc.id,
                "type": "function",
                "function": {
                    "name": tc.name,
                    "arguments": json.dumps(tc.arguments, ensure_ascii=False),
                },
            }
            for tc in tool_calls
        ]
    return msg


def tool_result_message(tool_call_id: str, content: str) -> dict[str, Any]:
    """Build an OpenAI-compatible ``role=tool`` message for the next completion."""
    return {
        "role": "tool",
        "tool_call_id": tool_call_id,
        "content": content,
    }


def _extract_choice_message(response: Any) -> Any:
    choices = getattr(response, "choices", None)
    if choices is None and isinstance(response, dict):
        choices = response.get("choices")
    if not choices:
        raise RuntimeError("LLM response has no choices")
    choice0 = choices[0]
    message = getattr(choice0, "message", None)
    if message is None and isinstance(choice0, dict):
        message = choice0.get("message")
    if message is None:
        raise RuntimeError("LLM choice has no message")
    return message


def _completion(
    *,
    provider: str,
    model: str,
    messages: list[dict[str, Any]],
    settings: LlmSettings,
    tools: list[dict[str, Any]] | None = None,
    tool_choice: str | dict[str, Any] | None = None,
) -> tuple[str, list[LlmToolCall], dict[str, Any]]:
    """Return (text, tool_calls, assistant_message_dict)."""
    try:
        import litellm  # type: ignore  # pyright: ignore[reportMissingImports]
    except ImportError as exc:
        raise RuntimeError(
            "litellm is not installed — run: pip install -e \".[llm]\""
        ) from exc

    kwargs: dict[str, Any] = {
        "model": _litellm_model_name(provider, model),
        "messages": messages,
        "max_tokens": settings.max_tokens,
        "temperature": settings.temperature,
    }
    api_base = _api_base(settings, provider)
    if api_base:
        kwargs["api_base"] = api_base
    if tools:
        kwargs["tools"] = tools
        kwargs["tool_choice"] = tool_choice or "auto"

    response = litellm.completion(**kwargs)
    message = _extract_choice_message(response)
    msg_dict = _as_dict(message)
    content = msg_dict.get("content")
    if content is None and hasattr(message, "content"):
        content = getattr(message, "content")
    text = str(content).strip() if content else ""

    tool_calls = parse_tool_calls(message)
    assistant = assistant_message_dict(content=text or None, tool_calls=tool_calls)
    # Prefer serializing original tool_calls shape when present on the message.
    if tool_calls and isinstance(msg_dict.get("tool_calls"), list):
        # Keep our normalized assistant message (stable across providers).
        pass
    return text, tool_calls, assistant


def complete(
    messages: list[dict[str, Any]],
    settings: LlmSettings | None = None,
    *,
    tools: list[dict[str, Any]] | None = None,
    tool_choice: str | dict[str, Any] | None = None,
) -> LlmResult:
    """Call primary LLM; on failure try cloud fallback; never raise to callers.

    When ``tools`` is provided, ``tool_calls`` may be populated even if ``text``
    is empty (valid OpenAI tool-call-only assistant turn).
    """
    cfg = settings or get_settings().llm
    provider = cfg.provider
    model = cfg.model

    try:
        text, tool_calls, assistant = _completion(
            provider=provider,
            model=model,
            messages=messages,
            settings=cfg,
            tools=tools,
            tool_choice=tool_choice,
        )
        if not text and not tool_calls:
            raise RuntimeError("empty LLM response")
        return LlmResult(
            text=text,
            provider=provider,
            model=model,
            tool_calls=tool_calls,
            assistant_message=assistant,
        )
    except Exception as primary_exc:  # noqa: BLE001 — intentional soft-fail boundary
        logger.warning("Primary LLM (%s/%s) failed: %s", provider, model, primary_exc)
        primary_error = f"{provider}/{model}: {primary_exc}"

    fb_provider = (cfg.fallback_provider or "").strip()
    fb_model = (cfg.fallback_model or "").strip()
    if fb_provider and fb_model:
        try:
            text, tool_calls, assistant = _completion(
                provider=fb_provider,
                model=fb_model,
                messages=messages,
                settings=cfg,
                tools=tools,
                tool_choice=tool_choice,
            )
            if not text and not tool_calls:
                raise RuntimeError("empty fallback LLM response")
            return LlmResult(
                text=text,
                provider=fb_provider,
                model=fb_model,
                used_fallback=True,
                error=primary_error,
                tool_calls=tool_calls,
                assistant_message=assistant,
            )
        except Exception as fallback_exc:  # noqa: BLE001
            logger.warning(
                "Fallback LLM (%s/%s) failed: %s", fb_provider, fb_model, fallback_exc
            )
            return LlmResult(
                text="",
                provider=fb_provider,
                model=fb_model,
                used_fallback=True,
                error=(
                    f"{primary_error}; fallback {fb_provider}/{fb_model}: {fallback_exc}"
                ),
                stub=True,
            )

    return LlmResult(
        text="",
        provider=provider,
        model=model,
        error=primary_error,
        stub=True,
    )


def echo_stub(message: str, context_summary: str = "") -> str:
    """Deterministic reply when no LLM is reachable."""
    base = f"[stub] Echo: {message.strip()}"
    if context_summary:
        return f"{base}\n\nContext:\n{context_summary}"
    return base
