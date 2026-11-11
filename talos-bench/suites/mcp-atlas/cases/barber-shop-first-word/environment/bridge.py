"""Expose the controlled Atlas sandbox's read-only tool over MCP stdio."""

import asyncio
import json
import os
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

TOOL_NAME = "filesystem_read_text_file"
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
    with urlopen(request, timeout=10) as response:
        return json.load(response)


async def list_tools(
    _context: ServerRequestContext, _params: PaginatedRequestParams | None,
) -> ListToolsResult:
    tools = await asyncio.to_thread(atlas_request, "list-tools")
    # Atlas's REST response returns content blocks without structuredContent.
    return ListToolsResult(tools=[
        Tool.model_validate({**tool, "outputSchema": None})
        for tool in tools if tool["name"] == TOOL_NAME
    ])


async def call_tool(_context: ServerRequestContext, params: CallToolRequestParams) -> CallToolResult:
    try:
        if params.name != TOOL_NAME:
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
