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

build_session_start_defaults() {
  python3 - <<'PY'
import json
import os

payload = {
    "modelSelection": {"modelRef": {"provider": os.environ["PRIMARY_PROVIDER"], "id": os.environ["PRIMARY_MODEL"]}, "reasoningEffort": None},
}
print(json.dumps(payload, ensure_ascii=False))
PY
}

memory_stream_session_payload="$(build_session_start_defaults)"
memory_stream_session_id="$(new_session_id)"
remember_session_start_defaults "${memory_stream_session_id}" "${memory_stream_session_payload}"
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
    "input": {
        "content": [
            {
                "type": "text",
                "text": "You must call MemorySearch exactly once with query talos项目, then answer in one short sentence.",
            }
        ]
    },
}
print(json.dumps(payload, ensure_ascii=False))
PY
)"
memory_stream_log="$(scenario_report_path memory_search_stream_events.log)"
send_message_sse_log "${memory_stream_session_id}" "${memory_stream_payload}" "${memory_stream_log}"
log_case_json "messages_memory_search_stream" "${memory_stream_payload}" "$(cat "${memory_stream_log}")"

memory_stream_tool_started_count="$(grep -c '"type":"toolCall"' "${memory_stream_log}" || true)"
memory_stream_tool_completed_count="$(grep -c '"type":"toolResult"' "${memory_stream_log}" || true)"
memory_stream_completed_count="$(grep -c '^event: timeline/turn/completed' "${memory_stream_log}" || true)"
memory_stream_tool_started_name_count="$(grep -c '"name":"MemorySearch"' "${memory_stream_log}" || true)"
memory_stream_tool_completed_name_count="$(grep -c '"toolName":"MemorySearch"' "${memory_stream_log}" || true)"
if [[ "${memory_stream_tool_started_count}" -lt 1 || "${memory_stream_tool_completed_count}" -lt 1 || "${memory_stream_completed_count}" -lt 1 ]]; then
  echo "CASE messages_memory_search_stream FAIL tool_started=${memory_stream_tool_started_count} tool_completed=${memory_stream_tool_completed_count} completed=${memory_stream_completed_count}"
  exit 1
fi
if [[ "${memory_stream_tool_started_name_count}" -lt 1 || "${memory_stream_tool_completed_name_count}" -lt 1 ]]; then
  echo "CASE messages_memory_search_stream FAIL missing_memory_search_payload started_name=${memory_stream_tool_started_name_count} completed_name=${memory_stream_tool_completed_name_count}"
  exit 1
fi
memory_stream_call_ids="$(python3 - "${memory_stream_log}" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
started_ids = set()
completed_ids = set()
result_payload_count = 0
for line in path.read_text().splitlines():
    if not line.startswith("data: "):
        continue
    raw_record = json.loads(line[6:])
    event = raw_record.get("event") if isinstance(raw_record.get("event"), dict) else {}
    payload = event.get("payload") or {}
    if payload.get("type") != "messageBlockCompleted":
        continue
    block = (payload.get("data") or {}).get("block") or {}
    content = block.get("content") or {}
    data = content.get("data") or {}
    if content.get("type") == "toolCall" and data.get("name") == "MemorySearch":
        started_ids.add(data.get("callId"))
    elif content.get("type") == "toolResult" and data.get("toolName") == "MemorySearch":
        completed_ids.add(data.get("callId"))
        content_items = data.get("content") or []
        if any(
            item.get("type") == "json"
            and isinstance(item.get("value"), dict)
            and isinstance(item["value"].get("data"), list)
            for item in content_items
            if isinstance(item, dict)
        ):
            result_payload_count += 1
started = next(iter(started_ids - {None}), "")
completed = started if started in completed_ids else ""
print(f"{started or ''}\t{completed or ''}\t{result_payload_count}")
PY
)"
memory_stream_started_call_id="${memory_stream_call_ids%%$'\t'*}"
memory_stream_completed_call_id="${memory_stream_call_ids#*$'\t'}"
memory_stream_completed_call_id="${memory_stream_completed_call_id%%$'\t'*}"
memory_stream_results_count="${memory_stream_call_ids##*$'\t'}"
if [[ -z "${memory_stream_started_call_id}" || -z "${memory_stream_completed_call_id}" ]]; then
  echo "CASE messages_memory_search_stream FAIL missing_call_id started=${memory_stream_started_call_id} completed=${memory_stream_completed_call_id}"
  exit 1
fi
if [[ "${memory_stream_started_call_id}" != "${memory_stream_completed_call_id}" || "${memory_stream_results_count}" -lt 1 ]]; then
  echo "CASE messages_memory_search_stream FAIL call_id_mismatch started=${memory_stream_started_call_id} completed=${memory_stream_completed_call_id}"
  exit 1
fi
echo "CASE messages_memory_search_stream PASS tool_started=${memory_stream_tool_started_count} tool_completed=${memory_stream_tool_completed_count} completed=${memory_stream_completed_count} started_name=${memory_stream_tool_started_name_count} completed_name=${memory_stream_tool_completed_name_count} results=${memory_stream_results_count}"
