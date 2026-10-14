#!/usr/bin/env bash
set -euo pipefail

TALOS_E2E_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${TALOS_E2E_DIR}/.." && pwd)"
TALOS_RS_DIR="${REPO_ROOT}/talos-rs"
TALOS_APP_DIR="${REPO_ROOT}/talos-app"
export TALOS_RESOURCE_DIR="${REPO_ROOT}"
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

if not (isinstance(payload, dict) and payload.get("status") == "success" and isinstance(payload.get("data"), dict)):
    print("")
    raise SystemExit(0)
models = payload["data"].get("models")
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

if not (isinstance(payload, dict) and payload.get("status") == "success" and isinstance(payload.get("data"), dict)):
    print("")
    raise SystemExit(0)
models = payload["data"].get("models")
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

select_e2e_model_from_cli_json() {
  local models_json="$1"
  local preferred_models="${TALOS_E2E_PREFERRED_MODELS:-deepseek-v4-flash}"

  MODELS_JSON="${models_json}" PREFERRED_MODELS="${preferred_models}" python3 - <<'PY'
import json
import os

try:
    payload = json.loads(os.environ.get("MODELS_JSON", ""))
except Exception:
    print("")
    raise SystemExit(0)

models = payload.get("models") if isinstance(payload, dict) else payload
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

select_e2e_model_provider_from_cli_json() {
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

models = payload.get("models") if isinstance(payload, dict) else payload
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
        if in_parent and indent == 2 and stripped == f"{section}:":
            in_section = True
            continue
        if in_section and stripped and indent <= 2:
            break
        if in_section and line.startswith(f"    {key}:"):
            newline = "\n" if line.endswith("\n") else ""
            lines[index] = f"    {key}: {value}{newline}"
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

context_compaction_max_bytes = os.environ.get("TALOS_TEST_CONTEXT_COMPACTION_MAX_BYTES")
if context_compaction_max_bytes:
    text = replace_nested_section_value(
        text,
        "agent",
        "kernel",
        "context_compaction_max_bytes",
        context_compaction_max_bytes,
    )

inner_model_provider = os.environ.get("TALOS_TEST_INNER_MODEL_PROVIDER")
inner_model_id = os.environ.get("TALOS_TEST_INNER_MODEL_ID")
if bool(inner_model_provider) != bool(inner_model_id):
    raise SystemExit(
        "TALOS_TEST_INNER_MODEL_PROVIDER and TALOS_TEST_INNER_MODEL_ID must be set together"
    )
if inner_model_provider and inner_model_id:
    text = replace_nested_section_value(
        text,
        "agent",
        "inner_model",
        "provider",
        json.dumps(inner_model_provider),
    )
    text = replace_nested_section_value(
        text,
        "agent",
        "inner_model",
        "id",
        json.dumps(inner_model_id),
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

resolve_cloud_server_binary() {
  local requested="${TALOS_CLOUD_SERVER_BIN:-cloud-server}"
  local resolved
  if [[ "${requested}" == */* ]]; then
    [[ -x "${requested}" ]] || {
      echo "Cloud auth fixture binary is not executable: ${requested}" >&2
      return 1
    }
    resolved="$(cd "$(dirname "${requested}")" && pwd)/$(basename "${requested}")"
  else
    resolved="$(command -v "${requested}" || true)"
    [[ -n "${resolved}" ]] || {
      echo "Cloud auth fixture binary '${requested}' was not found. Set TALOS_CLOUD_SERVER_BIN to its executable path." >&2
      return 1
    }
  fi
  printf '%s\n' "${resolved}"
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
  TALOS_RESOURCE_DIR="${REPO_ROOT}" \
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

build_worker_binary() {
  if [[ "${TALOS_WORKER_BINARY_READY:-}" == "1" ]] \
    && [[ -x "${TALOS_RS_DIR}/target/debug/talos-worker" ]]; then
    return 0
  fi

  mkdir -p "${REPORT_ROOT}"
  (cd "${TALOS_RS_DIR}" && cargo build -p talos-worker-bin >/dev/null)
  export TALOS_WORKER_BINARY_READY=1
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

api_data_json() {
  local json="$1"
  printf '%s' "$json" | python3 -c '
import json
import sys

raw = sys.stdin.read()
try:
    data = json.loads(raw)
except Exception as exc:
    print(f"invalid JSON response: {exc}", file=sys.stderr)
    sys.exit(1)

if not (isinstance(data, dict) and data.get("status") == "success" and "data" in data):
    print(f"expected ApiResponse success envelope, got: {raw}", file=sys.stderr)
    sys.exit(1)

print(json.dumps(data["data"], ensure_ascii=False, separators=(",", ":")))
'
}

json_get() {
  local json="$1"
  local key="$2"
  printf '%s' "$json" | python3 -c '
import json
import sys

key = sys.argv[1]
raw = sys.stdin.read()
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
' "$key"
}

api_json_get() {
  local json="$1"
  local key="$2"
  local data_json
  data_json="$(api_data_json "${json}")"
  json_get "${data_json}" "${key}"
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
  json_get "${json}" "id"
}

session_start_defaults_path() {
  local session_id="$1"
  local safe_session_id
  safe_session_id="$(printf '%s' "${session_id}" | sed 's#[^A-Za-z0-9_.-]#-#g')"
  printf '%s/session-start-defaults/%s.json\n' "${SCENARIO_REPORT_DIR}" "${safe_session_id}"
}

new_session_id() {
  python3 - <<'PY'
import uuid

print(f"sess_{uuid.uuid4().hex}")
PY
}

remember_session_start_defaults() {
  local session_id="$1"
  local defaults="$2"
  local payload_path
  payload_path="$(session_start_defaults_path "${session_id}")"
  mkdir -p "$(dirname "${payload_path}")"
  printf '%s' "${defaults}" > "${payload_path}"
}

message_payload_with_session_defaults() {
  local session_id="$1"
  local payload="$2"
  local payload_path
  payload_path="$(session_start_defaults_path "${session_id}")"
  if [[ ! -f "${payload_path}" ]]; then
    MESSAGE_PAYLOAD="${payload}" SESSION_ID="${session_id}" python3 - <<'PY'
import json
import os

message_payload = json.loads(os.environ["MESSAGE_PAYLOAD"])
message_payload["sessionId"] = os.environ["SESSION_ID"]
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
message_payload["sessionId"] = os.environ["SESSION_ID"]

for key in ("modelSelection", "userPermissionMode", "projectId", "currentDirectory", "clientRequestId"):
    if key in session_payload and message_payload.get(key) is None:
        message_payload[key] = session_payload[key]

print(json.dumps(message_payload, ensure_ascii=False, separators=(",", ":")))
PY
}

usage_total_from_json() {
  local json="$1"
  local usage_total
  usage_total="$(json_get "${json}" "usage.tokens.total_tokens")"
  if [[ -n "${usage_total}" ]]; then
    echo "${usage_total}"
    return 0
  fi

  usage_total="$(json_get "${json}" "usage.total_tokens")"
  if [[ -n "${usage_total}" ]]; then
    echo "${usage_total}"
    return 0
  fi

  usage_total="$(json_get "${json}" "session.usage.tokens.total_tokens")"
  if [[ -n "${usage_total}" ]]; then
    echo "${usage_total}"
    return 0
  fi

  usage_total="$(json_get "${json}" "session.usage.total_tokens")"
  if [[ -n "${usage_total}" ]]; then
    echo "${usage_total}"
    return 0
  fi

  usage_total="$(json_get "${json}" "state.usage.tokens.total_tokens")"
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
  payload="$(message_payload_with_session_defaults "${session_id}" "${payload}")"
  local args=(
    -sS -X POST "${GATEWAY_URL}/v1/turns/start"
    -H "Content-Type: application/json"
  )
  if [[ -n "${idempotency_key}" ]]; then
    args+=(-H "Idempotency-Key: ${idempotency_key}")
  fi
  if [[ -n "${TALOS_E2E_BEARER_TOKEN:-}" ]]; then
    args+=(-H "Authorization: Bearer ${TALOS_E2E_BEARER_TOKEN}")
  fi
  curl "${args[@]}" -d "${payload}"
}

collect_timeline_events() {
  local session_id="$1"
  local turn_id="$2"
  local output_path="$3"
  local after_event_id="${4:-}"
  local stop_payload_types="${5:-turnCompleted,turnFailed}"

  SESSION_ID="${session_id}" \
  TURN_ID="${turn_id}" \
  OUTPUT_PATH="${output_path}" \
  AFTER_EVENT_ID="${after_event_id}" \
  STOP_PAYLOAD_TYPES="${stop_payload_types}" \
  GATEWAY_URL="${GATEWAY_URL}" \
  TALOS_E2E_BEARER_TOKEN="${TALOS_E2E_BEARER_TOKEN:-}" \
  python3 - <<'PY'
import json
import os
import sys
import time
import urllib.parse
import urllib.request

session_id = os.environ["SESSION_ID"]
turn_id = os.environ["TURN_ID"]
output_path = os.environ["OUTPUT_PATH"]
after_event_id = os.environ.get("AFTER_EVENT_ID", "")
stop_payload_types = {
    value for value in os.environ["STOP_PAYLOAD_TYPES"].split(",") if value
}
gateway_url = os.environ["GATEWAY_URL"].rstrip("/")

query = {"limit": "10000", "turn_id": turn_id}
if after_event_id:
    query["after_event_id"] = after_event_id
url = f"{gateway_url}/v1/sessions/{session_id}/events?{urllib.parse.urlencode(query)}"
request = urllib.request.Request(
    url,
    headers={
        "Accept": "text/event-stream",
        **(
            {"Authorization": f"Bearer {os.environ['TALOS_E2E_BEARER_TOKEN']}"}
            if os.environ.get("TALOS_E2E_BEARER_TOKEN")
            else {}
        ),
    },
)

deadline = time.monotonic() + 180
current = {"id": "", "event": "", "data": []}

def agent_event(record):
    event = record.get("event")
    return event.get("data", {}) if isinstance(event, dict) and event.get("type") == "agent" else {}

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
    event_record = agent_event(record)
    if not event_record:
        reset()
        return False
    identity = event_record.get("identity") or {}
    if identity.get("turnId") != turn_id:
        raise RuntimeError(f"received event for unexpected turn: {identity.get('turnId')}")

    if current["id"]:
        handle.write(f"id: {current['id']}\n")
    if current["event"]:
        handle.write(f"event: {current['event']}\n")
    handle.write(f"data: {json.dumps(record, ensure_ascii=False, separators=(',', ':'))}\n\n")
    handle.flush()
    payload = event_record.get("payload") or {}
    done = payload.get("type") in stop_payload_types
    reset()
    return done

try:
    with urllib.request.urlopen(request, timeout=180) as response, open(output_path, "w", encoding="utf-8") as handle:
        for raw_line in response:
            if time.monotonic() >= deadline:
                raise TimeoutError(f"timed out collecting timeline events for turn {turn_id}")
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
if not (isinstance(ack, dict) and ack.get("status") == "success" and isinstance(ack.get("data"), dict)):
    raise ValueError(f"turn start ack must be ApiResponse success envelope: {ack!r}")
ack = ack["data"]
events_path = os.environ["EVENTS_PATH"]

request_id = ack.get("requestId", "")
model = ""
output_text = ""
reasoning_text = ""
output = []
messages = {}
error_text = ""
usage = {
    "input_tokens": 0,
    "output_tokens": 0,
    "total_tokens": 0,
}
stop_reason = "end_turn"
metrics = None

def agent_event(record):
    event = record.get("event")
    return event.get("data", {}) if isinstance(event, dict) and event.get("type") == "agent" else {}

def upsert_block(message_id, block):
    if not message_id or not isinstance(block, dict):
        return
    message = messages.setdefault(message_id, {
        "messageId": message_id,
        "role": "agent",
        "blocks": [],
    })
    blocks = message.setdefault("blocks", [])
    block_id = block.get("blockId")
    for index, existing in enumerate(blocks):
        if existing.get("blockId") == block_id:
            blocks[index] = block
            return
    blocks.append(block)

def snake_stop_reason(value):
    return {
        "endTurn": "end_turn",
        "outputLimit": "output_limit",
        "transferUnresolved": "transfer_unresolved",
    }.get(value, value or "end_turn")

with open(events_path, "r", encoding="utf-8") as handle:
    for line in handle:
        if not line.startswith("data: "):
            continue
        event = agent_event(json.loads(line[6:]))
        payload = event.get("payload") or {}
        event_type = payload.get("type")
        data = payload.get("data") or {}
        if event_type == "messageStarted":
            message = data.get("message") or {}
            message_id = message.get("messageId")
            if message_id:
                messages[message_id] = message
        elif event_type == "messageBlockStarted":
            upsert_block(data.get("messageId"), data.get("block") or {})
        elif event_type == "messageBlockCompleted":
            upsert_block(data.get("messageId"), data.get("block") or {})
        elif event_type == "messageCompleted":
            message_id = data.get("messageId")
            if message_id:
                messages[message_id] = data
        elif event_type == "metricsUpdated":
            metrics = data
            usage = data.get("usage") or usage
        elif event_type == "waitingForInput":
            stop_reason = (
                "waiting_user_action"
                if data.get("type") == "userAction"
                else "waiting_for_input"
            )
        elif event_type == "turnCompleted":
            stop_reason = snake_stop_reason(data.get("stopReason"))
        elif event_type == "turnFailed":
            stop_reason = "error"
            error = data.get("error")
            if not isinstance(error, dict) or not isinstance(error.get("code"), str) or not isinstance(error.get("message"), str):
                raise ValueError(f"turnFailed payload must contain AgentErrorInfo: {payload!r}")
            error_text = "Error: " + str(error.get("message") or error.get("code") or "")

if error_text:
    output_text = error_text
else:
    for message in messages.values():
        for block in message.get("blocks") or []:
            content = block.get("content") or {}
            content_type = content.get("type")
            data = content.get("data") or {}
            if content_type == "assistantText" and data.get("phase") == "finalAnswer":
                output_text += data.get("text", "")
            elif content_type == "reasoning":
                reasoning_text += data.get("text", "")
if output_text:
    output.append({"type": "output_text", "text": output_text})

for message in messages.values():
    for block in message.get("blocks") or []:
        content = block.get("content") or {}
        content_type = content.get("type")
        data = content.get("data") or {}
        if content_type == "toolCall":
            arguments_state = data.get("arguments") or {}
            arguments = arguments_state.get("value", arguments_state.get("jsonFragment", {}))
            output.append({
                "type": "tool_call",
                "call_id": data.get("callId", ""),
                "name": data.get("name", ""),
                "arguments": arguments,
            })
        elif content_type == "toolResult":
            output.append({
                "type": "tool_result",
                "call_id": data.get("callId", ""),
                "tool_name": data.get("toolName", ""),
                "content": data.get("content") or [],
                "is_error": bool(data.get("isError", False)),
            })
        elif content_type == "executionSummary":
            output.append({"type": "execution_summary", "text": data.get("text", "")})

response = {
    "request_id": request_id,
    "message_id": ack.get("messageId", ""),
    "session_id": ack.get("sessionId", ""),
    "model": model,
    "output": output,
    "reasoning": [{"text": reasoning_text}] if reasoning_text else [],
    "stop_reason": stop_reason,
    "usage": usage,
    "metrics": metrics,
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
  message_id="$(api_json_get "${ack_json}" "messageId")"
  turn_id="$(api_json_get "${ack_json}" "turnId")"
  if [[ -z "${message_id}" || -z "${turn_id}" ]]; then
    printf '%s\n' "${ack_json}"
    return 0
  fi

  events_log="$(scenario_report_path "${log_name}_events.log")"
  collect_timeline_events "${session_id}" "${turn_id}" "${events_log}" || return $?
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
  message_id="$(api_json_get "${ack_json}" "messageId")"
  turn_id="$(api_json_get "${ack_json}" "turnId")"
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
