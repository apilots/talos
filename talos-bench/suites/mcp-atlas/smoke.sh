#!/usr/bin/env bash
set -euo pipefail

suite_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$suite_dir/../../.." && pwd)"
image=ghcr.io/scaleapi/mcp-atlas@sha256:07f876f28aebf2a126e318bcd335bcd4b7fa1ad89702e872bb865d19a28e27c1

if [[ "$(docker version --format '{{.Server.Os}}/{{.Server.Arch}}')" != linux/arm64 ]]; then
  echo 'A native linux/arm64 Docker daemon is required.' >&2
  exit 1
fi
if ! docker image inspect "$image" >/dev/null 2>&1; then
  docker pull --platform linux/arm64 "$image"
fi
if [[ "$(docker image inspect --format '{{.Os}}/{{.Architecture}}' "$image")" != linux/arm64 ]]; then
  echo 'The sandbox image must be linux/arm64.' >&2
  exit 1
fi

mkdir -p "$repo_root/output/talos-bench"
run_dir="$(mktemp -d "$repo_root/output/talos-bench/mcp-atlas-smoke.XXXXXX")"
container="talos-${run_dir##*/}"
printf 'MCP-Atlas smoke artifacts: %s\n' "$run_dir"
printf '%s\n' "$image" > "$run_dir/image.txt"

trap 'docker logs "$container" > "$run_dir/sandbox.log" 2>&1 || true; docker rm -f "$container" >/dev/null 2>&1 || true' EXIT
SANDBOX_AUTH_TOKEN="$(python3 -c 'import secrets; print(secrets.token_hex(32))')" \
  docker run --detach --pull never --platform linux/arm64 --network none \
  --cpus .5 --memory 768m --name "$container" \
  -e SANDBOX_AUTH_TOKEN -e ENABLED_SERVERS=filesystem,calculator \
  -v "$suite_dir/cases/barber-shop-first-word/environment/offline-start.sh:/tmp/talos-atlas-offline.sh:ro" \
  "$image" sh /tmp/talos-atlas-offline.sh 127.0.0.1 > "$run_dir/container-id.txt"
docker exec "$container" uname -m > "$run_dir/architecture.txt"
docker inspect --format '{{.HostConfig.NetworkMode}}' "$container" > "$run_dir/network.txt"

request() {
  docker exec -i "$container" sh -c \
    'curl --fail-with-body --silent --show-error --max-time 10 -H "Authorization: Bearer ${SANDBOX_AUTH_TOKEN}" "$@"' \
    sh "$@"
}

ready=0
for ((attempt = 0; attempt < 20; attempt++)); do
  if request --max-time 2 http://127.0.0.1:1984/enabled-servers > "$run_dir/enabled-servers.json" 2>/dev/null; then
    ready=1
    break
  fi
  if [[ "$(docker inspect --format '{{.State.Running}}' "$container")" != true ]]; then
    break
  fi
  sleep 1
done
if [[ "$ready" != 1 ]]; then
  echo 'MCP-Atlas sandbox did not become ready. See sandbox.log.' >&2
  exit 1
fi

request -X POST http://127.0.0.1:1984/list-tools > "$run_dir/list-tools.json"
request -X POST -H 'Content-Type: application/json' --data-binary @- \
  http://127.0.0.1:1984/call-tool > "$run_dir/call-tool.json" <<'EOF'
{"tool_name":"filesystem_read_text_file","tool_args":{"path":"/data/Barber Shop.csv","head":1}}
EOF

python3 - "$run_dir" <<'PY'
import json
from pathlib import Path
import re
import sys

path = Path(sys.argv[1])
if (path / "architecture.txt").read_text().strip() != "aarch64":
    raise SystemExit("The sandbox did not report aarch64")
if (path / "network.txt").read_text().strip() != "none":
    raise SystemExit("The sandbox had a network connection")
servers = json.loads((path / "enabled-servers.json").read_text())
if servers["servers"] != [["calculator", "OK"], ["filesystem", "OK"]]:
    raise SystemExit("The expected sandbox servers were unavailable")
tools = json.loads((path / "list-tools.json").read_text())
if not any(tool["name"] == "filesystem_read_text_file" for tool in tools):
    raise SystemExit("The filesystem read tool was not advertised")
blocks = json.loads((path / "call-tool.json").read_text())
if not blocks or not re.match(r"Customer\b", blocks[0]["text"]):
    raise SystemExit("The fixture CSV did not begin with Customer")
print(f"ARM64 sandbox smoke passed. {len(tools)} tools listed. Fixture CSV begins with Customer.")
PY
