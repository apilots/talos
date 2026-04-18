#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command curl
require_command python3

if [[ -z "${BAILIAN_API_KEY:-}" && -z "${BAILIAN_CODINGPLAN_API_KEY:-}" && -z "${OPENAI_API_KEY:-}" && -z "${ANTHROPIC_API_KEY:-}" ]]; then
  echo "CASE exit_plan_mode_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE exit_plan_mode_real_llm_precheck PASS key_detected=true"

build_binaries
start_server
trap stop_server EXIT

models_json="$(curl -sS -H "Authorization: Bearer ${AUTH_TOKEN}" "${GATEWAY_URL}/v1/models")"
primary_model="$(python3 -c 'import json,sys; data=json.load(sys.stdin); arr=data.get("data",[]); print((arr[0].get("id") if arr else ""))' <<< "${models_json}")"
if [[ -z "${primary_model}" ]]; then
  echo "CASE exit_plan_mode_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
echo "CASE exit_plan_mode_select_model PASS model=${primary_model}"

create_session() {
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
    "stream": False,
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
  echo "CASE exit_plan_mode_create_session FAIL response=${session_json}"
  exit 1
fi
echo "CASE exit_plan_mode_create_session PASS session_id=${session_id}"

enter_input="You must call tool 'EnterPlanMode' exactly once with reason Need explicit planning. After the tool call, respond with exactly PLAN_ENTERED"
enter_payload="$(build_payload "${enter_input}")"
enter_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions/${session_id}/messages" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${enter_payload}")"
log_case_json "seed_enter_plan_mode_before_exit" "${enter_payload}" "${enter_json}"

enter_output="$(extract_output_text "${enter_json}")"
if [[ -z "${enter_output}" || "${enter_output}" == Error:* ]]; then
  echo "CASE seed_enter_plan_mode_before_exit FAIL response=${enter_json}"
  exit 1
fi
if ! echo "${enter_output}" | grep -Fq "PLAN_ENTERED"; then
  echo "CASE seed_enter_plan_mode_before_exit FAIL missing_ack output=${enter_output}"
  exit 1
fi

plan_input="You are now in plan mode. Respond with exactly two concise numbered steps for validating tool coverage, then end with PLAN_READY."
plan_payload="$(build_payload "${plan_input}")"
plan_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions/${session_id}/messages" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${plan_payload}")"
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

exit_input="You must call tool 'ExitPlanMode' exactly once with summary Planning finished. After the tool call, respond with exactly EXIT_PLAN_MODE_OK"
exit_payload="$(build_payload "${exit_input}")"
exit_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions/${session_id}/messages" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${exit_payload}")"
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
result = results[0]
assert result.get("tool_name") == "ExitPlanMode", result.get("tool_name")
content_items = result.get("content") or []
assert content_items, "missing content payload"
content = content_items[0].get("value") or {}
assert content.get("mode") == "default", content
assert content.get("summary") == "Planning finished", content
PY

echo "CASE tool_exit_plan_mode PASS"
echo "DETAIL tool_exit_plan_mode_output=${exit_output}"
