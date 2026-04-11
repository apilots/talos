#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
PORT="${TALOS_TEST_PORT:-8080}"
HEALTH_URL="http://127.0.0.1:${PORT}/health"
GATEWAY_URL="http://127.0.0.1:${PORT}"
DEFAULT_TOKEN="talos-local-test-token"
AUTH_TOKEN="${GATEWAY_TOKEN:-$DEFAULT_TOKEN}"
SERVER_LOG="${ROOT_DIR}/tests/integration/reports/server.log"

load_env() {
  if [[ -f "${ROOT_DIR}/.env" ]]; then
    set -a
    # shellcheck disable=SC1090
    source "${ROOT_DIR}/.env"
    set +a
  fi
}

prepare_talos_home() {
  local test_home="${HOME}/.talos"
  local config_src="${ROOT_DIR}/conf/talos.yaml"
  local config_dst="${test_home}/conf/talos.yaml"

  mkdir -p "${test_home}/conf" "${test_home}/logs" "${test_home}/data"
  if [[ ! -f "${config_dst}" ]]; then
    cp "${config_src}" "${config_dst}"
  fi
}

require_command() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "Missing required command: ${cmd}" >&2
    exit 1
  fi
}

wait_for_health() {
  local max_retry="${1:-60}"
  local retry=0
  while (( retry < max_retry )); do
    if [[ -n "${TALOS_SERVER_PID:-}" ]] && ! kill -0 "${TALOS_SERVER_PID}" >/dev/null 2>&1; then
      return 1
    fi
    if curl -sS "${HEALTH_URL}" >/dev/null 2>&1; then
      return 0
    fi
    retry=$((retry + 1))
    sleep 1
  done
  return 1
}

start_server() {
  if command -v lsof >/dev/null 2>&1; then
    local existing_pid
    existing_pid="$(lsof -t -nP -iTCP:"${PORT}" -sTCP:LISTEN | head -n 1 || true)"
    if [[ -n "${existing_pid}" ]]; then
      echo "Port ${PORT} is already in use by PID ${existing_pid}. Stop the existing server first." >&2
      exit 1
    fi
  fi

  mkdir -p "${ROOT_DIR}/tests/integration/reports"
  prepare_talos_home
  : > "${SERVER_LOG}"
  "${ROOT_DIR}/target/debug/talos" >>"${SERVER_LOG}" 2>&1 &
  export TALOS_SERVER_PID=$!
  if ! wait_for_health 90; then
    echo "Server failed to start. See log: ${SERVER_LOG}" >&2
    stop_server
    exit 1
  fi
}

stop_server() {
  if [[ -n "${TALOS_SERVER_PID:-}" ]]; then
    kill "${TALOS_SERVER_PID}" >/dev/null 2>&1 || true
    wait "${TALOS_SERVER_PID}" 2>/dev/null || true
    unset TALOS_SERVER_PID
  fi
}

build_binaries() {
  if [[ "${TALOS_BINARIES_READY:-}" == "1" ]] \
    && [[ -x "${ROOT_DIR}/target/debug/talos" ]] \
    && [[ -x "${ROOT_DIR}/target/debug/talos-client" ]] \
    && [[ -x "${ROOT_DIR}/target/debug/talos-ui-tui" ]]; then
    return 0
  fi

  mkdir -p "${ROOT_DIR}/tests/integration/reports"
  (cd "${ROOT_DIR}" && cargo build -p talos -p talos-cli -p talos-ui-tui >/dev/null)
  export TALOS_BINARIES_READY=1
}

json_get() {
  local json="$1"
  local key="$2"
  JSON_INPUT="$json" python3 - "$key" <<'PY'
import json
import os
import sys

key = sys.argv[1]
raw = os.environ.get("JSON_INPUT", "")
if not raw:
    print("")
    sys.exit(0)

try:
    data = json.loads(raw)
except Exception:
    print("")
    sys.exit(0)

node=data
for part in key.split("."):
    if isinstance(node, dict):
        node=node.get(part)
    else:
        node=None
        break
if node is None:
    print("")
elif isinstance(node, (dict,list)):
    print(json.dumps(node))
else:
    print(node)
PY
}

session_id_from_json() {
  local json="$1"
  local session_id
  session_id="$(json_get "${json}" "context.session_id")"
  if [[ -n "${session_id}" ]]; then
    echo "${session_id}"
    return 0
  fi

  json_get "${json}" "id"
}

usage_total_from_json() {
  local json="$1"
  local usage_total
  usage_total="$(json_get "${json}" "usage.total_tokens")"
  if [[ -n "${usage_total}" ]]; then
    echo "${usage_total}"
    return 0
  fi

  json_get "${json}" "state.usage.total_tokens"
}

log_case_json() {
  local case_name="$1"
  local request_payload="${2:-}"
  local response_payload="${3:-}"
  CASE_NAME="$case_name" REQUEST_JSON="$request_payload" RESPONSE_JSON="$response_payload" python3 - <<'PY'
import json
import os

case_name = os.environ.get("CASE_NAME", "")
request_raw = os.environ.get("REQUEST_JSON", "")
response_raw = os.environ.get("RESPONSE_JSON", "")

def parse_json(raw: str):
    if not raw:
        return None
    try:
        return json.loads(raw)
    except Exception:
        return {"raw": raw}

record = {
    "case": case_name,
    "request": parse_json(request_raw),
    "response": parse_json(response_raw),
}
print("DETAIL_JSON " + json.dumps(record, ensure_ascii=False))
PY
}
