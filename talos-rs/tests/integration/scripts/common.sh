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

if os.environ.get("TALOS_TEST_ENABLE_JWT_AUTH", "").strip() == "1":
    text = text.replace('token: "talos-local-test-token"', 'token: ~', 1)
    text = text.replace('jwt_secret: ${JWT_SECRET}', 'jwt_secret: "talos-test-jwt-secret"', 1)
    text = text.replace('auth: false', 'auth: true', 1)
    text = text.replace('allow_register: true', 'allow_register: true', 1)
else:
    # Most integration scenarios exercise product flows, not auth.
    # Keep gateway auth disabled unless a scenario explicitly opts into JWT auth.
    text = text.replace('auth: true', 'auth: false', 1)

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

send_message_ack() {
  local session_id="$1"
  local payload="$2"
  local idempotency_key="${3:-}"
  local args=(
    -sS -X POST "${GATEWAY_URL}/v1/sessions/${session_id}/messages"
    -H "Content-Type: application/json"
    -H "Authorization: Bearer ${AUTH_TOKEN}"
  )
  if [[ -n "${idempotency_key}" ]]; then
    args+=(-H "Idempotency-Key: ${idempotency_key}")
  fi
  curl "${args[@]}" -d "${payload}"
}

collect_message_events() {
  local session_id="$1"
  local message_id="$2"
  local output_path="$3"
  local after_event_id="${4:-}"

  SESSION_ID="${session_id}" \
  MESSAGE_ID="${message_id}" \
  OUTPUT_PATH="${output_path}" \
  AFTER_EVENT_ID="${after_event_id}" \
  GATEWAY_URL="${GATEWAY_URL}" \
  AUTH_TOKEN="${AUTH_TOKEN}" \
  python3 - <<'PY'
import json
import os
import sys
import urllib.parse
import urllib.request

session_id = os.environ["SESSION_ID"]
message_id = os.environ["MESSAGE_ID"]
output_path = os.environ["OUTPUT_PATH"]
after_event_id = os.environ.get("AFTER_EVENT_ID", "")
gateway_url = os.environ["GATEWAY_URL"].rstrip("/")
auth_token = os.environ["AUTH_TOKEN"]

query = {"limit": "10000"}
if after_event_id:
    query["after_event_id"] = after_event_id
url = f"{gateway_url}/v1/sessions/{session_id}/events?{urllib.parse.urlencode(query)}"
request = urllib.request.Request(
    url,
    headers={
        "Accept": "text/event-stream",
        "Authorization": f"Bearer {auth_token}",
    },
)

terminal_types = {"message_completed", "message_error"}
current = {"id": "", "event": "", "data": []}

def reset():
    current["id"] = ""
    current["event"] = ""
    current["data"] = []

def flush(handle):
    if not current["data"]:
        reset()
        return False
    raw_data = "\n".join(current["data"])
    try:
        record = json.loads(raw_data)
    except Exception:
        reset()
        return False
    if record.get("message_id") != message_id:
        reset()
        return False

    if current["id"]:
        handle.write(f"id: {current['id']}\n")
    if current["event"]:
        handle.write(f"event: {current['event']}\n")
    handle.write(f"data: {json.dumps(record, ensure_ascii=False, separators=(',', ':'))}\n\n")
    handle.flush()
    done = record.get("event_type") in terminal_types
    reset()
    return done

try:
    with urllib.request.urlopen(request, timeout=180) as response, open(output_path, "w", encoding="utf-8") as handle:
        for raw_line in response:
            line = raw_line.decode("utf-8").rstrip("\r\n")
            if line == "":
                if flush(handle):
                    sys.exit(0)
                continue
            if line.startswith("id:"):
                current["id"] = line[3:].strip()
            elif line.startswith("event:"):
                current["event"] = line[6:].strip()
            elif line.startswith("data:"):
                current["data"].append(line[5:].strip())
except Exception as exc:
    print(f"collect_message_events failed: {exc}", file=sys.stderr)
    sys.exit(1)

print(f"message stream ended before terminal event: {message_id}", file=sys.stderr)
sys.exit(1)
PY
}

