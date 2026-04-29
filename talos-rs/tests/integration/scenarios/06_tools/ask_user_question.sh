#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command curl
require_command python3

if [[ -z "${BAILIAN_API_KEY:-}" && -z "${BAILIAN_CODINGPLAN_API_KEY:-}" && -z "${OPENAI_API_KEY:-}" && -z "${ANTHROPIC_API_KEY:-}" ]]; then
  echo "CASE ask_user_question_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE ask_user_question_real_llm_precheck PASS key_detected=true"

build_binaries
start_server
trap stop_server EXIT

models_json="$(curl -sS -H "Authorization: Bearer ${AUTH_TOKEN}" "${GATEWAY_URL}/v1/models")"
primary_model="$(python3 -c 'import json,sys; data=json.load(sys.stdin); arr=data.get("data",[]); print((arr[0].get("id") if arr else ""))' <<< "${models_json}")"
if [[ -z "${primary_model}" ]]; then
  echo "CASE ask_user_question_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
echo "CASE ask_user_question_select_model PASS model=${primary_model}"

create_session() {
  python3 - <<'PY'
import json
import os

payload = {
    "context": {
        "source_type": "http",
        "provider": "http",
        "channel_id": "tool-ask-user-question",
        "user_id": "tool-ask-user-question-user",
        "thread_id": None,
        "message_id": "msg-tool-ask-user-question",
        "trace_id": "trace-tool-ask-user-question",
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
  echo "CASE ask_user_question_create_session FAIL response=${session_json}"
  exit 1
fi
echo "CASE ask_user_question_create_session PASS session_id=${session_id}"

question_input="You must call tool 'AskUserQuestion' exactly once with header Execution Mode, question Choose mode, options plan and direct. After the tool call, respond with exactly ASK_USER_QUESTION_OK"
question_payload="$(build_payload "${question_input}")"
question_json="$(send_message_response "${session_id}" "${question_payload}" "question")"
log_case_json "tool_ask_user_question" "${question_payload}" "${question_json}"

question_output="$(extract_output_text "${question_json}")"
if [[ -z "${question_output}" || "${question_output}" == Error:* ]]; then
  echo "CASE tool_ask_user_question FAIL response=${question_json}"
  exit 1
fi
if ! echo "${question_output}" | grep -Fq "ASK_USER_QUESTION_OK"; then
  echo "CASE tool_ask_user_question FAIL missing_ack output=${question_output}"
  exit 1
fi

QUESTION_JSON="${question_json}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["QUESTION_JSON"])
results = [item for item in data.get("output", []) if item.get("type") == "tool_result"]
assert results, "missing tool_result"
result = results[0]
assert result.get("tool_name") == "AskUserQuestion", result.get("tool_name")
content_items = result.get("content") or []
assert content_items, "missing content payload"
content = content_items[0].get("value") or {}
assert content.get("awaiting_user_response") is True, content
question = content.get("question") or {}
assert question.get("request", {}).get("header") == "Execution Mode", question
PY

echo "CASE tool_ask_user_question PASS"
echo "DETAIL tool_ask_user_question_output=${question_output}"
