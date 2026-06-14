#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command curl
require_command python3
require_command mktemp

if [[ -z "${TALOS_LLM_BAILIAN_API_KEY:-}" && -z "${TALOS_LLM_ANTHROPIC_API_KEY:-}" && -z "${TALOS_LLM_OPENAI_API_KEY:-}" ]]; then
  echo "CASE memory_get_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE memory_get_real_llm_precheck PASS key_detected=true"

TEST_TALOS_HOME="$(mktemp -d "${TMPDIR:-/tmp}/talos-tool-memory-get-home.XXXXXX")"
cleanup() {
  stop_server
  rm -rf "${TEST_TALOS_HOME}"
}
export TALOS_HOME="${TEST_TALOS_HOME}"

MEMORY_DATE="$(date '+%Y-%m-%d')"
MEMORY_MARKER="memory-get-marker-$(date '+%Y%m%d%H%M%S')-$$"
MEMORY_FILE_PATH="memory/${MEMORY_DATE}.md"

build_binaries
start_server
trap cleanup EXIT

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
primary_model="$(select_e2e_model_from_json "${models_json}")"
if [[ -z "${primary_model}" ]]; then
  echo "CASE memory_get_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
echo "CASE memory_get_select_model PASS model=${primary_model}"

request_json() {
  local method="$1"
  local url="$2"
  local body="${3:-}"
  local response_file
  response_file="$(mktemp "${TMPDIR:-/tmp}/talos-integration-response.XXXXXX")"
  local status
  if [[ -n "${body}" ]]; then
    status="$(curl -sS -o "${response_file}" -w "%{http_code}" \
      -X "${method}" "${url}" \
      -H "Content-Type: application/json" \
      -d "${body}")"
  else
    status="$(curl -sS -o "${response_file}" -w "%{http_code}" \
      -X "${method}" "${url}" \
     )"
  fi
  cat "${response_file}"
  rm -f "${response_file}"
  printf '\nHTTP_STATUS:%s' "${status}"
}

parse_response() {
  local raw="$1"
  RESPONSE_STATUS="${raw##*HTTP_STATUS:}"
  RESPONSE_BODY="${raw%HTTP_STATUS:*}"
}

create_session() {
  python3 - <<'PY'
import json
import os

payload = {
    "context": {
        "source_type": "http",
        "provider": "http",
        "channel_id": "tool-memory-get",
        "user_id": "tool-memory-get-user",
        "thread_id": None,
        "message_id": "msg-tool-memory-get",
        "trace_id": "trace-tool-memory-get",
    },
    "model": os.environ["PRIMARY_MODEL"],
}
print(json.dumps(payload, ensure_ascii=False))
PY
}

build_payload() {
  local input_text="$1"
  INPUT_TEXT="$input_text" python3 - <<'PY'
import json
import os

payload = {
    "input": [{"type": "text", "text": os.environ["INPUT_TEXT"]}],
    "workspace_id": None,
}
print(json.dumps(payload, ensure_ascii=False))
PY
}

extract_output_text() {
  local response_json="$1"
  RESPONSE_JSON="${response_json}" python3 - <<'PY'
import json
import os

raw = os.environ.get("RESPONSE_JSON", "")
try:
    data = json.loads(raw)
except Exception:
    print("")
    raise SystemExit(0)

for item in data.get("output", []):
    if item.get("type") == "output_text":
        print(item.get("text", ""))
        raise SystemExit(0)
print(data.get("output_text", ""))
PY
}

create_memory_payload="$(python3 - <<PY
import json
print(json.dumps({
    "content": "# Memory\\n${MEMORY_MARKER}\\n",
    "base_version": {"mode": "must_not_exist"},
}))
PY
)"
parse_response "$(request_json PUT "${GATEWAY_URL}/v1/memory/files/${MEMORY_DATE}" "${create_memory_payload}")"
status="${RESPONSE_STATUS}"
body="${RESPONSE_BODY}"
log_case_json "seed_memory_file_for_memory_get" "${create_memory_payload}" "${body}"
if [[ "${status}" != "200" ]]; then
  echo "CASE seed_memory_file_for_memory_get FAIL status=${status} body=${body}"
  exit 1
fi
echo "CASE seed_memory_file_for_memory_get PASS status=${status}"

session_payload="$(create_session)"
session_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions" \
  -H "Content-Type: application/json" \
  -d "${session_payload}")"
session_id="$(session_id_from_json "${session_json}")"
if [[ -z "${session_id}" ]]; then
  echo "CASE memory_get_create_session FAIL response=${session_json}"
  exit 1
fi
echo "CASE memory_get_create_session PASS session_id=${session_id}"

memory_get_input="You must call tool 'MemoryGet' exactly once with path ${MEMORY_FILE_PATH}. After the tool call, respond with exactly MEMORY_GET_OK:${MEMORY_MARKER}"
memory_get_payload="$(build_payload "${memory_get_input}")"
memory_get_json="$(send_message_response "${session_id}" "${memory_get_payload}" "memory_get")"
log_case_json "tool_memory_get" "${memory_get_payload}" "${memory_get_json}"

memory_get_output="$(extract_output_text "${memory_get_json}")"
if [[ -z "${memory_get_output}" || "${memory_get_output}" == Error:* ]]; then
  echo "CASE tool_memory_get FAIL response=${memory_get_json}"
  exit 1
fi
if ! echo "${memory_get_output}" | grep -Fq "MEMORY_GET_OK:${MEMORY_MARKER}"; then
  echo "CASE tool_memory_get FAIL missing_marker output=${memory_get_output}"
  exit 1
fi

tool_result_tool="$(MEMORY_GET_JSON="${memory_get_json}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["MEMORY_GET_JSON"])
results = [item for item in data.get("output", []) if item.get("type") == "tool_result"]
print(results[0].get("tool_name", "") if results else "")
PY
)"
if [[ "${tool_result_tool}" != "MemoryGet" ]]; then
  echo "CASE tool_memory_get FAIL unexpected_tool result=${tool_result_tool}"
  exit 1
fi

echo "CASE tool_memory_get PASS"
echo "DETAIL tool_memory_get_output=${memory_get_output}"
