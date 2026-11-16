#!/bin/sh
set -eu

# Use the pinned binaries already installed in the image.
uv run --offline --no-sync python - <<'PY'
import json
import os
from pathlib import Path

path = Path("src/agent_environment/mcp_server_config.json")
config = json.loads(path.read_text())
commands = {
    "calculator": ["mcp-server-calculator"],
    "ddg-search": ["duckduckgo-mcp-server"],
    "fetch": ["mcp-server-fetch"],
    "filesystem": ["mcp-server-filesystem", "/data"],
    "git": ["mcp-server-git"],
    "open-library": ["mcp-open-library"],
    "osm-mcp-server": ["osm-mcp-server"],
    "wikipedia": ["wikipedia-mcp"],
}
servers = {}
for name in os.environ["ENABLED_SERVERS"].split(","):
    command, *args = commands[name]
    servers[name] = {**config["mcpServers"][name], "command": command, "args": args}
config["mcpServers"] = servers
path.write_text(json.dumps(config))
PY
exec uv run --offline --no-sync python -m uvicorn agent_environment.main:app --host "$1" --port 1984
