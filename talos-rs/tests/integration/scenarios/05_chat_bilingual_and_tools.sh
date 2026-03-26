#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command curl

if [[ -z "${BAILIAN_API_KEY:-}" && -z "${OPENAI_API_KEY:-}" && -z "${ANTHROPIC_API_KEY:-}" ]]; then
  echo "CASE real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE real_llm_precheck PASS key_detected=true"

start_server
trap stop_server EXIT

build_payload() {
  local input_text="$1"
  INPUT_TEXT="$input_text" python3 - <<'PY'
import json
import os

payload = {
    "input": [
        {
            "type": "text",
            "text": os.environ["INPUT_TEXT"]
        }
    ],
    "stream": False,
    "workspace_id": None,
}
print(json.dumps(payload, ensure_ascii=False))
PY
}

create_session() {
  local user_id="$1"
  USER_ID="$user_id" python3 - <<'PY'
import json
import os

payload = {
    "context": {
        "source_type": "http",
        "provider": "http",
        "channel_id": "bilingual-tools",
        "user_id": os.environ["USER_ID"],
        "thread_id": None,
        "message_id": "msg-bilingual-tools",
        "trace_id": "trace-bilingual-tools",
    },
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

en_session_payload="$(create_session "lang-user-en")"
en_session_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${en_session_payload}")"
en_session_id="$(json_get "${en_session_json}" "id")"
if [[ -z "${en_session_id}" ]]; then
  echo "CASE messages_english_create_session FAIL response=${en_session_json}"
  exit 1
fi
echo "CASE messages_english_create_session PASS session_id=${en_session_id}"

english_input="Please respond in English with one short sentence containing 'english-ok'."
english_payload="$(build_payload "${english_input}")"
english_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions/${en_session_id}/messages" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${english_payload}")"
log_case_json "messages_english" "${english_payload}" "${english_json}"
english_output="$(extract_output_text "${english_json}")"
english_stop="$(json_get "${english_json}" "stop_reason")"
if [[ -z "${english_output}" || "${english_stop}" == "error" || "${english_output}" == Error:* ]]; then
  echo "CASE messages_english FAIL response=${english_json}"
  exit 1
fi
if ! echo "${english_output}" | grep -qi "english-ok"; then
  echo "CASE messages_english FAIL missing=english-ok output=${english_output}"
  exit 1
fi
echo "CASE messages_english PASS"
echo "DETAIL messages_english_output=${english_output}"

zh_session_payload="$(create_session "lang-user-zh")"
zh_session_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${zh_session_payload}")"
zh_session_id="$(json_get "${zh_session_json}" "id")"
if [[ -z "${zh_session_id}" ]]; then
  echo "CASE messages_chinese_create_session FAIL response=${zh_session_json}"
  exit 1
fi
echo "CASE messages_chinese_create_session PASS session_id=${zh_session_id}"

chinese_input="请用中文回复一句话，并包含“中文通过”四个字。"
chinese_payload="$(build_payload "${chinese_input}")"
chinese_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions/${zh_session_id}/messages" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${chinese_payload}")"
log_case_json "messages_chinese" "${chinese_payload}" "${chinese_json}"
chinese_output="$(extract_output_text "${chinese_json}")"
chinese_stop="$(json_get "${chinese_json}" "stop_reason")"
if [[ -z "${chinese_output}" || "${chinese_stop}" == "error" || "${chinese_output}" == Error:* ]]; then
  echo "CASE messages_chinese FAIL response=${chinese_json}"
  exit 1
fi
if ! echo "${chinese_output}" | grep -q "中文通过"; then
  echo "CASE messages_chinese FAIL missing=中文通过 output=${chinese_output}"
  exit 1
fi
echo "CASE messages_chinese PASS"
echo "DETAIL messages_chinese_output=${chinese_output}"

tool_session_payload="$(create_session "tool-user")"
tool_session_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${tool_session_payload}")"
tool_session_id="$(json_get "${tool_session_json}" "id")"
if [[ -z "${tool_session_id}" ]]; then
  echo "CASE messages_tool_exec_create_session FAIL response=${tool_session_json}"
  exit 1
fi
echo "CASE messages_tool_exec_create_session PASS session_id=${tool_session_id}"

tool_input="You must call tool 'exec' exactly once with command ls, then return the raw stdout inside <tool_output></tool_output>."
tool_payload="$(build_payload "${tool_input}")"
tool_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions/${tool_session_id}/messages" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${tool_payload}")"
log_case_json "messages_tool_exec" "${tool_payload}" "${tool_json}"
tool_output="$(extract_output_text "${tool_json}")"
tool_stop="$(json_get "${tool_json}" "stop_reason")"
if [[ -z "${tool_output}" || "${tool_stop}" == "error" || "${tool_output}" == Error:* ]]; then
  echo "CASE messages_tool_exec FAIL response=${tool_json}"
  exit 1
fi

tool_call_count="$(python3 -c 'import json,sys; d=json.load(sys.stdin); print(sum(1 for i in d.get("output", []) if i.get("type")=="tool_call"))' <<< "${tool_json}")"
tool_result_count="$(python3 -c 'import json,sys; d=json.load(sys.stdin); print(sum(1 for i in d.get("output", []) if i.get("type")=="tool_result"))' <<< "${tool_json}")"
if [[ "${tool_call_count}" -lt 1 || "${tool_result_count}" -lt 1 ]]; then
  echo "CASE messages_tool_exec FAIL missing_structured_tool_items call=${tool_call_count} result=${tool_result_count}"
  exit 1
