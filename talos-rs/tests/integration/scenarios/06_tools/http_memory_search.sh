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

models_json="$(curl -sS -H "Authorization: Bearer ${AUTH_TOKEN}" "${GATEWAY_URL}/v1/models")"
primary_model="$(python3 -c 'import json,sys; data=json.load(sys.stdin); arr=data.get("data",[]); print((arr[0].get("id") if arr else ""))' <<< "${models_json}")"
if [[ -z "${primary_model}" ]]; then
  echo "CASE select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
echo "CASE select_model PASS model=${primary_model}"

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

memory_stream_session_payload="$(create_session "memory-user-stream")"
memory_stream_session_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${memory_stream_session_payload}")"
memory_stream_session_id="$(session_id_from_json "${memory_stream_session_json}")"
if [[ -z "${memory_stream_session_id}" ]]; then
  echo "CASE messages_memory_search_stream_create_session FAIL response=${memory_stream_session_json}"
  exit 1
fi
echo "CASE messages_memory_search_stream_create_session PASS session_id=${memory_stream_session_id}"

memory_stream_payload="$(python3 - <<'PY'
import json
payload = {
    "input": [
        {
            "type": "text",
            "text": "You must call MemorySearch exactly once with query talos项目, then answer in one short sentence.",
        }
    ],
}
print(json.dumps(payload, ensure_ascii=False))
PY
)"
memory_stream_log="$(scenario_report_path memory_search_stream_events.log)"
send_message_sse_log "${memory_stream_session_id}" "${memory_stream_payload}" "${memory_stream_log}"
log_case_json "messages_memory_search_stream" "${memory_stream_payload}" "$(cat "${memory_stream_log}")"

memory_stream_tool_started_count="$(grep -c '^event: message.tool_call.started' "${memory_stream_log}" || true)"
memory_stream_tool_completed_count="$(grep -c '^event: message.tool_call.completed' "${memory_stream_log}" || true)"
memory_stream_completed_count="$(grep -c '^event: message.completed' "${memory_stream_log}" || true)"
memory_stream_tool_started_name_count="$(grep -c '"name":"MemorySearch"' "${memory_stream_log}" || true)"
memory_stream_tool_completed_name_count="$(grep -c '"tool":"MemorySearch"' "${memory_stream_log}" || true)"
memory_stream_results_count="$(grep -c '"results"' "${memory_stream_log}" || true)"
if [[ "${memory_stream_tool_started_count}" -lt 1 || "${memory_stream_tool_completed_count}" -lt 1 || "${memory_stream_completed_count}" -lt 1 ]]; then
  echo "CASE messages_memory_search_stream FAIL tool_started=${memory_stream_tool_started_count} tool_completed=${memory_stream_tool_completed_count} completed=${memory_stream_completed_count}"
  exit 1
fi
if [[ "${memory_stream_tool_started_name_count}" -lt 1 || "${memory_stream_tool_completed_name_count}" -lt 1 || "${memory_stream_results_count}" -lt 1 ]]; then
  echo "CASE messages_memory_search_stream FAIL missing_memory_search_payload started_name=${memory_stream_tool_started_name_count} completed_name=${memory_stream_tool_completed_name_count} results=${memory_stream_results_count}"
  exit 1
fi
memory_stream_call_ids="$(python3 - "${memory_stream_log}" <<'PY'
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
memory_stream_started_call_id="${memory_stream_call_ids%%$'\t'*}"
memory_stream_completed_call_id="${memory_stream_call_ids#*$'\t'}"
if [[ -z "${memory_stream_started_call_id}" || -z "${memory_stream_completed_call_id}" ]]; then
  echo "CASE messages_memory_search_stream FAIL missing_call_id started=${memory_stream_started_call_id} completed=${memory_stream_completed_call_id}"
  exit 1
fi
if [[ "${memory_stream_started_call_id}" != "${memory_stream_completed_call_id}" ]]; then
  echo "CASE messages_memory_search_stream FAIL call_id_mismatch started=${memory_stream_started_call_id} completed=${memory_stream_completed_call_id}"
  exit 1
fi
echo "CASE messages_memory_search_stream PASS tool_started=${memory_stream_tool_started_count} tool_completed=${memory_stream_tool_completed_count} completed=${memory_stream_completed_count} started_name=${memory_stream_tool_started_name_count} completed_name=${memory_stream_tool_completed_name_count} results=${memory_stream_results_count}"
