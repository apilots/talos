#!/usr/bin/env bash
set -euo pipefail

TALOS_E2E_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${TALOS_E2E_DIR}/.." && pwd)"
TALOS_RS_DIR="${REPO_ROOT}/talos-rs"
TALOS_APP_DIR="${REPO_ROOT}/talos-app"
PORT="${TALOS_TEST_PORT:-8080}"
HEALTH_URL="http://127.0.0.1:${PORT}/health"
GATEWAY_URL="http://127.0.0.1:${PORT}"
REPORT_ROOT="${TALOS_E2E_DIR}/reports"
SCENARIO_REPORT_DIR="${TALOS_SCENARIO_REPORT_DIR:-${REPORT_ROOT}/manual}"
SERVER_LOG="${SCENARIO_REPORT_DIR}/server.log"
export TALOS_E2E_TMP_ROOT="${TALOS_E2E_TMP_ROOT:-${TALOS_E2E_DIR}/.tmp}"
E2E_TMP_ROOT="${TALOS_E2E_TMP_ROOT}"

scenario_report_path() {
  local name="$1"
  mkdir -p "${SCENARIO_REPORT_DIR}"
  printf '%s/%s\n' "${SCENARIO_REPORT_DIR}" "${name}"
}

make_e2e_temp_dir() {
  local prefix="$1"
  mkdir -p "${E2E_TMP_ROOT}"
  mktemp -d "${E2E_TMP_ROOT}/${prefix}.XXXXXX"
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
  if [[ -f "${REPO_ROOT}/.env" ]]; then
    set -a
    # shellcheck disable=SC1090
    source "${REPO_ROOT}/.env"
    set +a
  fi
}

select_e2e_model_from_json() {
  local models_json="$1"
  local preferred_models="${TALOS_E2E_PREFERRED_MODELS:-deepseek-v4-flash}"

  MODELS_JSON="${models_json}" PREFERRED_MODELS="${preferred_models}" python3 - <<'PY'
import json
import os
import sys

try:
    payload = json.loads(os.environ.get("MODELS_JSON", ""))
except Exception:
    print("")
    raise SystemExit(0)

models = payload.get("models", payload.get("data", payload)) if isinstance(payload, dict) else payload
if not isinstance(models, list):
    print("")
    raise SystemExit(0)

ids = [
    item.get("id")
    for item in models
    if isinstance(item, dict) and isinstance(item.get("id"), str) and item.get("id")
]
available = set(ids)

for model in (m.strip() for m in os.environ.get("PREFERRED_MODELS", "").split(",")):
    if model and model in available:
        print(model)
        raise SystemExit(0)

print(ids[0] if ids else "")
PY
}

select_e2e_model_provider_from_json() {
  local models_json="$1"
  local model_id="$2"

  MODELS_JSON="${models_json}" MODEL_ID="${model_id}" python3 - <<'PY'
import json
import os

try:
    payload = json.loads(os.environ.get("MODELS_JSON", ""))
except Exception:
    print("")
    raise SystemExit(0)

models = payload.get("models", payload.get("data", payload)) if isinstance(payload, dict) else payload
if not isinstance(models, list):
    print("")
    raise SystemExit(0)

model_id = os.environ.get("MODEL_ID", "")
for item in models:
    if isinstance(item, dict) and item.get("id") == model_id:
        print(item.get("provider", ""))
        raise SystemExit(0)
print("")
PY
}

default_talos_home() {
  local scenario_name="${TALOS_SCENARIO_NAME:-manual}"
  local safe_name
  safe_name="$(printf '%s' "${scenario_name}" | sed 's#[^A-Za-z0-9_.-]#-#g')"
  printf '%s/homes/%s\n' "${E2E_TMP_ROOT}" "${safe_name}"
}

