"""Settings loaded from config/*.toml with environment overrides."""

from __future__ import annotations

import os
import tomllib
from functools import lru_cache
from pathlib import Path
from typing import Any

from pydantic import BaseModel


def _repo_root() -> Path:
    """apps/agent/src/lan_iot_agent/settings.py → repo root (4 parents up from file)."""
    return Path(__file__).resolve().parents[4]


def _resolve_path(raw: str) -> Path:
    """Resolve a path relative to the repo root when not absolute."""
    path = Path(raw)
    if path.is_absolute():
        return path
    return _repo_root() / path


def _load_toml(path: Path) -> dict[str, Any]:
    """Load a TOML file; return {} when missing."""
    if not path.is_file():
        return {}
    with path.open("rb") as fh:
        return tomllib.load(fh)


class HubSettings(BaseModel):
    """Hub REST/MCP connection settings (URL, timeouts, retries)."""

    url: str = "http://127.0.0.1:3000"
    # Local default; Docker Compose should set HUB_MCP_URL=http://hub:3000/mcp
    mcp_url: str = "http://127.0.0.1:3000/mcp"
    timeout_seconds: float = 5.0
    max_retries: int = 3
    retry_backoff_seconds: float = 0.4


class AgentBindSettings(BaseModel):
    """HTTP bind address/port for the Agent FastAPI process."""

    bind: str = "0.0.0.0"
    port: int = 8000


class LlmSettings(BaseModel):
    """LiteLLM primary/fallback provider settings and tool-loop limits."""

    provider: str = "ollama"
    model: str = "qwen2.5:7b"
    fallback_provider: str | None = "openai"
    fallback_model: str | None = "gpt-4o-mini"
    max_tokens: int = 1024
    temperature: float = 0.3
    # Max LLM ↔ MCP tool rounds before forcing a text reply.
    max_tool_iterations: int = 5
    ollama_base_url: str = "http://ollama:11434"
    openai_base_url: str = "https://api.openai.com/v1"
    anthropic_base_url: str = "https://api.anthropic.com"


class Settings(BaseModel):
    """Root Agent settings: bind, Hub, and LLM subsections."""

    # Nested models use instance defaults (Pydantic v2 deep-copies). Avoid
    # Field(default_factory=...) so pylint/Pylance don't treat attrs as FieldInfo.
    agent: AgentBindSettings = AgentBindSettings()
    hub: HubSettings = HubSettings()
    llm: LlmSettings = LlmSettings()
    llm_config_path: str = "config/llm.toml"


def _env(name: str, default: str | None = None) -> str | None:
    """Read a non-empty environment variable, else return ``default``."""
    value = os.environ.get(name)
    if value is None or value.strip() == "":
        return default
    return value


def load_settings(
    agent_config: str | Path | None = None,
    llm_config: str | Path | None = None,
) -> Settings:
    """Load agent + llm TOML, then apply env overrides."""
    agent_path = Path(
        agent_config
        or _env("AGENT_CONFIG")
        or _resolve_path("config/agent.toml")
    )
    if not agent_path.is_absolute():
        agent_path = _resolve_path(str(agent_path))

    agent_data = _load_toml(agent_path)
    agent_section = agent_data.get("agent", {})
    hub_section = agent_data.get("hub", {})
    llm_meta = agent_data.get("llm", {})

    llm_path_raw = (
        llm_config
        or _env("LLM_CONFIG")
        or llm_meta.get("config_path")
        or "config/llm.toml"
    )
    llm_path = Path(str(llm_path_raw))
    if not llm_path.is_absolute():
        llm_path = _resolve_path(str(llm_path))
    llm_data = _load_toml(llm_path)
    llm_section = llm_data.get("llm", {})
    ollama_section = llm_data.get("ollama", {})
    openai_section = llm_data.get("openai", {})
    anthropic_section = llm_data.get("anthropic", {})

    settings = Settings(
        agent=AgentBindSettings(
            bind=str(agent_section.get("bind", "0.0.0.0")),
            port=int(agent_section.get("port", 8000)),
        ),
        hub=HubSettings(
            url=str(hub_section.get("url", "http://127.0.0.1:3000")),
            mcp_url=str(hub_section.get("mcp_url", "http://127.0.0.1:3000/mcp")),
            timeout_seconds=float(hub_section.get("timeout_seconds", 5.0)),
            max_retries=int(hub_section.get("max_retries", 3)),
            retry_backoff_seconds=float(hub_section.get("retry_backoff_seconds", 0.4)),
        ),
        llm=LlmSettings(
            provider=str(llm_section.get("provider", "ollama")),
            model=str(llm_section.get("model", "qwen2.5:7b")),
            fallback_provider=llm_section.get("fallback_provider", "openai"),
            fallback_model=llm_section.get("fallback_model", "gpt-4o-mini"),
            max_tokens=int(llm_section.get("max_tokens", 1024)),
            temperature=float(llm_section.get("temperature", 0.3)),
            max_tool_iterations=int(llm_section.get("max_tool_iterations", 5)),
            ollama_base_url=str(ollama_section.get("base_url", "http://ollama:11434")),
            openai_base_url=str(openai_section.get("base_url", "https://api.openai.com/v1")),
            anthropic_base_url=str(
                anthropic_section.get("base_url", "https://api.anthropic.com")
            ),
        ),
        llm_config_path=str(llm_path),
    )

    # Environment overrides (local-dev friendly).
    if hub_url := _env("HUB_URL"):
        settings.hub.url = hub_url.rstrip("/")
        # Keep MCP on same host unless an MCP URL env is set.
        if not (_env("HUB_MCP_URL") or _env("MCP_URL")):
            settings.hub.mcp_url = f"{settings.hub.url}/mcp"

    # HUB_MCP_URL is the preferred name; MCP_URL kept as alias.
    if mcp_url := _env("HUB_MCP_URL") or _env("MCP_URL"):
        settings.hub.mcp_url = mcp_url.rstrip("/")

    if provider := _env("LLM_PROVIDER"):
        settings.llm.provider = provider
    if model := _env("LLM_MODEL"):
        settings.llm.model = model
    if fallback_provider := _env("LLM_FALLBACK_PROVIDER"):
        settings.llm.fallback_provider = fallback_provider
    if fallback_model := _env("LLM_FALLBACK_MODEL"):
        settings.llm.fallback_model = fallback_model
    if ollama_url := _env("OLLAMA_URL"):
        settings.llm.ollama_base_url = ollama_url.rstrip("/")
    if max_iters := _env("LLM_MAX_TOOL_ITERATIONS"):
        settings.llm.max_tool_iterations = int(max_iters)

    if bind := _env("AGENT_BIND"):
        settings.agent.bind = bind
    if port := _env("AGENT_PORT"):
        settings.agent.port = int(port)

    return settings


@lru_cache(maxsize=1)
def get_settings() -> Settings:
    """Cached process-wide settings from TOML + environment."""
    return load_settings()