message_response_from_events() {
  local ack_json="$1"
  local events_path="$2"
  ACK_JSON="${ack_json}" EVENTS_PATH="${events_path}" python3 - <<'PY'
import json
import os

ack = json.loads(os.environ["ACK_JSON"])
events_path = os.environ["EVENTS_PATH"]

request_id = ack.get("request_id", "")
model = ""
output_text = ""
reasoning_text = ""
output = []
usage = {
    "input_tokens": 0,
    "output_tokens": 0,
    "total_tokens": 0,
}
stop_reason = "end_turn"

with open(events_path, "r", encoding="utf-8") as handle:
    for line in handle:
        if not line.startswith("data: "):
            continue
        record = json.loads(line[6:])
        payload = record.get("payload") or {}
        event_type = record.get("event_type")
        request_id = payload.get("request_id", request_id)
        if event_type == "message_started":
            model = payload.get("model", model)
        elif event_type == "message_reasoning_delta":
            reasoning_text += payload.get("delta", "")
        elif event_type == "message_execution_summary":
            output.append({"type": "execution_summary", "text": payload.get("text", "")})
        elif event_type == "message_output_delta":
            output_text += payload.get("delta", "")
        elif event_type == "message_tool_call_started":
            output.append({
                "type": "tool_call",
                "call_id": payload.get("call_id", ""),
                "name": payload.get("name", ""),
                "arguments": payload.get("arguments") or {},
            })
        elif event_type == "message_tool_call_completed":
            output.append({
                "type": "tool_result",
                "call_id": payload.get("call_id", ""),
                "tool_name": payload.get("tool", ""),
                "content": payload.get("content") or [],
                "is_error": bool(payload.get("is_error", False)),
            })
        elif event_type == "message_completed":
            stop_reason = payload.get("stop_reason", stop_reason)
            usage = payload.get("usage") or usage
        elif event_type == "message_error":
            stop_reason = "error"
            output_text = "Error: " + payload.get("error", "")

if output_text:
    output.append({"type": "output_text", "text": output_text})

response = {
    "request_id": request_id,
    "message_id": ack.get("message_id", ""),
    "session_id": ack.get("session_id", ""),
    "model": model,
    "output": output,
    "reasoning": [{"text": reasoning_text}] if reasoning_text else [],
    "stop_reason": stop_reason,
    "usage": usage,
}
print(json.dumps(response, ensure_ascii=False, separators=(",", ":")))
PY
}

send_message_response() {
  local session_id="$1"
  local payload="$2"
  local log_name="$3"
  local idempotency_key="${4:-}"
  local ack_json
  local message_id
  local events_log

  ack_json="$(send_message_ack "${session_id}" "${payload}" "${idempotency_key}")"
  message_id="$(json_get "${ack_json}" "message_id")"
  if [[ -z "${message_id}" ]]; then
    printf '%s\n' "${ack_json}"
    return 0
  fi

  events_log="$(scenario_report_path "${log_name}_events.log")"
  collect_message_events "${session_id}" "${message_id}" "${events_log}"
  message_response_from_events "${ack_json}" "${events_log}"
}

send_message_sse_log() {
  local session_id="$1"
  local payload="$2"
  local output_path="$3"
  local idempotency_key="${4:-}"
  local ack_json
  local message_id

  ack_json="$(send_message_ack "${session_id}" "${payload}" "${idempotency_key}")"
  message_id="$(json_get "${ack_json}" "message_id")"
  if [[ -z "${message_id}" ]]; then
    printf '%s\n' "${ack_json}" > "${output_path}"
    return 1
  fi
  collect_message_events "${session_id}" "${message_id}" "${output_path}"
}

log_case_json() {
  local case_name="$1"
  local request_payload="${2:-}"
  local response_payload="${3:-}"
  local request_file
  local response_file
  request_file="$(mktemp)"
  response_file="$(mktemp)"
  printf '%s' "${request_payload}" > "${request_file}"
  printf '%s' "${response_payload}" > "${response_file}"
  CASE_NAME="$case_name" REQUEST_JSON_FILE="${request_file}" RESPONSE_JSON_FILE="${response_file}" python3 - <<'PY'
import json
import os

case_name = os.environ.get("CASE_NAME", "")
request_file = os.environ.get("REQUEST_JSON_FILE", "")
response_file = os.environ.get("RESPONSE_JSON_FILE", "")

def load_text(path: str) -> str:
    if not path:
        return ""
    try:
        with open(path, "r", encoding="utf-8") as handle:
            return handle.read()
    except Exception:
        return ""

request_raw = load_text(request_file)
response_raw = load_text(response_file)

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
  rm -f "${request_file}" "${response_file}"
}