prepare_talos_home() {
  local using_default_home=0
  local test_home="${TALOS_HOME:-}"
  local config_src="${TALOS_TEST_CONFIG_TEMPLATE:-${REPO_ROOT}/conf/talos-dev.yaml}"
  local env_src="${TALOS_TEST_ENV_TEMPLATE:-${REPO_ROOT}/.env}"

  if [[ -z "${test_home}" ]]; then
    using_default_home=1
    test_home="$(default_talos_home)"
  fi

  if [[ "${test_home}" == "${HOME}/.talos" ]]; then
    echo "Refusing to use default user Talos home for e2e: ${test_home}" >&2
    exit 1
  fi

  export TALOS_HOME="${test_home}"
  if [[ "${using_default_home}" == "1" ]]; then
    rm -rf "${test_home}"
  fi

  local config_dst="${test_home}/conf/talos.yaml"
  local env_dst="${test_home}/.env"
  mkdir -p "${test_home}/conf" "${test_home}/logs" "${test_home}/data"
  cp "${config_src}" "${config_dst}"
  if [[ -f "${env_src}" ]]; then
    cp "${env_src}" "${env_dst}"
  else
    : >"${env_dst}"
  fi
  chmod 600 "${env_dst}"
  apply_test_config_overrides "${config_dst}"
}

