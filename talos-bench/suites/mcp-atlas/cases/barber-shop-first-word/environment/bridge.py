"""Expose the published Atlas task's tool menu over MCP stdio."""

import asyncio
import json
import os
from pathlib import Path
from urllib.request import Request, urlopen

from mcp.server import Server, ServerRequestContext
from mcp.server.stdio import stdio_server
from mcp.types import (
    CallToolRequestParams,
    CallToolResult,
    ContentBlock,
    ListToolsResult,
    PaginatedRequestParams,
    TextContent,
    Tool,
)
from pydantic import TypeAdapter

TOOL_NAMES = frozenset(json.loads(Path(__file__).with_name("enabled-tools.json").read_text()))
content_blocks = TypeAdapter(list[ContentBlock])


def atlas_request(endpoint, payload=None):
    request = Request(
        f"{os.environ['ATLAS_URL']}/{endpoint}",
        data=None if payload is None else json.dumps(payload).encode(),
        headers={
            "Authorization": f"Bearer {os.environ['SANDBOX_AUTH_TOKEN']}",
            "Content-Type": "application/json",
        },
        method="POST",
    )
    with urlopen(request, timeout=60) as response:
        return json.load(response)


async def list_tools(
    _context: ServerRequestContext, _params: PaginatedRequestParams | None,
) -> ListToolsResult:
    tools = await asyncio.to_thread(atlas_request, "list-tools")
    tools_by_name = {tool["name"]: tool for tool in tools}
    missing = TOOL_NAMES - tools_by_name.keys()
    if missing:
        raise ValueError(f"Atlas sandbox is missing task tools: {', '.join(sorted(missing))}")
    # Atlas's REST response returns content blocks without structuredContent.
    return ListToolsResult(tools=[
        Tool.model_validate({**tools_by_name[name], "outputSchema": None})
        for name in sorted(TOOL_NAMES)
    ])


async def call_tool(_context: ServerRequestContext, params: CallToolRequestParams) -> CallToolResult:
    try:
        if params.name not in TOOL_NAMES:
            raise ValueError(f"Unexpected Atlas tool: {params.name}")
        result = await asyncio.to_thread(
            atlas_request, "call-tool", {"tool_name": params.name, "tool_args": params.arguments or {}},
        )
        return CallToolResult(content=content_blocks.validate_python(result))
    except (OSError, ValueError) as error:
        return CallToolResult(content=[TextContent(type="text", text=str(error))], is_error=True)


async def main():
    server = Server("atlas", on_list_tools=list_tools, on_call_tool=call_tool)
    async with stdio_server() as (read, write):
        await server.run(read, write, server.create_initialization_options())


if __name__ == "__main__":
    asyncio.run(main())
