#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command curl

if [[ -z "${BAILIAN_API_KEY:-}" && -z "${BAILIAN_CODINGPLAN_API_KEY:-}" && -z "${OPENAI_API_KEY:-}" && -z "${ANTHROPIC_API_KEY:-}" ]]; then
  echo "CASE real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE real_llm_precheck PASS key_detected=true"

build_binaries
start_server
trap stop_server EXIT

models_json="$(curl -sS -H "Authorization: Bearer ${AUTH_TOKEN}" "${GATEWAY_URL}/v1/models")"
primary_model="$(python3 -c 'import json,sys; data=json.load(sys.stdin); arr=data.get("data",[]); print((arr[0].get("id") if arr else ""))' <<< "${models_json}")"
if [[ -z "${primary_model}" ]]; then
  echo "CASE select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
echo "CASE select_model PASS model=${primary_model}"

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
    "model": os.environ["PRIMARY_MODEL"],
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

tool_session_payload="$(create_session "tool-user")"
tool_session_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${tool_session_payload}")"
tool_session_id="$(session_id_from_json "${tool_session_json}")"
if [[ -z "${tool_session_id}" ]]; then
  echo "CASE messages_tool_exec_create_session FAIL response=${tool_session_json}"
  exit 1
fi
echo "CASE messages_tool_exec_create_session PASS session_id=${tool_session_id}"

tool_input="You must call tool 'Bash' exactly once with command ls, then return the raw stdout inside <tool_output></tool_output>."
tool_payload="$(build_payload "${tool_input}")"
tool_json="$(send_message_response "${tool_session_id}" "${tool_payload}" "tool")"
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
tool_result_tool="$(python3 -c 'import json,sys; d=json.load(sys.stdin); results=[i for i in d.get("output", []) if i.get("type")=="tool_result"]; print(results[0].get("tool_name","") if results else "")' <<< "${tool_json}")"
execution_summary_count="$(python3 -c 'import json,sys; d=json.load(sys.stdin); print(sum(1 for i in d.get("output", []) if i.get("type")=="execution_summary"))' <<< "${tool_json}")"
if [[ "${tool_call_name}" != "Bash" || "${tool_result_tool}" != "Bash" ]]; then
  echo "CASE messages_tool_exec FAIL unexpected_tool call=${tool_call_name} result=${tool_result_tool}"
  exit 1
fi
if [[ "${execution_summary_count}" -lt 1 ]]; then
  echo "CASE messages_tool_exec FAIL missing_execution_summary_in_output"
  exit 1
fi

echo "CASE messages_tool_exec PASS"
echo "DETAIL messages_tool_exec_output=${tool_output}"
echo "DETAIL messages_tool_exec_tool_call_count=${tool_call_count}"
echo "DETAIL messages_tool_exec_tool_result_count=${tool_result_count}"
echo "DETAIL messages_tool_exec_tool_call_name=${tool_call_name}"
echo "DETAIL messages_tool_exec_tool_result_tool=${tool_result_tool}"
echo "DETAIL messages_tool_exec_execution_summary_count=${execution_summary_count}"

stream_tool_session_payload="$(create_session "tool-user-stream")"
stream_tool_session_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${stream_tool_session_payload}")"
stream_tool_session_id="$(session_id_from_json "${stream_tool_session_json}")"
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
            "text": "You must call Bash tool with command ls, then summarize in one short sentence.",
        }
    ],
}
print(json.dumps(payload, ensure_ascii=False))
PY
)"
stream_tool_log="$(scenario_report_path tool_stream_events.log)"
send_message_sse_log "${stream_tool_session_id}" "${stream_tool_payload}" "${stream_tool_log}"
log_case_json "messages_tool_exec_stream" "${stream_tool_payload}" "$(cat "${stream_tool_log}")"

