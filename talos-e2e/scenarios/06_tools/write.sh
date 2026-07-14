#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

require_command curl
require_command python3
require_command mktemp

if ! has_real_llm_key; then
  echo "CASE write_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE write_real_llm_precheck PASS key_detected=true"

TEST_TALOS_HOME="$(make_e2e_temp_dir "talos-tool-write-home")"
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

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
primary_model="$(select_e2e_model_from_json "${models_json}")"
primary_provider="$(select_e2e_model_provider_from_json "${models_json}" "${primary_model}")"
if [[ -z "${primary_model}" || -z "${primary_provider}" ]]; then
  echo "CASE write_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
export PRIMARY_PROVIDER="${primary_provider}"
echo "CASE write_select_model PASS model=${primary_model}"

build_session_context() {
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
    "modelSelection": {"modelRef": {"provider": os.environ["PRIMARY_PROVIDER"], "id": os.environ["PRIMARY_MODEL"]}, "reasoning": {"type": "modelDefault"}},
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

session_payload="$(build_session_context)"
session_id="$(new_session_id)"
remember_session_context "${session_id}" "${session_payload}"
session_json="{\"id\":\"${session_id}\"}"
session_id="$(session_id_from_json "${session_json}")"
if [[ -z "${session_id}" ]]; then
  echo "CASE write_session_id FAIL response=${session_json}"
  exit 1
fi
echo "CASE write_session_id PASS session_id=${session_id}"

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
