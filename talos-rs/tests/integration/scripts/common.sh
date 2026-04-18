#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
PORT="${TALOS_TEST_PORT:-8080}"
HEALTH_URL="http://127.0.0.1:${PORT}/health"
GATEWAY_URL="http://127.0.0.1:${PORT}"
DEFAULT_TOKEN="talos-local-test-token"
AUTH_TOKEN="${GATEWAY_TOKEN:-$DEFAULT_TOKEN}"
REPORT_ROOT="${ROOT_DIR}/tests/integration/reports"
SCENARIO_REPORT_DIR="${TALOS_SCENARIO_REPORT_DIR:-${REPORT_ROOT}/manual}"
SERVER_LOG="${SCENARIO_REPORT_DIR}/server.log"

scenario_report_path() {
  local name="$1"
  printf '%s/%s\n' "${SCENARIO_REPORT_DIR}" "${name}"
}

find_listening_pid() {
  if ! command -v lsof >/dev/null 2>&1; then
    return 0
  fi

  lsof -t -nP -iTCP:"${PORT}" -sTCP:LISTEN | head -n 1 || true
}

wait_for_port_release() {
  local max_retry="${1:-30}"
  local retry=0
  while (( retry < max_retry )); do
    local existing_pid
    existing_pid="$(find_listening_pid)"
    if [[ -z "${existing_pid}" ]]; then
      return 0
    fi
    retry=$((retry + 1))
    sleep 1
  done
  return 1
}

terminate_pid() {
  local pid="$1"
  local signal="${2:-TERM}"

  if [[ -z "${pid}" ]]; then
    return 0
  fi
  if ! kill -0 "${pid}" >/dev/null 2>&1; then
    return 0
  fi

  kill "-${signal}" "${pid}" >/dev/null 2>&1 || true
}

cleanup_stale_server() {
  local existing_pid
  existing_pid="$(find_listening_pid)"
  if [[ -z "${existing_pid}" ]]; then
    return 0
  fi

  local existing_comm
  existing_comm="$(ps -o comm= -p "${existing_pid}" 2>/dev/null | tr -d '[:space:]' || true)"
  existing_comm="$(basename "${existing_comm}")"
  if [[ "${existing_comm}" != "talos" && "${existing_comm}" != "talos-server" ]]; then
    echo "Port ${PORT} is already in use by PID ${existing_pid}. Stop the existing server first." >&2
    exit 1
  fi

  terminate_pid "${existing_pid}" TERM
  if ! wait_for_port_release 10; then
    terminate_pid "${existing_pid}" KILL
    if ! wait_for_port_release 5; then
      echo "Port ${PORT} is still in use by stale Talos PID ${existing_pid} after forced shutdown." >&2
      exit 1
    fi
  fi
}

load_env() {
  if [[ -f "${ROOT_DIR}/.env" ]]; then
    set -a
    # shellcheck disable=SC1090
    source "${ROOT_DIR}/.env"
    set +a
  fi
}

prepare_talos_home() {
  local test_home="${TALOS_HOME:-${HOME}/.talos}"
  local config_src="${ROOT_DIR}/conf/talos.yaml"
  local config_dst="${test_home}/conf/talos.yaml"

  mkdir -p "${test_home}/conf" "${test_home}/logs" "${test_home}/data"
  cp "${config_src}" "${config_dst}"
  apply_test_config_overrides "${config_dst}"
}

apply_test_config_overrides() {
  local config_path="$1"

  CONFIG_PATH="${config_path}" python3 - <<'PY'
import os
from pathlib import Path

config_path = Path(os.environ["CONFIG_PATH"])
text = config_path.read_text()

# Integration tests should avoid planner-selector quota variance.
text = text.replace('planner_selection: "auto"', 'planner_selection: "static"', 1)

# Prefer the direct Bailian provider for deterministic tool cases when available.
if os.environ.get("BAILIAN_API_KEY", "").strip():
    text = text.replace('provider: "anthropic"', 'provider: "bailian"', 1)

config_path.write_text(text)
PY
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
  cleanup_stale_server

  mkdir -p "${SCENARIO_REPORT_DIR}"
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
    terminate_pid "${TALOS_SERVER_PID}" TERM
    if ! wait_for_port_release 10; then
      terminate_pid "${TALOS_SERVER_PID}" KILL
      wait_for_port_release 5 || true
    fi
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

  mkdir -p "${REPORT_ROOT}"
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
