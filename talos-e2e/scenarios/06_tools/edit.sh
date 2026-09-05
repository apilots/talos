#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

require_command curl
require_command python3
require_command mktemp

if ! has_real_llm_key; then
  echo "CASE edit_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE edit_real_llm_precheck PASS key_detected=true"

TEST_TALOS_HOME="$(make_e2e_temp_dir "talos-tool-edit-home")"
cleanup() {
  stop_server
  rm -rf "${TEST_TALOS_HOME}"
}
export TALOS_HOME="${TEST_TALOS_HOME}"

WORKSPACE_DIR="${TALOS_HOME}/workspace-talos/anonymous"
FIXTURE_DIR="${WORKSPACE_DIR}/notes"
FIXTURE_FILE="${FIXTURE_DIR}/edit_target.txt"
OLD_MARKER="old-marker-$(date '+%Y%m%d%H%M%S')-$$"
NEW_MARKER="new-marker-$(date '+%Y%m%d%H%M%S')-$$"
mkdir -p "${FIXTURE_DIR}"
cat > "${FIXTURE_FILE}" <<EOF
before
${OLD_MARKER}
after
EOF

build_binaries
start_server
trap cleanup EXIT

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
primary_model="$(select_e2e_model_from_json "${models_json}")"
primary_provider="$(select_e2e_model_provider_from_json "${models_json}" "${primary_model}")"
if [[ -z "${primary_model}" || -z "${primary_provider}" ]]; then
  echo "CASE edit_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
export PRIMARY_PROVIDER="${primary_provider}"
echo "CASE edit_select_model PASS model=${primary_model}"

build_session_start_defaults() {
  python3 - <<'PY'
import json
import os

payload = {
    "modelSelection": {"modelRef": {"provider": os.environ["PRIMARY_PROVIDER"], "id": os.environ["PRIMARY_MODEL"]}, "reasoningEffort": None},
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
    "input": {"content": [{"type": "text", "text": os.environ["INPUT_TEXT"]}]},
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

session_payload="$(build_session_start_defaults)"
session_id="$(new_session_id)"
remember_session_start_defaults "${session_id}" "${session_payload}"
session_json="{\"id\":\"${session_id}\"}"
session_id="$(session_id_from_json "${session_json}")"
if [[ -z "${session_id}" ]]; then
  echo "CASE edit_session_id FAIL response=${session_json}"
  exit 1
fi
echo "CASE edit_session_id PASS session_id=${session_id}"

edit_input="You must first call Read on notes/edit_target.txt, then call Edit exactly once on notes/edit_target.txt replacing ${OLD_MARKER} with ${NEW_MARKER}. After the tool call, respond with exactly EDIT_OK:${NEW_MARKER}"
edit_payload="$(build_payload "${edit_input}")"
edit_json="$(send_message_response "${session_id}" "${edit_payload}" "edit")"
log_case_json "tool_edit" "${edit_payload}" "${edit_json}"

edit_output="$(extract_output_text "${edit_json}")"
if [[ -z "${edit_output}" || "${edit_output}" == Error:* ]]; then
  echo "CASE tool_edit FAIL response=${edit_json}"
  exit 1
fi
if ! echo "${edit_output}" | grep -Fq "EDIT_OK:${NEW_MARKER}"; then
  echo "CASE tool_edit FAIL missing_marker output=${edit_output}"
  exit 1
fi

tool_result_tool="$(python3 -c 'import json,sys; d=json.load(sys.stdin); results=[i.get("tool_name","") for i in d.get("output", []) if i.get("type")=="tool_result"]; print(",".join(results))' <<< "${edit_json}")"
if ! echo "${tool_result_tool}" | grep -Fq "Edit"; then
  echo "CASE tool_edit FAIL missing_edit_tool result=${tool_result_tool}"
  exit 1
fi

if ! grep -Fq "${NEW_MARKER}" "${FIXTURE_FILE}"; then
  echo "CASE tool_edit FAIL file_not_updated path=${FIXTURE_FILE}"
  exit 1
fi
if grep -Fq "${OLD_MARKER}" "${FIXTURE_FILE}"; then
  echo "CASE tool_edit FAIL old_marker_still_present path=${FIXTURE_FILE}"
  exit 1
fi

echo "CASE tool_edit PASS"
echo "DETAIL tool_edit_output=${edit_output}"
