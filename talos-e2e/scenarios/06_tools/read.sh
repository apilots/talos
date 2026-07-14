#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

require_command curl
require_command python3
require_command mktemp

if ! has_real_llm_key; then
  echo "CASE read_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE read_real_llm_precheck PASS key_detected=true"

TEST_TALOS_HOME="$(make_e2e_temp_dir "talos-tool-read-home")"
cleanup() {
  stop_server
  rm -rf "${TEST_TALOS_HOME}"
}
export TALOS_HOME="${TEST_TALOS_HOME}"

WORKSPACE_DIR="${TALOS_HOME}/workspace-talos"
FIXTURE_DIR="${WORKSPACE_DIR}/notes"
FIXTURE_FILE="${FIXTURE_DIR}/read_target.txt"
READ_MARKER="read-marker-$(date '+%Y%m%d%H%M%S')-$$"
mkdir -p "${FIXTURE_DIR}"
cat > "${FIXTURE_FILE}" <<EOF
This is a read fixture.
${READ_MARKER}
EOF

build_binaries
start_server
trap cleanup EXIT

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
primary_model="$(select_e2e_model_from_json "${models_json}")"
primary_provider="$(select_e2e_model_provider_from_json "${models_json}" "${primary_model}")"
if [[ -z "${primary_model}" || -z "${primary_provider}" ]]; then
  echo "CASE read_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
export PRIMARY_PROVIDER="${primary_provider}"
echo "CASE read_select_model PASS model=${primary_model}"

build_session_context() {
  python3 - <<'PY'
import json
import os

payload = {
    "context": {
        "source_type": "http",
        "provider": "http",
        "channel_id": "tool-read",
        "user_id": "tool-read-user",
        "thread_id": None,
        "message_id": "msg-tool-read",
        "trace_id": "trace-tool-read",
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
  echo "CASE read_session_id FAIL response=${session_json}"
  exit 1
fi
echo "CASE read_session_id PASS session_id=${session_id}"

read_input="You must call tool 'Read' exactly once with path notes/read_target.txt. After the tool call, respond with exactly READ_OK:${READ_MARKER}"
read_payload="$(build_payload "${read_input}")"
read_json="$(send_message_response "${session_id}" "${read_payload}" "read")"
log_case_json "tool_read" "${read_payload}" "${read_json}"

read_output="$(extract_output_text "${read_json}")"
if [[ -z "${read_output}" || "${read_output}" == Error:* ]]; then
  echo "CASE tool_read FAIL response=${read_json}"
  exit 1
fi
if ! echo "${read_output}" | grep -Fq "READ_OK:${READ_MARKER}"; then
  echo "CASE tool_read FAIL missing_marker output=${read_output}"
  exit 1
fi

tool_call_name="$(python3 -c 'import json,sys; d=json.load(sys.stdin); calls=[i for i in d.get("output", []) if i.get("type")=="tool_call"]; print(calls[0].get("name","") if calls else "")' <<< "${read_json}")"
tool_result_tool="$(python3 -c 'import json,sys; d=json.load(sys.stdin); results=[i for i in d.get("output", []) if i.get("type")=="tool_result"]; print(results[0].get("tool_name","") if results else "")' <<< "${read_json}")"
if [[ "${tool_call_name}" != "Read" || "${tool_result_tool}" != "Read" ]]; then
  echo "CASE tool_read FAIL unexpected_tool call=${tool_call_name} result=${tool_result_tool}"
  exit 1
fi

echo "CASE tool_read PASS"
echo "DETAIL tool_read_output=${read_output}"
