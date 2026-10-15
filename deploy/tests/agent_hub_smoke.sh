#!/usr/bin/env bash
# Start only disposable local containers from published images; no build or SSH.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
mkdir -p "$root/output"
fixture="$(mktemp -d "$root/output/agent-hub-smoke.XXXXXX")"
server="talos-hub-smoke-$$"
proxy="talos-hub-proxy-$$"
image="ghcr.io/apilots/talos-server@sha256:5b5199794b9fabc06e3ba819c4e940e82ceff31a16e521d9ca6ff144c9c1f349"
cleanup() {
  docker logs "$server" > "$fixture/server.log" 2>&1 || true
  docker logs "$proxy" > "$fixture/proxy.log" 2>&1 || true
  docker rm -f "$proxy" "$server" >/dev/null 2>&1 || true
}
trap cleanup EXIT
mkdir -p "$fixture/home/conf" "$fixture/tls"
cp "$root/deploy/docker/talos-agent.yaml.example" "$fixture/home/conf/talos.yaml"
cp "$root/deploy/docker/agent-hub.compose.yml" "$fixture/"
cp "$root/deploy/docker/.hub.env.example" "$fixture/.hub.env"
printf 'TALOS_LLM_DEEPSEEK_API_KEY=unused-startup-probe\n' > "$fixture/.env"
docker compose --env-file "$fixture/.hub.env" -f "$fixture/agent-hub.compose.yml" config --quiet
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj /CN=Talos-test-CA \
  -addext 'basicConstraints=critical,CA:TRUE' \
  -keyout "$fixture/ca.key" -out "$fixture/tls/ca.crt" >/dev/null 2>&1
openssl req -new -newkey rsa:2048 -nodes -subj /CN=talos-agent.internal \
  -keyout "$fixture/tls/server.key" -out "$fixture/server.csr" >/dev/null 2>&1
printf 'basicConstraints=critical,CA:FALSE\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:talos-agent.internal,IP:127.0.0.1\n' > "$fixture/server.ext"
openssl x509 -req -in "$fixture/server.csr" -CA "$fixture/tls/ca.crt" \
  -CAkey "$fixture/ca.key" -CAcreateserial -days 1 -extfile "$fixture/server.ext" \
  -out "$fixture/tls/server.crt" >/dev/null 2>&1
# This disposable test identity is not a production secret.
chmod 644 "$fixture/tls/server.key"
docker run --rm --network none --user 0 --entrypoint sh \
  --mount "type=bind,source=$fixture/home,target=/fixture" "$image" \
  -c 'chown -R 10001:10001 /fixture'
docker run -d --name "$server" \
  --mount "type=bind,source=$fixture/home,target=/home/talos/.talos" \
  --mount "type=bind,source=$fixture/tls,target=/run/talos-tls,readonly" \
  -e TALOS_LLM_DEEPSEEK_API_KEY=unused-startup-probe \
  -p 127.0.0.1::443 "$image" >/dev/null
# Exercise the maintained agent routes with a local test identity, no ACME.
python3 - "$root" "$fixture" <<'PY'
from pathlib import Path
import sys
root, fixture = map(Path, sys.argv[1:])
config = (root / 'deploy/caddy/Caddyfile.example').read_text()
config = config.replace('agent.talos.love {', 'https://talos-agent.internal {\n\ttls /opt/talos-server/tls/server.crt /opt/talos-server/tls/server.key')
(fixture / 'Caddyfile').write_text('{\n admin off\n auto_https off\n}\n' + config)
PY
docker run -d --name "$proxy" --network "container:$server" \
  --mount "type=bind,source=$fixture/Caddyfile,target=/etc/caddy/Caddyfile,readonly" \
  --mount "type=bind,source=$fixture/tls,target=/opt/talos-server/tls,readonly" \
  caddy:2.11.7-alpine >/dev/null
port="$(docker port "$server" 443/tcp | cut -d: -f2)"
ready=0
for _ in {1..40}; do
  if curl -fsS --max-time 2 --noproxy '*' --resolve "talos-agent.internal:$port:127.0.0.1" --cacert "$fixture/tls/ca.crt" \
    "https://talos-agent.internal:$port/health" > "$fixture/health.json" 2>/dev/null; then
    ready=1
    break
  fi
  sleep 1
done
[[ "$ready" == 1 ]] || { echo 'Hub HTTPS startup failed' >&2; exit 1; }
python3 - "$fixture" <<'PY'
import json,sys
from pathlib import Path
fixture=Path(sys.argv[1])
health=json.loads((fixture/'health.json').read_text())
assert health['auth_enabled'] is True and health['mode']=='cloud',health
(fixture/'request.bin').write_bytes(bytes(5))
PY
curl --http2 -fsS --max-time 10 --noproxy '*' --resolve "talos-agent.internal:$port:127.0.0.1" --cacert "$fixture/tls/ca.crt" \
  -D "$fixture/grpc-headers.txt" -o "$fixture/grpc-body.bin" \
  -H 'content-type: application/grpc' --data-binary "@$fixture/request.bin" \
  "https://talos-agent.internal:$port/talos.agent.v1.AgentNodeControl/Register"
python3 - "$fixture" <<'PY'
from pathlib import Path
import sys
headers=(Path(sys.argv[1])/'grpc-headers.txt').read_text().lower()
assert headers.startswith('http/2 200'),headers
assert 'content-type: application/grpc' in headers,headers
assert 'grpc-status: 0' in headers,headers
PY
printf 'PASS: published hub starts with production profile; verified HTTPS + gRPC proxy route. Evidence: %s\n' "$fixture"