apply_test_config_overrides() {
  local config_path="$1"

CONFIG_PATH="${config_path}" python3 - <<'PY'
import os
import json
from pathlib import Path

config_path = Path(os.environ["CONFIG_PATH"])
talos_home = Path(os.environ["TALOS_HOME"])
text = config_path.read_text()

def replace_section_value(content: str, section: str, key: str, value: str) -> str:
    lines = content.splitlines(keepends=True)
    in_section = False
    for index, line in enumerate(lines):
        stripped = line.strip()
        if line.startswith(f"{section}:"):
            in_section = True
            continue
        if in_section and stripped and not line.startswith(" "):
            break
        if in_section and line.startswith(f"  {key}:"):
            newline = "\n" if line.endswith("\n") else ""
            lines[index] = f"  {key}: {value}{newline}"
            return "".join(lines)
    raise SystemExit(f"missing config key: {section}.{key}")

def replace_nested_section_value(
    content: str,
    parent: str,
    section: str,
    key: str,
    value: str,
) -> str:
    lines = content.splitlines(keepends=True)
    in_parent = False
    in_section = False
    for index, line in enumerate(lines):
        stripped = line.strip()
        indent = len(line) - len(line.lstrip(" "))
        if indent == 0 and stripped == f"{parent}:":
            in_parent = True
            in_section = False
            continue
        if in_parent and stripped and indent == 0:
            break
        if in_parent and indent == 4 and stripped == f"{section}:":
            in_section = True
            continue
        if in_section and stripped and indent <= 4:
            break
        if in_section and line.startswith(f"      {key}:"):
            newline = "\n" if line.endswith("\n") else ""
            lines[index] = f"      {key}: {value}{newline}"
            return "".join(lines)
    raise SystemExit(f"missing config key: {parent}.{section}.{key}")

def add_e2e_skill_root(content: str, talos_home: Path) -> str:
    root_path = str(talos_home / "agents-skills")
    if root_path in content:
        return content

    def root_entry(sequence_indent: int) -> str:
        item_indent = " " * sequence_indent
        field_indent = " " * (sequence_indent + 2)
        return (
            f"{item_indent}- kind: skill\n"
            f"{field_indent}path: {json.dumps(root_path)}\n"
            f"{field_indent}read_only: true\n"
        )

    plugins_marker = "    plugins:\n"
    roots_marker = "      roots:\n"

    def block_end(block_start: int, child_indent: int) -> int:
        cursor = block_start
        for line in content[block_start:].splitlines(keepends=True):
            stripped = line.strip()
            indent = len(line) - len(line.lstrip(" "))
            if stripped and indent < child_indent:
                return cursor
            cursor += len(line)
        return len(content)

    plugins_start = content.find(plugins_marker)
    if plugins_start >= 0:
        plugins_content_start = plugins_start + len(plugins_marker)
        plugins_end = block_end(plugins_content_start, 6)
        plugins_block = content[plugins_content_start:plugins_end]
        roots_start = plugins_block.find(roots_marker)
        if roots_start >= 0:
            roots_content_start = plugins_content_start + roots_start + len(roots_marker)
            insert_at = plugins_end
            sequence_indent = None
            cursor = roots_content_start
            for line in content[roots_content_start:plugins_end].splitlines(keepends=True):
                stripped = line.strip()
                indent = len(line) - len(line.lstrip(" "))
                if stripped.startswith("-") and sequence_indent is None:
                    sequence_indent = indent
                if stripped and indent == 6 and not stripped.startswith("-"):
                    insert_at = cursor
                    break
                cursor += len(line)
            return (
                content[:insert_at]
                + root_entry(sequence_indent if sequence_indent is not None else 8)
                + content[insert_at:]
            )

        insert_at = plugins_content_start
        roots_block = "      roots:\n" + root_entry(8)
        return content[:insert_at] + roots_block + content[insert_at:]

    block = (
        "    skills:\n"
        "      max_skill_md_bytes: 65536\n"
        "      max_available_summary_bytes: 8192\n"
        "    plugins:\n"
        "      sources:\n"
        "        - source_type: \"official\"\n"
        "          display_name: \"Apilots Skills\"\n"
        "          kind: \"git\"\n"
        "          url: \"https://github.com/apilots/skills\"\n"
        "          reference: \"main\"\n"
        "          sparse_paths:\n"
        "            - \"skills\"\n"
        "      roots:\n"
        f"{root_entry(8)}"
    )
    planner_start = content.find("    planner_mode:")
    if planner_start >= 0:
        return content[:planner_start] + block + content[planner_start:]
    raise SystemExit("unable to locate agent.kernel plugins block")

gateway_auth = os.environ.get("TALOS_TEST_GATEWAY_AUTH")
if gateway_auth is None:
    # Most Talos scenarios exercise local product flows. Cloud auth has a
    # dedicated scenario that opts back into real gateway auth explicitly.
    text = replace_section_value(text, "gateway", "auth", "false")
else:
    enabled = gateway_auth.lower() in ("1", "true", "yes", "on")
    text = replace_section_value(text, "gateway", "auth", "true" if enabled else "false")

gateway_port = os.environ.get("TALOS_TEST_PORT")
if gateway_port:
    text = replace_section_value(text, "gateway", "port", gateway_port)

compaction_threshold_bytes = os.environ.get("TALOS_TEST_COMPACTION_THRESHOLD_BYTES")
if compaction_threshold_bytes:
    text = replace_nested_section_value(
        text,
        "agent",
        "memory",
        "compaction_threshold_bytes",
        compaction_threshold_bytes,
    )

text = add_e2e_skill_root(text, talos_home)

cloud_api_base_url = os.environ.get("TALOS_TEST_CLOUD_API_BASE_URL")
if cloud_api_base_url:
    text = replace_section_value(
        text,
        "cloud",
        "api_base_url",
        json.dumps(cloud_api_base_url),
    )

cloud_web_base_url = os.environ.get("TALOS_TEST_CLOUD_WEB_BASE_URL")
if cloud_web_base_url:
    text = replace_section_value(
        text,
        "cloud",
        "web_base_url",
        json.dumps(cloud_web_base_url),
    )

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

has_real_llm_key() {
  [[ -n "${TALOS_LLM_ANTHROPIC_API_KEY:-}" \
    || -n "${TALOS_LLM_BAILIAN_API_KEY:-}" \
    || -n "${TALOS_LLM_DEEPSEEK_API_KEY:-}" \
    || -n "${TALOS_LLM_OPENAI_API_KEY:-}" \
    || -n "${TALOS_LLM_OPENROUTER_API_KEY:-}" ]]
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
  "${TALOS_RS_DIR}/target/debug/talos-server" >>"${SERVER_LOG}" 2>&1 &
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

build_server_binary() {
  if [[ "${TALOS_SERVER_BINARY_READY:-}" == "1" ]] \
    && [[ -x "${TALOS_RS_DIR}/target/debug/talos-server" ]]; then
    return 0
  fi

  mkdir -p "${REPORT_ROOT}"
  (cd "${TALOS_RS_DIR}" && cargo build -p talos-server >/dev/null)
  export TALOS_SERVER_BINARY_READY=1
}

build_cli_binary() {
  if [[ "${TALOS_CLI_BINARY_READY:-}" == "1" ]] \
    && [[ -x "${TALOS_RS_DIR}/target/debug/talos" ]]; then
    return 0
  fi

  mkdir -p "${REPORT_ROOT}"
  (cd "${TALOS_RS_DIR}" && cargo build -p talos-tui >/dev/null)
  export TALOS_CLI_BINARY_READY=1
}

build_binaries() {
  if [[ "${TALOS_E2E_REQUIRED_BINARIES_READY:-}" == "1" ]]; then
    return 0
  fi

  if [[ "${TALOS_BINARIES_READY:-}" == "1" ]] \
    && [[ -x "${TALOS_RS_DIR}/target/debug/talos-server" ]] \
    && [[ -x "${TALOS_RS_DIR}/target/debug/talos" ]]; then
    return 0
  fi

  build_server_binary
  build_cli_binary
  export TALOS_BINARIES_READY=1
}

build_app_server_binaries() {
  if [[ "${TALOS_APP_SERVER_BINARIES_READY:-}" == "1" ]] \
    && [[ -x "${TALOS_RS_DIR}/target/debug/app-server" ]] \
    && [[ -x "${TALOS_RS_DIR}/target/debug/talos-app-server-test-client" ]]; then
    return 0
  fi

  mkdir -p "${REPORT_ROOT}"
  (cd "${TALOS_RS_DIR}" && cargo build -p app-server -p talos-app-server-test-client >/dev/null)
  export TALOS_APP_SERVER_BINARIES_READY=1
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

json_equal() {
  local left="$1"
  local right="$2"
  LEFT_JSON="${left}" RIGHT_JSON="${right}" python3 - <<'PY'
import json
import os

try:
    left = json.loads(os.environ.get("LEFT_JSON", ""))
    right = json.loads(os.environ.get("RIGHT_JSON", ""))
except Exception:
    raise SystemExit(1)
raise SystemExit(0 if left == right else 1)
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

session_context_payload_path() {
  local session_id="$1"
  local safe_session_id
  safe_session_id="$(printf '%s' "${session_id}" | sed 's#[^A-Za-z0-9_.-]#-#g')"
  printf '%s/session-contexts/%s.json\n' "${SCENARIO_REPORT_DIR}" "${safe_session_id}"
}

new_session_id() {
  python3 - <<'PY'
import uuid

print(f"sess_{uuid.uuid4().hex}")
PY
}

remember_session_context() {
  local session_id="$1"
  local create_payload="$2"
  local payload_path
  payload_path="$(session_context_payload_path "${session_id}")"
  mkdir -p "$(dirname "${payload_path}")"
  printf '%s' "${create_payload}" > "${payload_path}"
}

message_payload_with_session_context() {
  local session_id="$1"
  local payload="$2"
  local payload_path
  payload_path="$(session_context_payload_path "${session_id}")"
  if [[ ! -f "${payload_path}" ]]; then
    MESSAGE_PAYLOAD="${payload}" SESSION_ID="${session_id}" python3 - <<'PY'
import json
import os

message_payload = json.loads(os.environ["MESSAGE_PAYLOAD"])
message_payload["session_id"] = os.environ["SESSION_ID"]
print(json.dumps(message_payload, ensure_ascii=False, separators=(",", ":")))
PY
    return 0
  fi

  MESSAGE_PAYLOAD="${payload}" SESSION_ID="${session_id}" SESSION_PAYLOAD_PATH="${payload_path}" python3 - <<'PY'
import json
import os
from pathlib import Path

message_payload = json.loads(os.environ["MESSAGE_PAYLOAD"])
session_payload = json.loads(Path(os.environ["SESSION_PAYLOAD_PATH"]).read_text())
message_payload["session_id"] = os.environ["SESSION_ID"]

for key in ("context", "modelSelection", "project_id", "current_directory"):
    if key in session_payload and message_payload.get(key) is None:
        message_payload[key] = session_payload[key]

print(json.dumps(message_payload, ensure_ascii=False, separators=(",", ":")))
PY
}

usage_total_from_json() {
  local json="$1"
  local usage_total
  usage_total="$(json_get "${json}" "usage.total_tokens")"
  if [[ -n "${usage_total}" ]]; then
    echo "${usage_total}"
    return 0
  fi

  usage_total="$(json_get "${json}" "session.usage.total_tokens")"
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
  payload="$(message_payload_with_session_context "${session_id}" "${payload}")"
  local args=(
    -sS -X POST "${GATEWAY_URL}/v1/turns/start"
    -H "Content-Type: application/json"
  )
  if [[ -n "${idempotency_key}" ]]; then
    args+=(-H "Idempotency-Key: ${idempotency_key}")
  fi
  curl "${args[@]}" -d "${payload}"
}

collect_timeline_events() {
  local session_id="$1"
  local turn_id="$2"
  local output_path="$3"
  local after_event_id="${4:-}"

  SESSION_ID="${session_id}" \
  TURN_ID="${turn_id}" \
  OUTPUT_PATH="${output_path}" \
  AFTER_EVENT_ID="${after_event_id}" \
  GATEWAY_URL="${GATEWAY_URL}" \
  python3 - <<'PY'
import json
import os
import sys
import urllib.parse
import urllib.request

session_id = os.environ["SESSION_ID"]
turn_id = os.environ["TURN_ID"]
output_path = os.environ["OUTPUT_PATH"]
after_event_id = os.environ.get("AFTER_EVENT_ID", "")
gateway_url = os.environ["GATEWAY_URL"].rstrip("/")

query = {"limit": "10000", "turn_id": turn_id}
if after_event_id:
    query["after_event_id"] = after_event_id
url = f"{gateway_url}/v1/sessions/{session_id}/events?{urllib.parse.urlencode(query)}"
request = urllib.request.Request(
    url,
    headers={
        "Accept": "text/event-stream",
    },
)

terminal_types = {"turn_completed", "turn_error"}
current = {"id": "", "event": "", "data": []}

def timeline_event(record):
    event = record.get("event")
    if isinstance(event, dict):
        return event
    return record

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
    event_record = timeline_event(record)
    if event_record.get("turn_id") != turn_id:
        raise RuntimeError(f"received event for unexpected turn: {event_record.get('turn_id')}")

    if current["id"]:
        handle.write(f"id: {current['id']}\n")
    if current["event"]:
        handle.write(f"event: {current['event']}\n")
    handle.write(f"data: {json.dumps(record, ensure_ascii=False, separators=(',', ':'))}\n\n")
    handle.flush()
    done = event_record.get("event_type") in terminal_types
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
    print(f"collect_timeline_events failed: {exc}", file=sys.stderr)
    sys.exit(1)

print(f"turn stream ended before terminal event: {turn_id}", file=sys.stderr)
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
items = {}
tool_results = {}
error_text = ""
usage = {
    "input_tokens": 0,
    "output_tokens": 0,
    "total_tokens": 0,
}
stop_reason = "end_turn"

def timeline_event(record):
    event = record.get("event")
    if isinstance(event, dict):
        return event
    return record

with open(events_path, "r", encoding="utf-8") as handle:
    for line in handle:
        if not line.startswith("data: "):
            continue
        record = timeline_event(json.loads(line[6:]))
        payload = record.get("payload") or {}
        event_type = record.get("event_type")
        request_id = payload.get("request_id", request_id)
        if event_type == "item_started":
            item = payload.get("item") or {}
            item_id = item.get("item_id", "")
            kind = item.get("kind", "")
            phase = item.get("phase", "")
            content = item.get("content") or {}
            if item_id:
                items[item_id] = {
                    "kind": kind,
                    "phase": phase,
                    "content": content,
                    "text": content.get("text", "") if content.get("type") == "text" else "",
                    "arguments": content.get("arguments", "") if content.get("type") == "tool_call" else "",
                }
            if kind == "execution_summary" and content.get("type") == "execution_summary":
                output.append({"type": "execution_summary", "text": content.get("summary", "")})
        elif event_type == "item_delta":
            item_id = payload.get("item_id", "")
            item = items.setdefault(item_id, {"kind": "", "phase": "", "content": {}, "text": "", "arguments": ""})
            delta = payload.get("delta") or {}
            delta_type = delta.get("type")
            phase = item.get("phase", "")
            if delta_type == "text":
                text = delta.get("text", "")
                item["text"] = item.get("text", "") + text
            elif delta_type == "tool_arguments":
                item["arguments"] = item.get("arguments", "") + delta.get("arguments_delta", "")
            elif delta_type == "tool_result":
                content = item.get("content") or {}
                if content.get("type") == "tool_call":
                    call_id = content.get("call_id", "")
                    tool_name = content.get("name", "")
                else:
                    call_id = ""
                    tool_name = ""
                tool_results[item_id] = {
                    "type": "tool_result",
                    "call_id": call_id,
                    "tool_name": tool_name,
                    "content": delta.get("content") or [],
                    "is_error": bool(delta.get("is_error", False)),
                }
        elif event_type == "assistant_text_committed":
            item_id = payload.get("item_id", "")
            if item_id:
                item = items.setdefault(item_id, {"kind": "assistant_text", "phase": "", "content": {}, "text": "", "arguments": ""})
                item["kind"] = item.get("kind") or "assistant_text"
                item["phase"] = payload.get("phase", item.get("phase", ""))
        elif event_type == "turn_completed":
            stop_reason = payload.get("stop_reason", stop_reason)
            usage = payload.get("usage") or usage
        elif event_type == "turn_error":
            stop_reason = "error"
            error_text = "Error: " + payload.get("error", "")

if error_text:
    output_text = error_text
else:
    output_text = "".join(
        item.get("text", "")
        for item in items.values()
        if item.get("kind") == "assistant_text" and item.get("phase") == "final_answer"
    )
    reasoning_text = "".join(
        item.get("text", "")
        for item in items.values()
        if item.get("kind") == "reasoning" or item.get("phase") == "thinking"
    )
if output_text:
    output.append({"type": "output_text", "text": output_text})

for item_id, item in items.items():
    content = item.get("content") or {}
    if item.get("kind") != "tool_call" or content.get("type") != "tool_call":
        continue
    raw_arguments = item.get("arguments", "")
    try:
        arguments = json.loads(raw_arguments) if raw_arguments else {}
    except Exception:
        arguments = raw_arguments
    output.append({
        "type": "tool_call",
        "call_id": content.get("call_id", ""),
        "name": content.get("name", ""),
        "arguments": arguments,
    })
    if item_id in tool_results:
        output.append(tool_results[item_id])

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
  local turn_id
  local events_log

  ack_json="$(send_message_ack "${session_id}" "${payload}" "${idempotency_key}")"
  message_id="$(json_get "${ack_json}" "message_id")"
  turn_id="$(json_get "${ack_json}" "turn_id")"
  if [[ -z "${message_id}" || -z "${turn_id}" ]]; then
    printf '%s\n' "${ack_json}"
    return 0
  fi

  events_log="$(scenario_report_path "${log_name}_events.log")"
  collect_timeline_events "${session_id}" "${turn_id}" "${events_log}"
  message_response_from_events "${ack_json}" "${events_log}"
}

send_message_sse_log() {
  local session_id="$1"
  local payload="$2"
  local output_path="$3"
  local idempotency_key="${4:-}"
  local ack_json
  local message_id
  local turn_id

  ack_json="$(send_message_ack "${session_id}" "${payload}" "${idempotency_key}")"
  message_id="$(json_get "${ack_json}" "message_id")"
  turn_id="$(json_get "${ack_json}" "turn_id")"
  if [[ -z "${message_id}" || -z "${turn_id}" ]]; then
    printf '%s\n' "${ack_json}" > "${output_path}"
    return 1
  fi
  collect_timeline_events "${session_id}" "${turn_id}" "${output_path}"
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

SECRET_KEY_PARTS = (
    "api_key",
    "apikey",
    "token",
    "secret",
    "password",
    "authorization",
)

def redact(value):
    if isinstance(value, dict):
        redacted = {}
        for key, child in value.items():
            normalized = str(key).lower().replace("-", "_")
            if any(part in normalized for part in SECRET_KEY_PARTS):
                redacted[key] = "<redacted>" if child is not None else None
            else:
                redacted[key] = redact(child)
        return redacted
    if isinstance(value, list):
        return [redact(item) for item in value]
    return value

record = {
    "case": case_name,
    "request": redact(parse_json(request_raw)),
    "response": redact(parse_json(response_raw)),
}
print("DETAIL_JSON " + json.dumps(record, ensure_ascii=False))
PY
  rm -f "${request_file}" "${response_file}"
}
