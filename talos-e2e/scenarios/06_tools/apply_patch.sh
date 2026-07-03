#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

load_env
require_command curl
require_command python3
require_command mktemp

if [[ -z "${TALOS_LLM_BAILIAN_API_KEY:-}" && -z "${TALOS_LLM_ANTHROPIC_API_KEY:-}" && -z "${TALOS_LLM_OPENAI_API_KEY:-}" ]]; then
  echo "CASE apply_patch_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE apply_patch_real_llm_precheck PASS key_detected=true"

TEST_TALOS_HOME="$(make_e2e_temp_dir "talos-tool-apply-patch-home")"
cleanup() {
  stop_server
  rm -rf "${TEST_TALOS_HOME}"
}
export TALOS_HOME="${TEST_TALOS_HOME}"

WORKSPACE_DIR="${TALOS_HOME}/workspace-talos"
FIXTURE_DIR="${WORKSPACE_DIR}/notes"
FIXTURE_FILE="${FIXTURE_DIR}/apply_patch_target.txt"
PATCH_MARKER="patch-marker-$(date '+%Y%m%d%H%M%S')-$$"
mkdir -p "${FIXTURE_DIR}"

build_binaries
start_server
trap cleanup EXIT

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
primary_model="$(select_e2e_model_from_json "${models_json}")"
primary_provider="$(select_e2e_model_provider_from_json "${models_json}" "${primary_model}")"
if [[ -z "${primary_model}" || -z "${primary_provider}" ]]; then
  echo "CASE apply_patch_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
export PRIMARY_PROVIDER="${primary_provider}"
echo "CASE apply_patch_select_model PASS model=${primary_model}"

build_session_context() {
  python3 - <<'PY'
import json
import os

payload = {
    "context": {
        "source_type": "http",
        "provider": "http",
        "channel_id": "tool-apply-patch",
        "user_id": "tool-apply-patch-user",
        "thread_id": None,
        "message_id": "msg-tool-apply-patch",
        "trace_id": "trace-tool-apply-patch",
    },
    "model_ref": {"provider": os.environ["PRIMARY_PROVIDER"], "id": os.environ["PRIMARY_MODEL"]},
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
print(data.get("output_text", ""))
PY
}

session_payload="$(build_session_context)"
session_id="$(new_session_id)"
remember_session_context "${session_id}" "${session_payload}"
session_json="{\"id\":\"${session_id}\"}"
session_id="$(session_id_from_json "${session_json}")"
if [[ -z "${session_id}" ]]; then
  echo "CASE apply_patch_session_id FAIL response=${session_json}"
  exit 1
fi
echo "CASE apply_patch_session_id PASS session_id=${session_id}"

apply_patch_input="$(cat <<EOF
You must call tool 'ApplyPatch' exactly once to add notes/apply_patch_target.txt with exactly one line: ${PATCH_MARKER}.

Use this exact patch argument:
*** Begin Patch
*** Add File: notes/apply_patch_target.txt
+${PATCH_MARKER}
*** End Patch

Do not write any visible text before calling the tool. After the tool returns, the final visible answer must be exactly:
APPLY_PATCH_OK:${PATCH_MARKER}
EOF
)"
apply_patch_payload="$(build_payload "${apply_patch_input}")"
apply_patch_json="$(send_message_response "${session_id}" "${apply_patch_payload}" "apply_patch")"
log_case_json "tool_apply_patch" "${apply_patch_payload}" "${apply_patch_json}"

apply_patch_output="$(extract_output_text "${apply_patch_json}")"
if [[ -z "${apply_patch_output}" || "${apply_patch_output}" == Error:* ]]; then
  echo "CASE tool_apply_patch FAIL response=${apply_patch_json}"
  exit 1
fi
if ! echo "${apply_patch_output}" | grep -Fq "APPLY_PATCH_OK:${PATCH_MARKER}"; then
  echo "CASE tool_apply_patch FAIL missing_marker output=${apply_patch_output}"
  exit 1
fi

tool_result_tool="$(python3 -c 'import json,sys; d=json.load(sys.stdin); results=[i.get("tool_name","") for i in d.get("output", []) if i.get("type")=="tool_result"]; print(",".join(results))' <<< "${apply_patch_json}")"
if ! echo "${tool_result_tool}" | grep -Fq "ApplyPatch"; then
  echo "CASE tool_apply_patch FAIL missing_apply_patch_tool result=${tool_result_tool}"
  exit 1
fi

if [[ ! -f "${FIXTURE_FILE}" ]]; then
  echo "CASE tool_apply_patch FAIL missing_file path=${FIXTURE_FILE}"
  exit 1
fi
if ! grep -Fq "${PATCH_MARKER}" "${FIXTURE_FILE}"; then
  echo "CASE tool_apply_patch FAIL file_missing_marker path=${FIXTURE_FILE}"
  exit 1
fi

echo "CASE tool_apply_patch PASS"
echo "DETAIL tool_apply_patch_output=${apply_patch_output}"
