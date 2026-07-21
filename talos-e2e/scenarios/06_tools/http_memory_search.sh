#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

require_command curl

if ! has_real_llm_key; then
  echo "CASE real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE real_llm_precheck PASS key_detected=true"

build_binaries
start_server
trap stop_server EXIT

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
primary_model="$(select_e2e_model_from_json "${models_json}")"
primary_provider="$(select_e2e_model_provider_from_json "${models_json}" "${primary_model}")"
if [[ -z "${primary_model}" || -z "${primary_provider}" ]]; then
  echo "CASE select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
export PRIMARY_PROVIDER="${primary_provider}"
echo "CASE select_model PASS model=${primary_model}"

build_session_context() {
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
    "modelSelection": {"modelRef": {"provider": os.environ["PRIMARY_PROVIDER"], "id": os.environ["PRIMARY_MODEL"]}, "reasoningEffort": None},
}
print(json.dumps(payload, ensure_ascii=False))
PY
}

memory_stream_session_payload="$(build_session_context "memory-user-stream")"
memory_stream_session_id="$(new_session_id)"
remember_session_context "${memory_stream_session_id}" "${memory_stream_session_payload}"
memory_stream_session_json="{\"id\":\"${memory_stream_session_id}\"}"
memory_stream_session_id="$(session_id_from_json "${memory_stream_session_json}")"
if [[ -z "${memory_stream_session_id}" ]]; then
  echo "CASE messages_memory_search_stream_session_id FAIL response=${memory_stream_session_json}"
  exit 1
fi
echo "CASE messages_memory_search_stream_session_id PASS session_id=${memory_stream_session_id}"

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

memory_stream_tool_started_count="$(grep -c '"kind":"tool_call"' "${memory_stream_log}" || true)"
memory_stream_tool_completed_count="$(grep -c '"type":"tool_result"' "${memory_stream_log}" || true)"
memory_stream_completed_count="$(grep -c '^event: timeline.turn.completed' "${memory_stream_log}" || true)"
memory_stream_tool_started_name_count="$(grep -c '"name":"MemorySearch"' "${memory_stream_log}" || true)"
memory_stream_tool_completed_name_count="$(grep -c '"name":"MemorySearch"' "${memory_stream_log}" || true)"
memory_stream_results_count="$(grep -c '"data":\[' "${memory_stream_log}" || true)"
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
item_by_id = {}
result_item = None
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
            result_item = event_payload.get("item_id")
started = next((call_id for call_id in item_by_id.values() if call_id), "")
completed = item_by_id.get(result_item, "") if result_item else ""
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
