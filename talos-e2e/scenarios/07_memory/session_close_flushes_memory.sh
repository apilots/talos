#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

require_command curl
require_command python3
require_command mktemp

TEST_TALOS_HOME="$(make_e2e_temp_dir "talos-close-flush-home")"
cleanup() {
  stop_server
  rm -rf "${TEST_TALOS_HOME}"
}
export TALOS_HOME="${TEST_TALOS_HOME}"

start_server
trap cleanup EXIT

workspace_info_json="$(curl -sS "${GATEWAY_URL}/v1/workspace/info")"
WORKSPACE_DIR="$(json_get "${workspace_info_json}" "user_workspace_dir")"
if [[ -z "${WORKSPACE_DIR}" ]]; then
  echo "CASE workspace_info FAIL response=${workspace_info_json}"
  exit 1
fi
echo "CASE workspace_info PASS user_workspace_dir=${WORKSPACE_DIR}"
ROOT_MEMORY_FILE="${WORKSPACE_DIR}/MEMORY.md"
DAY_FILE="${WORKSPACE_DIR}/memory/$(date '+%Y-%m-%d').md"
mkdir -p "${WORKSPACE_DIR}"

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
primary_model="$(select_e2e_model_from_json "${models_json}")"
primary_provider="$(select_e2e_model_provider_from_json "${models_json}" "${primary_model}")"
if [[ -z "${primary_model}" || -z "${primary_provider}" ]]; then
  echo "CASE close_flush_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
export PRIMARY_PROVIDER="${primary_provider}"
echo "CASE close_flush_select_model PASS model=${primary_model}"

pre_size=0
if [[ -f "${DAY_FILE}" ]]; then
  pre_size="$(wc -c < "${DAY_FILE}" | tr -d ' ')"
fi

message_marker="close-flush-$(date '+%Y%m%d%H%M%S')-$$"

create_payload="$(python3 - <<'PY'
import json
import os

payload = {
    "context": {
        "source_type": "http",
        "provider": "http",
        "channel_id": "session",
        "user_id": "session-close-user",
        "thread_id": None,
        "message_id": "msg-session-close-flush",
        "trace_id": "trace-session-close-flush",
    },
    "modelSelection": {"modelRef": {"provider": os.environ["PRIMARY_PROVIDER"], "id": os.environ["PRIMARY_MODEL"]}, "reasoning": {"type": "modelDefault"}},
}
print(json.dumps(payload, ensure_ascii=False))
PY
)"
create_id="$(new_session_id)"
remember_session_context "${create_id}" "${create_payload}"
create_json="{\"id\":\"${create_id}\"}"
log_case_json "session_id_for_close_flush" "${create_payload}" "${create_json}"
session_id="$(session_id_from_json "${create_json}")"
if [[ -z "${session_id}" ]]; then
  echo "CASE session_id_for_close_flush FAIL response=${create_json}"
  exit 1
fi
echo "CASE session_id_for_close_flush PASS session_id=${session_id}"

message_payload='{
  "input": [
    {
      "type": "text",
      "text": "__MESSAGE_MARKER__ Reply with exactly CLOSE_FLUSH_READY. Do not call tools."
    }
  ],
  "workspace_id": null
}'
message_payload="${message_payload/__MESSAGE_MARKER__/${message_marker}}"
message_json="$(send_message_response "${session_id}" "${message_payload}" "message")"
log_case_json "send_message_for_close_flush" "${message_payload}" "${message_json}"
message_session_id="$(json_get "${message_json}" "session_id")"
if [[ "${message_session_id}" != "${session_id}" ]]; then
  echo "CASE send_message_for_close_flush FAIL expected=${session_id} actual=${message_session_id}"
  exit 1
fi
message_stop="$(json_get "${message_json}" "stop_reason")"
message_output="$(MESSAGE_JSON="${message_json}" python3 - <<'PY'
import json
import os
import sys

try:
    data = json.loads(os.environ.get("MESSAGE_JSON", ""))
except Exception:
    print("")
    sys.exit(0)

print("\n".join(
    item.get("text", "")
    for item in data.get("output", [])
    if item.get("type") == "output_text"
))
PY
)"
if [[ "${message_stop}" == "error" || "${message_output}" == Error:* ]]; then
  echo "CASE send_message_for_close_flush FAIL response=${message_json}"
  exit 1
fi
echo "CASE send_message_for_close_flush PASS session_id=${message_session_id}"

if [[ ! -f "${ROOT_MEMORY_FILE}" ]]; then
  echo "CASE root_memory_bootstrap FAIL missing=${ROOT_MEMORY_FILE}"
  exit 1
fi
echo "CASE root_memory_bootstrap PASS path=${ROOT_MEMORY_FILE}"

close_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions/${session_id}/close" \
 )"
log_case_json "close_session_for_memory_flush" "{\"session_id\":\"${session_id}\"}" "${close_json}"
close_success="$(json_get "${close_json}" "success")"
if [[ "${close_success}" != "true" && "${close_success}" != "True" ]]; then
  echo "CASE close_session_for_memory_flush FAIL response=${close_json}"
  exit 1
fi

post_size="${pre_size}"
found_session_entry="false"
flush_log_seen="false"
for _ in $(seq 1 480); do
  if [[ -f "${DAY_FILE}" ]]; then
    post_size="$(wc -c < "${DAY_FILE}" | tr -d ' ')"
    if grep -Fq "session: ${session_id}" "${DAY_FILE}"; then
      found_session_entry="true"
    fi
  fi
  if grep -Fq "memory flush completed session_id=${session_id}" "${SERVER_LOG}"; then
    flush_log_seen="true"
  fi
  if [[ "${found_session_entry}" == "true" && "${flush_log_seen}" == "true" ]]; then
    break
  fi
  sleep 0.25
done

if [[ ! -f "${DAY_FILE}" ]]; then
  echo "CASE close_session_for_memory_flush FAIL missing_day_file=${DAY_FILE}"
  exit 1
fi

if [[ "${found_session_entry}" != "true" ]]; then
  echo "CASE close_session_for_memory_flush FAIL session_id=${session_id} marker=${message_marker} pre_size=${pre_size} post_size=${post_size}"
  exit 1
fi

if [[ "${flush_log_seen}" != "true" ]]; then
  echo "CASE close_session_for_memory_flush FAIL missing_flush_log session_id=${session_id}"
  exit 1
fi

echo "CASE close_session_for_memory_flush PASS session_id=${session_id} marker=${message_marker} pre_size=${pre_size} post_size=${post_size}"
