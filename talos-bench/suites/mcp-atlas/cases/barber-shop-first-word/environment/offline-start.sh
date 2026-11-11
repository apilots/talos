#!/bin/sh
set -eu

# Use the pinned binaries already installed in the image.
uv run --offline --no-sync python - <<'PY'
import json
from pathlib import Path

path = Path("src/agent_environment/mcp_server_config.json")
config = json.loads(path.read_text())
servers = config["mcpServers"]
servers["filesystem"].update(command="/usr/bin/mcp-server-filesystem", args=["/data"])
servers["calculator"].update(command="/usr/local/bin/mcp-server-calculator", args=[])
path.write_text(json.dumps(config))
PY
exec uv run --offline --no-sync python -m uvicorn agent_environment.main:app --host "$1" --port 1984