stream_started_count="$(grep -c '^event: message.started' "${stream_tool_log}" || true)"
stream_reasoning_count="$(grep -c '^event: message.reasoning.delta' "${stream_tool_log}" || true)"
stream_summary_count="$(grep -c '^event: message.execution.summary' "${stream_tool_log}" || true)"
stream_tool_queued_count="$(grep -c '^event: message.tool_call.queued' "${stream_tool_log}" || true)"
stream_tool_executing_count="$(grep -c '^event: message.tool_call.executing' "${stream_tool_log}" || true)"
stream_tool_progress_count="$(grep -c '^event: message.tool_call.progress_delta' "${stream_tool_log}" || true)"
stream_tool_started_count="$(grep -c '^event: message.tool_call.started' "${stream_tool_log}" || true)"
stream_tool_completed_count="$(grep -c '^event: message.tool_call.completed' "${stream_tool_log}" || true)"
stream_output_count="$(grep -c '^event: message.output.delta' "${stream_tool_log}" || true)"
stream_completed_count="$(grep -c '^event: message.completed' "${stream_tool_log}" || true)"
stream_step_id_count="$(grep -c '"step_id"' "${stream_tool_log}" || true)"
stream_iteration_count="$(grep -c '"iteration"' "${stream_tool_log}" || true)"
stream_request_id_count="$(grep -c '"request_id"' "${stream_tool_log}" || true)"
if [[ "${stream_started_count}" -lt 1 || "${stream_tool_queued_count}" -lt 1 || "${stream_tool_executing_count}" -lt 1 || "${stream_tool_progress_count}" -lt 1 || "${stream_tool_started_count}" -lt 1 || "${stream_tool_completed_count}" -lt 1 || "${stream_output_count}" -lt 1 || "${stream_completed_count}" -lt 1 || "${stream_step_id_count}" -lt 1 || "${stream_iteration_count}" -lt 1 || "${stream_request_id_count}" -lt 1 ]]; then
  echo "CASE messages_tool_exec_stream FAIL started=${stream_started_count} tool_queued=${stream_tool_queued_count} tool_executing=${stream_tool_executing_count} tool_progress=${stream_tool_progress_count} tool_started=${stream_tool_started_count} tool_completed=${stream_tool_completed_count} output=${stream_output_count} completed=${stream_completed_count}"
  exit 1
fi
if [[ "${stream_reasoning_count}" -lt 1 && "${stream_summary_count}" -lt 1 ]]; then
  echo "CASE messages_tool_exec_stream FAIL missing_thinking_or_summary reasoning=${stream_reasoning_count} summary=${stream_summary_count}"
  exit 1
fi
stream_tool_call_ids="$(python3 - "${stream_tool_log}" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
started = None
completed = None
for line in path.read_text().splitlines():
    if not line.startswith("data: "):
        continue
    payload = json.loads(line[6:])
    event_type = payload.get("event_type")
    event_payload = payload.get("payload", {})
    if event_type == "message_tool_call_started":
        started = event_payload.get("call_id")
    elif event_type == "message_tool_call_completed":
        completed = event_payload.get("call_id")
print(f"{started or ''}\t{completed or ''}")
PY
)"
stream_started_call_id="${stream_tool_call_ids%%$'\t'*}"
stream_completed_call_id="${stream_tool_call_ids#*$'\t'}"
if [[ -z "${stream_started_call_id}" || -z "${stream_completed_call_id}" ]]; then
  echo "CASE messages_tool_exec_stream FAIL missing_call_id started=${stream_started_call_id} completed=${stream_completed_call_id}"
  exit 1
fi
if [[ "${stream_started_call_id}" != "${stream_completed_call_id}" ]]; then
  echo "CASE messages_tool_exec_stream FAIL call_id_mismatch started=${stream_started_call_id} completed=${stream_completed_call_id}"
  exit 1
fi
echo "CASE messages_tool_exec_stream PASS started=${stream_started_count} reasoning=${stream_reasoning_count} summary=${stream_summary_count} tool_queued=${stream_tool_queued_count} tool_executing=${stream_tool_executing_count} tool_progress=${stream_tool_progress_count} tool_started=${stream_tool_started_count} tool_completed=${stream_tool_completed_count} output=${stream_output_count} completed=${stream_completed_count} step_id=${stream_step_id_count} iteration=${stream_iteration_count} request_id=${stream_request_id_count}"
