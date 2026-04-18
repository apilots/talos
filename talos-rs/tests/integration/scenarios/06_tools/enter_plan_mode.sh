#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command curl
require_command python3

if [[ -z "${BAILIAN_API_KEY:-}" && -z "${BAILIAN_CODINGPLAN_API_KEY:-}" && -z "${OPENAI_API_KEY:-}" && -z "${ANTHROPIC_API_KEY:-}" ]]; then
  echo "CASE enter_plan_mode_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE enter_plan_mode_real_llm_precheck PASS key_detected=true"

build_binaries
start_server
trap stop_server EXIT

models_json="$(curl -sS -H "Authorization: Bearer ${AUTH_TOKEN}" "${GATEWAY_URL}/v1/models")"
primary_model="$(python3 -c 'import json,sys; data=json.load(sys.stdin); arr=data.get("data",[]); print((arr[0].get("id") if arr else ""))' <<< "${models_json}")"
if [[ -z "${primary_model}" ]]; then
  echo "CASE enter_plan_mode_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
echo "CASE enter_plan_mode_select_model PASS model=${primary_model}"

create_session() {
  python3 - <<'PY'
import json
import os

payload = {
    "context": {
        "source_type": "http",
        "provider": "http",
        "channel_id": "tool-enter-plan-mode",
        "user_id": "tool-enter-plan-mode-user",
        "thread_id": None,
        "message_id": "msg-tool-enter-plan-mode",
        "trace_id": "trace-tool-enter-plan-mode",
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
  echo "CASE enter_plan_mode_create_session FAIL response=${session_json}"
  exit 1
fi
echo "CASE enter_plan_mode_create_session PASS session_id=${session_id}"

enter_input="You must call tool 'EnterPlanMode' exactly once with reason Need explicit planning. After the tool call, respond with exactly ENTER_PLAN_MODE_OK"
enter_payload="$(build_payload "${enter_input}")"
enter_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions/${session_id}/messages" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${enter_payload}")"
log_case_json "tool_enter_plan_mode" "${enter_payload}" "${enter_json}"

enter_output="$(extract_output_text "${enter_json}")"
if [[ -z "${enter_output}" || "${enter_output}" == Error:* ]]; then
  echo "CASE tool_enter_plan_mode FAIL response=${enter_json}"
  exit 1
fi
if ! echo "${enter_output}" | grep -Fq "ENTER_PLAN_MODE_OK"; then
  echo "CASE tool_enter_plan_mode FAIL missing_ack output=${enter_output}"
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
content = content_items[0].get("value") or {}
assert content.get("mode") == "plan", content
assert content.get("reason") == "Need explicit planning", content
PY

echo "CASE tool_enter_plan_mode PASS"
echo "DETAIL tool_enter_plan_mode_output=${enter_output}"
