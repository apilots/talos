#!/usr/bin/env bash
# Render published deployment examples only; never start containers or use SSH.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
mkdir -p "$root/output"
fixture="$(mktemp -d "$root/output/compose-examples.XXXXXX")"
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/server" "$fixture/worker"
cp "$root/deploy/docker/talos-server/.env.example" "$fixture/server/.env"
cp "$root/deploy/docker/talos-server/docker-compose.yml" "$fixture/server/"
cp "$root/deploy/docker/talos-server/compose.production.yml" "$fixture/server/"
cp "$root/deploy/docker/talos-worker/.env.example" "$fixture/worker/.env"
cp "$root/deploy/docker/talos-worker/docker-compose.yml" "$fixture/worker/"
(cd "$fixture/server" && docker compose config --format json) > "$fixture/basic.json"
(cd "$fixture/server" && docker compose -f compose.production.yml config --format json) > "$fixture/production.json"
# This is an isolated rendering identity, never registered or started.
(cd "$fixture/worker" && TALOS_WORKER_NODE_ID=00000000-0000-0000-0000-000000000001 \
  docker compose config --format json) > "$fixture/worker.json"
python3 - "$fixture" <<'PY'
import json
from pathlib import Path
import sys

fixture = Path(sys.argv[1])
basic = json.loads((fixture / "basic.json").read_text())
production = json.loads((fixture / "production.json").read_text())
worker = json.loads((fixture / "worker.json").read_text())
for config, product in [(basic, "server"), (production, "server"), (worker, "worker")]:
    service_name = "talos-" + product
    assert set(config["services"]) == {service_name}
    service = config["services"][service_name]
    assert service["image"].startswith("ghcr.io/apilots/" + service_name + ":v")
    assert not any(key.startswith("TALOS_LLM_") for key in service["environment"])
service = production["services"]["talos-server"]
assert production["volumes"]["talos-data"]["external"]
assert production["volumes"]["talos-data"]["name"] == "talos-server_talos-data"
assert {port["target"] for port in service["ports"]} == {8080, 9443}
tls = next(mount for mount in service["volumes"] if mount["target"] == "/run/talos-tls")
assert tls["read_only"] and not tls.get("bind", {}).get("create_host_path", False)
assert service["environment"]["TALOS_HOME"] == "/home/talos/.talos"
service = worker["services"]["talos-worker"]
assert not service.get("ports") and not service.get("healthcheck")
assert service["environment"]["TALOS_WORKER_NODE_ID"] == "00000000-0000-0000-0000-000000000001"
assert len(service["volumes"]) == 1
home = service["volumes"][0]
assert home["type"] == "bind" and home["target"] == "/home/talos/.talos"
assert not home.get("bind", {}).get("create_host_path", False)
print("PASS: Server basic/production and outbound Worker Compose examples; optional provider keys remain omitted.")
PY
