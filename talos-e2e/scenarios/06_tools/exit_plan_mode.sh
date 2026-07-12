#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

load_env
require_command curl
require_command python3

if ! has_real_llm_key; then
  echo "CASE exit_plan_mode_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE exit_plan_mode_real_llm_precheck PASS key_detected=true"

build_binaries
start_server
trap stop_server EXIT

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
primary_model="$(select_e2e_model_from_json "${models_json}")"
primary_provider="$(select_e2e_model_provider_from_json "${models_json}" "${primary_model}")"
if [[ -z "${primary_model}" || -z "${primary_provider}" ]]; then
  echo "CASE exit_plan_mode_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
export PRIMARY_PROVIDER="${primary_provider}"
echo "CASE exit_plan_mode_select_model PASS model=${primary_model}"

build_session_context() {
  python3 - <<'PY'
import json
import os

payload = {
    "context": {
        "source_type": "http",
        "provider": "http",
        "channel_id": "tool-exit-plan-mode",
        "user_id": "tool-exit-plan-mode-user",
        "thread_id": None,
        "message_id": "msg-tool-exit-plan-mode",
        "trace_id": "trace-tool-exit-plan-mode",
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
  echo "CASE exit_plan_mode_session_id FAIL response=${session_json}"
  exit 1
fi
echo "CASE exit_plan_mode_session_id PASS session_id=${session_id}"

enter_input="You must call tool 'EnterPlanMode' exactly once with reason Need explicit planning. After the tool call, respond with exactly PLAN_ENTERED"
enter_payload="$(build_payload "${enter_input}")"
enter_json="$(send_message_response "${session_id}" "${enter_payload}" "enter")"
log_case_json "seed_enter_plan_mode_before_exit" "${enter_payload}" "${enter_json}"

enter_output="$(extract_output_text "${enter_json}")"
if [[ -z "${enter_output}" || "${enter_output}" == Error:* ]]; then
  echo "CASE seed_enter_plan_mode_before_exit FAIL response=${enter_json}"
  exit 1
fi
ENTER_JSON="${enter_json}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["ENTER_JSON"])
results = [item for item in data.get("output", []) if item.get("type") == "tool_result"]
assert results, "missing tool_result"
result = results[0]
assert result.get("tool_name") == "EnterPlanMode", result.get("tool_name")
content_items = result.get("content") or []
assert content_items, "missing content payload"
item = content_items[0]
assert item.get("type") == "json", item
content = (item.get("value") or {}).get("value") or {}
assert content.get("mode") == "plan", content
PY

plan_input="You are now in plan mode. Respond with exactly two concise numbered steps for validating tool coverage, then end with PLAN_READY."
plan_payload="$(build_payload "${plan_input}")"
plan_json="$(send_message_response "${session_id}" "${plan_payload}" "plan")"
log_case_json "seed_plan_content_before_exit" "${plan_payload}" "${plan_json}"

plan_output="$(extract_output_text "${plan_json}")"
if [[ -z "${plan_output}" || "${plan_output}" == Error:* ]]; then
  echo "CASE seed_plan_content_before_exit FAIL response=${plan_json}"
  exit 1
fi
if ! echo "${plan_output}" | grep -Fq "PLAN_READY"; then
  echo "CASE seed_plan_content_before_exit FAIL missing_ack output=${plan_output}"
  exit 1
fi

exit_input="Do not inspect files. Do not call Bash, Glob, Grep, Read, or any other tool. You must call tool 'ExitPlanMode' exactly once with summary exactly \"Planning finished\". After the tool call, respond with exactly EXIT_PLAN_MODE_OK"
exit_payload="$(build_payload "${exit_input}")"
exit_json="$(send_message_response "${session_id}" "${exit_payload}" "exit")"
log_case_json "tool_exit_plan_mode" "${exit_payload}" "${exit_json}"

exit_output="$(extract_output_text "${exit_json}")"
if [[ -z "${exit_output}" || "${exit_output}" == Error:* ]]; then
  echo "CASE tool_exit_plan_mode FAIL response=${exit_json}"
  exit 1
fi
if ! echo "${exit_output}" | grep -Fq "EXIT_PLAN_MODE_OK"; then
  echo "CASE tool_exit_plan_mode FAIL missing_ack output=${exit_output}"
  exit 1
fi

EXIT_JSON="${exit_json}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["EXIT_JSON"])
results = [item for item in data.get("output", []) if item.get("type") == "tool_result"]
assert results, "missing tool_result"
exit_results = [item for item in results if item.get("tool_name") == "ExitPlanMode"]
assert len(exit_results) == 1, [item.get("tool_name") for item in results]
result = exit_results[0]
content_items = result.get("content") or []
assert content_items, "missing content payload"
item = content_items[0]
assert item.get("type") == "json", item
content = (item.get("value") or {}).get("value") or {}
assert content.get("mode") == "default", content
assert content.get("summary") == "Planning finished", content
PY

echo "CASE tool_exit_plan_mode PASS"
echo "DETAIL tool_exit_plan_mode_output=${exit_output}"
