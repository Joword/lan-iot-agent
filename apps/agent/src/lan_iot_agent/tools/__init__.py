"""MCP / Hub tool clients for the intelligence layer."""

from lan_iot_agent.tools.hub import (
    HubCallResult,
    HubClient,
    extract_device_list,
    extract_scene_ids,
)
from lan_iot_agent.tools.keywords import (
    DEFAULT_CLIMATE_ENTITY,
    DEFAULT_COMPANION_ID,
    DEFAULT_ROBOT_ID,
    PENDING_ACTION_SHUTDOWN_ALL,
    detect_dangerous_intent,
    entity_id_of,
    infer_tools_from_message,
    is_controllable_entity,
    resolve_climate_entity,
    resolve_companion_id,
    resolve_robot_id,
    wants_device_list,
)
from lan_iot_agent.tools.mcp import (
    KNOWN_TOOLS,
    McpClient,
    McpStubClient,
    extract_devices_from_mcp,
)
from lan_iot_agent.tools.schemas import (
    ALLOWED_LLM_TOOLS,
    allowed_llm_tools,
    hub_tool_names,
    hub_tool_schemas,
    refresh_hub_tool_schemas,
    tools_from_hub_catalog,
)

__all__ = [
    "ALLOWED_LLM_TOOLS",
    "allowed_llm_tools",
    "DEFAULT_CLIMATE_ENTITY",
    "DEFAULT_COMPANION_ID",
    "DEFAULT_ROBOT_ID",
    "KNOWN_TOOLS",
    "PENDING_ACTION_SHUTDOWN_ALL",
    "HubCallResult",
    "HubClient",
    "McpClient",
    "McpStubClient",
    "detect_dangerous_intent",
    "entity_id_of",
    "extract_device_list",
    "extract_devices_from_mcp",
    "extract_scene_ids",
    "hub_tool_names",
    "hub_tool_schemas",
    "infer_tools_from_message",
    "is_controllable_entity",
    "refresh_hub_tool_schemas",
    "resolve_climate_entity",
    "resolve_companion_id",
    "resolve_robot_id",
    "tools_from_hub_catalog",
    "wants_device_list",
]