fi

tool_call_name="$(python3 -c 'import json,sys; d=json.load(sys.stdin); calls=[i for i in d.get("output", []) if i.get("type")=="tool_call"]; print(calls[0].get("name","") if calls else "")' <<< "${tool_json}")"
tool_result_tool="$(python3 -c 'import json,sys; d=json.load(sys.stdin); results=[i for i in d.get("output", []) if i.get("type")=="tool_result"]; print((((results[0].get("content") or [{}])[0].get("value") or {}).get("tool","")) if results else "")' <<< "${tool_json}")"
reasoning_count="$(python3 -c 'import json,sys; d=json.load(sys.stdin); print(sum(1 for i in d.get("output", []) if i.get("type")=="reasoning"))' <<< "${tool_json}")"
if [[ "${tool_call_name}" != "exec" || "${tool_result_tool}" != "exec" ]]; then
  echo "CASE messages_tool_exec FAIL unexpected_tool call=${tool_call_name} result=${tool_result_tool}"
  exit 1
fi
if [[ "${reasoning_count}" -lt 1 ]]; then
  echo "CASE messages_tool_exec FAIL missing_reasoning_in_output"
  exit 1
fi

if ! echo "${tool_output}" | grep -q "Cargo.toml"; then
  echo "CASE messages_tool_exec FAIL missing=Cargo.toml output=${tool_output}"
  exit 1
fi

echo "CASE messages_tool_exec PASS"
echo "DETAIL messages_tool_exec_output=${tool_output}"
echo "DETAIL messages_tool_exec_tool_call_count=${tool_call_count}"
echo "DETAIL messages_tool_exec_tool_result_count=${tool_result_count}"
echo "DETAIL messages_tool_exec_tool_call_name=${tool_call_name}"
echo "DETAIL messages_tool_exec_tool_result_tool=${tool_result_tool}"
echo "DETAIL messages_tool_exec_reasoning_count=${reasoning_count}"

stream_tool_session_payload="$(create_session "tool-user-stream")"
stream_tool_session_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${stream_tool_session_payload}")"
stream_tool_session_id="$(json_get "${stream_tool_session_json}" "id")"
if [[ -z "${stream_tool_session_id}" ]]; then
  echo "CASE messages_tool_exec_stream_create_session FAIL response=${stream_tool_session_json}"
  exit 1
fi
echo "CASE messages_tool_exec_stream_create_session PASS session_id=${stream_tool_session_id}"

stream_tool_payload="$(python3 - <<'PY'
import json
payload = {
    "input": [
        {
            "type": "text",
            "text": "You must call exec tool with command ls, then summarize in one short sentence.",
        }
    ],
    "stream": True,
}
print(json.dumps(payload, ensure_ascii=False))
PY
)"
stream_tool_log="${ROOT_DIR}/tests/integration/reports/tool_stream_events.log"
curl -sS -N -X POST "${GATEWAY_URL}/v1/sessions/${stream_tool_session_id}/messages" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${stream_tool_payload}" > "${stream_tool_log}"
log_case_json "messages_tool_exec_stream" "${stream_tool_payload}" "$(cat "${stream_tool_log}")"

stream_started_count="$(grep -c '^event: message.started' "${stream_tool_log}" || true)"
stream_reasoning_count="$(grep -c '^event: message.reasoning.delta' "${stream_tool_log}" || true)"
stream_tool_started_count="$(grep -c '^event: message.tool_call.started' "${stream_tool_log}" || true)"
stream_tool_completed_count="$(grep -c '^event: message.tool_call.completed' "${stream_tool_log}" || true)"
stream_output_count="$(grep -c '^event: message.output.delta' "${stream_tool_log}" || true)"
stream_completed_count="$(grep -c '^event: message.completed' "${stream_tool_log}" || true)"
stream_step_id_count="$(grep -c '"step_id"' "${stream_tool_log}" || true)"
stream_iteration_count="$(grep -c '"iteration"' "${stream_tool_log}" || true)"
stream_request_id_count="$(grep -c '"request_id"' "${stream_tool_log}" || true)"
if [[ "${stream_started_count}" -lt 1 || "${stream_reasoning_count}" -lt 1 || "${stream_tool_started_count}" -lt 1 || "${stream_tool_completed_count}" -lt 1 || "${stream_output_count}" -lt 1 || "${stream_completed_count}" -lt 1 || "${stream_step_id_count}" -lt 1 || "${stream_iteration_count}" -lt 1 || "${stream_request_id_count}" -lt 1 ]]; then
  echo "CASE messages_tool_exec_stream FAIL started=${stream_started_count} reasoning=${stream_reasoning_count} tool_started=${stream_tool_started_count} tool_completed=${stream_tool_completed_count} output=${stream_output_count} completed=${stream_completed_count}"
  exit 1
fi
echo "CASE messages_tool_exec_stream PASS started=${stream_started_count} reasoning=${stream_reasoning_count} tool_started=${stream_tool_started_count} tool_completed=${stream_tool_completed_count} output=${stream_output_count} completed=${stream_completed_count} step_id=${stream_step_id_count} iteration=${stream_iteration_count} request_id=${stream_request_id_count}"
