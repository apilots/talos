"""Complete the upstream click-test-2 episode with real Browser MCP calls."""

import json
from pathlib import Path

from fixture import model_config as fixture_model_config, tool_responses


STEPS = ("browser_navigate", "browser_state", "browser_click", "browser_state")


def respond(request):
    names = {tool["function"]["name"] for tool in request["tools"]}
    prefix = "mcp__talos_browser__"
    if prefix + STEPS[0] not in names:
        search, = (name for name in names if name.lower() == "toolsearch")
        return search, {"query": "browser", "limit": 10}
    calls = [call for message in request["messages"] for call in message.get("tool_calls", [])
             if call["function"]["name"].startswith(prefix)]
    step = len(calls)
    if step >= len(STEPS):
        return "Completed the requested button click."
    name = prefix + STEPS[step]
    assert name in names, f"Missing Browser tool: {name}"
    if STEPS[step] == "browser_navigate":
        episode = json.loads(Path("/opt/miniwob/episode-config.json").read_text())
        return name, {"url": episode["url"]}
    if STEPS[step] == "browser_click":
        content = tool_responses(request)[-1]["content"]
        decoder = json.JSONDecoder()
        for index, token in enumerate(content):
            if token != "{":
                continue
            try:
                result, _ = decoder.raw_decode(content, index)
            except json.JSONDecodeError:
                continue
            if result.get("attributes", {}).get("id") == "subbtn" and "element" in result:
                return name, {"element": result["element"]}
        raise AssertionError("The upstream ONE button did not have a Browser reference")
    return name, {}


def model_config():
    return fixture_model_config("miniwob-browser-fixture", respond)
