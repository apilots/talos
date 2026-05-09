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
  echo "CASE write_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE write_real_llm_precheck PASS key_detected=true"

TEST_TALOS_HOME="$(mktemp -d "${TMPDIR:-/tmp}/talos-tool-write-home.XXXXXX")"
cleanup() {
  stop_server
  rm -rf "${TEST_TALOS_HOME}"
}
export TALOS_HOME="${TEST_TALOS_HOME}"

WORKSPACE_DIR="${TALOS_HOME}/workspace-talos"
FIXTURE_DIR="${WORKSPACE_DIR}/notes"
FIXTURE_FILE="${FIXTURE_DIR}/write_target.txt"
WRITE_MARKER="write-marker-$(date '+%Y%m%d%H%M%S')-$$"
mkdir -p "${FIXTURE_DIR}"

build_binaries
start_server
trap cleanup EXIT

models_json="$(curl -sS -H "Authorization: Bearer ${AUTH_TOKEN}" "${GATEWAY_URL}/v1/models")"
primary_model="$(python3 -c 'import json,sys; data=json.load(sys.stdin); arr=data.get("data",[]); print((arr[0].get("id") if arr else ""))' <<< "${models_json}")"
if [[ -z "${primary_model}" ]]; then
  echo "CASE write_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
echo "CASE write_select_model PASS model=${primary_model}"

create_session() {
  python3 - <<'PY'
import json
import os

payload = {
    "context": {
        "source_type": "http",
        "provider": "http",
        "channel_id": "tool-write",
        "user_id": "tool-write-user",
        "thread_id": None,
        "message_id": "msg-tool-write",
        "trace_id": "trace-tool-write",
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

session_payload="$(create_session)"
session_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${session_payload}")"
session_id="$(session_id_from_json "${session_json}")"
if [[ -z "${session_id}" ]]; then
  echo "CASE write_create_session FAIL response=${session_json}"
  exit 1
fi
echo "CASE write_create_session PASS session_id=${session_id}"

write_input="You must call tool 'Write' exactly once to create notes/write_target.txt with content ${WRITE_MARKER}. After the tool call, respond with exactly WRITE_OK:${WRITE_MARKER}"
write_payload="$(build_payload "${write_input}")"
write_json="$(send_message_response "${session_id}" "${write_payload}" "write")"
log_case_json "tool_write" "${write_payload}" "${write_json}"

write_output="$(extract_output_text "${write_json}")"
if [[ -z "${write_output}" || "${write_output}" == Error:* ]]; then
  echo "CASE tool_write FAIL response=${write_json}"
  exit 1
fi
if ! echo "${write_output}" | grep -Fq "WRITE_OK:${WRITE_MARKER}"; then
  echo "CASE tool_write FAIL missing_marker output=${write_output}"
  exit 1
fi

tool_result_tool="$(WRITE_JSON="${write_json}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["WRITE_JSON"])
results = [item for item in data.get("output", []) if item.get("type") == "tool_result"]
names = [item.get("tool_name", "") for item in results]
print(",".join(names))
PY
)"
if ! echo "${tool_result_tool}" | grep -Fq "Write"; then
  echo "CASE tool_write FAIL unexpected_tool result=${tool_result_tool}"
  exit 1
fi

if [[ ! -f "${FIXTURE_FILE}" ]]; then
  echo "CASE tool_write FAIL file_missing path=${FIXTURE_FILE}"
  exit 1
fi
if ! grep -Fq "${WRITE_MARKER}" "${FIXTURE_FILE}"; then
  echo "CASE tool_write FAIL marker_missing path=${FIXTURE_FILE}"
  exit 1
fi

echo "CASE tool_write PASS"
echo "DETAIL tool_write_output=${write_output}"
