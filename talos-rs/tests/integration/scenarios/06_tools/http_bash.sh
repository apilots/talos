#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command curl

if [[ -z "${TALOS_LLM_BAILIAN_API_KEY:-}" && -z "${TALOS_LLM_ANTHROPIC_API_KEY:-}" && -z "${TALOS_LLM_OPENAI_API_KEY:-}" ]]; then
  echo "CASE real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE real_llm_precheck PASS key_detected=true"

build_binaries
start_server
trap stop_server EXIT

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
primary_model="$(select_e2e_model_from_json "${models_json}")"
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
if [[ "${tool_call_name}" != "Bash" || "${tool_result_tool}" != "Bash" ]]; then
  echo "CASE messages_tool_exec FAIL unexpected_tool call=${tool_call_name} result=${tool_result_tool}"
  exit 1
fi

echo "CASE messages_tool_exec PASS"
echo "DETAIL messages_tool_exec_output=${tool_output}"
echo "DETAIL messages_tool_exec_tool_call_count=${tool_call_count}"
echo "DETAIL messages_tool_exec_tool_result_count=${tool_result_count}"
echo "DETAIL messages_tool_exec_tool_call_name=${tool_call_name}"
echo "DETAIL messages_tool_exec_tool_result_tool=${tool_result_tool}"

stream_tool_session_payload="$(create_session "tool-user-stream")"
stream_tool_session_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions" \
  -H "Content-Type: application/json" \
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

stream_started_count="$(grep -c '^event: timeline.turn.started' "${stream_tool_log}" || true)"
stream_reasoning_count="$(grep -c '"phase":"thinking"' "${stream_tool_log}" || true)"
stream_summary_count="$(grep -c '"kind":"execution_summary"' "${stream_tool_log}" || true)"
stream_tool_started_count="$(grep -c '"kind":"tool_call"' "${stream_tool_log}" || true)"
stream_tool_progress_count="$(grep -c '"type":"tool_progress"' "${stream_tool_log}" || true)"
stream_tool_result_count="$(grep -c '"type":"tool_result"' "${stream_tool_log}" || true)"
stream_final_count="$(grep -c '"phase":"final_answer"' "${stream_tool_log}" || true)"
stream_completed_count="$(grep -c '^event: timeline.turn.completed' "${stream_tool_log}" || true)"
stream_request_id_count="$(grep -c '"request_id"' "${stream_tool_log}" || true)"
if [[ "${stream_started_count}" -lt 1 || "${stream_tool_progress_count}" -lt 1 || "${stream_tool_started_count}" -lt 1 || "${stream_tool_result_count}" -lt 1 || "${stream_final_count}" -lt 1 || "${stream_completed_count}" -lt 1 || "${stream_request_id_count}" -lt 1 ]]; then
  echo "CASE messages_tool_exec_stream FAIL started=${stream_started_count} tool_progress=${stream_tool_progress_count} tool_started=${stream_tool_started_count} tool_result=${stream_tool_result_count} final=${stream_final_count} completed=${stream_completed_count}"
  exit 1
fi
if [[ "${stream_reasoning_count}" -lt 1 && "${stream_summary_count}" -lt 1 ]]; then
  echo "CASE messages_tool_exec_stream FAIL missing_thinking_or_summary reasoning=${stream_reasoning_count} summary=${stream_summary_count}"
  exit 1
fi
stream_tool_call_check="$(python3 - "${stream_tool_log}" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
item_by_id = {}
result_pairs = []
for line in path.read_text().splitlines():
    if not line.startswith("data: "):
        continue
    raw_record = json.loads(line[6:])
    record = raw_record.get("event") if isinstance(raw_record.get("event"), dict) else raw_record
    event_type = record.get("event_type")
    event_payload = record.get("payload", {})
    if event_type == "item_started":
        item = event_payload.get("item", {})
        content = item.get("content", {})
        if item.get("kind") == "tool_call" and content.get("type") == "tool_call":
            item_by_id[item.get("item_id")] = content.get("call_id")
    elif event_type == "item_delta":
        delta = event_payload.get("delta", {})
        if delta.get("type") == "tool_result":
            item_id = event_payload.get("item_id")
            result_pairs.append((item_id, item_by_id.get(item_id, "")))
started_count = sum(1 for call_id in item_by_id.values() if call_id)
matched_result_count = sum(1 for _, call_id in result_pairs if call_id)
print(f"{started_count}\t{matched_result_count}")
PY
)"
stream_started_tool_items="${stream_tool_call_check%%$'\t'*}"
stream_matched_tool_results="${stream_tool_call_check#*$'\t'}"
if [[ "${stream_started_tool_items}" -lt 1 || "${stream_matched_tool_results}" -lt 1 ]]; then
  echo "CASE messages_tool_exec_stream FAIL missing_call_id started_items=${stream_started_tool_items} matched_results=${stream_matched_tool_results}"
  exit 1
fi
echo "CASE messages_tool_exec_stream PASS started=${stream_started_count} reasoning=${stream_reasoning_count} summary=${stream_summary_count} tool_progress=${stream_tool_progress_count} tool_started=${stream_tool_started_count} tool_result=${stream_tool_result_count} final=${stream_final_count} completed=${stream_completed_count} request_id=${stream_request_id_count} started_items=${stream_started_tool_items} matched_results=${stream_matched_tool_results}"
