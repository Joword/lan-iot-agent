"""Test isolation for the Hub MCP tool catalog.

``build_context`` refreshes the LLM tool catalog from ``GET {HUB_MCP_URL}/tools``
and caches it in a module global. Without this file the unit suite would do real
network I/O and ``hub_tool_schemas()`` would return whatever a Hub happened to
advertise — making assertions depend on test order and on whether a Hub is up.
"""

from __future__ import annotations

from collections.abc import Iterator

import pytest

from lan_iot_agent.settings import get_settings
from lan_iot_agent.tools import schemas


@pytest.fixture(autouse=True)
def offline_tool_catalog(monkeypatch: pytest.MonkeyPatch) -> Iterator[None]:
    """Pin every test to the static catalog, no Hub, no cross-test leakage."""
    monkeypatch.setenv("HUB_TOOL_CATALOG_REFRESH", "0")
    # The degraded-path tests reach a Hub that isn't there. On Windows a refused
    # loopback connect costs ~2s, so keep retries and timeout small.
    monkeypatch.setenv("HUB_TIMEOUT_SECONDS", "0.3")
    monkeypatch.setenv("HUB_MAX_RETRIES", "1")
    get_settings.cache_clear()
    schemas.reset_hub_tool_cache()
    yield
    schemas.reset_hub_tool_cache()
    get_settings.cache_clear()
