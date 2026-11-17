#!/usr/bin/env bash
set -euo pipefail

python3 - <<'PY'
import json
from pathlib import Path

episode = json.loads(Path("/logs/artifacts/episode.json").read_text())
transcript = json.loads(Path("/logs/agent/talos-transcript.json").read_text())
tools = [item["content"]["data"]["name"] for item in transcript if item["content"]["type"] == "toolCall"]
browser = [name for name in tools if name.startswith("mcp__talos_browser__browser_")]
unexpected = [name for name in tools if name not in browser and name not in {"ToolSearch", "TodoWrite"}]
success = episode["done"] and episode["raw_reward"] == 1 and bool(browser) and not unexpected
print(json.dumps({"episode": episode, "browserCalls": browser, "outOfProtocolTools": unexpected}))
Path("/logs/verifier/reward.txt").write_text("1\n" if success else "0\n")
PY
