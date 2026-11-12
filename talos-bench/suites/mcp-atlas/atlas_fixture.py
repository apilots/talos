"""Exercise Atlas tool discovery and read the answer from Talos's tool result."""

import json
import re
import time

from fixture import model_config as serve_fixture, tool_responses

MODEL = "mcp-atlas-fixture"
TOOL = "mcp__atlas__filesystem_read_text_file"


def respond(request):
    offered = {tool["function"]["name"] for tool in request["tools"]}
    results = tool_responses(request)
    if results:
        content = results[-1]["content"]
        value = json.loads(content).get("value")
        if isinstance(value, dict) and "activated_tools" in value:
            if TOOL in value["activated_tools"]:
                assert TOOL in offered, "Talos did not offer the activated Atlas tool"
                return TOOL, {"path": "/data/Barber Shop.csv", "head": 1}
        else:
            # Match the CSV prefix within Talos's MCP result representation.
            match = re.search(r"\b([A-Za-z]+) Gender,\1 Age,Barber_ID,", content)
            assert match, "Talos did not return the Atlas CSV header"
            return match.group(1)

        # Talos starts MCP connections asynchronously as the embedded worker starts.
        assert len(results) < 8, f"Atlas did not appear in ToolSearch: {content}"
        time.sleep(0.25)

    assert "ToolSearch" in offered, "Talos did not offer ToolSearch"
    return "ToolSearch", {"query": TOOL}


def model_config():
    return serve_fixture(MODEL, respond)
