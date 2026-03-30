#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command curl
require_command python3

start_server
trap stop_server EXIT

WORKSPACE_DIR="${HOME}/.talos/workspace"
ROOT_MEMORY_FILE="${WORKSPACE_DIR}/MEMORY.md"
DAY_FILE="${WORKSPACE_DIR}/memory/$(date '+%Y-%m-%d').md"
mkdir -p "${WORKSPACE_DIR}"

if [[ ! -f "${ROOT_MEMORY_FILE}" ]]; then
  echo "CASE root_memory_bootstrap FAIL missing=${ROOT_MEMORY_FILE}"
  exit 1
fi
echo "CASE root_memory_bootstrap PASS path=${ROOT_MEMORY_FILE}"

pre_size=0
if [[ -f "${DAY_FILE}" ]]; then
  pre_size="$(wc -c < "${DAY_FILE}" | tr -d ' ')"
fi

create_payload='{
  "context": {
    "source_type": "http",
    "provider": "http",
    "channel_id": "session",
    "user_id": "session-close-user",
    "thread_id": null,
    "message_id": "msg-session-close-flush",
    "trace_id": "trace-session-close-flush"
  },
  "model": null
}'
create_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${create_payload}")"
log_case_json "create_session_for_close_flush" "${create_payload}" "${create_json}"
session_id="$(session_id_from_json "${create_json}")"
if [[ -z "${session_id}" ]]; then
  echo "CASE create_session_for_close_flush FAIL response=${create_json}"
  exit 1
fi
echo "CASE create_session_for_close_flush PASS session_id=${session_id}"

message_payload='{
  "input": [
    {
      "type": "text",
      "text": "Please remember that this session should be flushed on close."
    }
  ],
  "stream": false,
  "workspace_id": null
}'
message_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions/${session_id}/messages" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${message_payload}")"
log_case_json "send_message_for_close_flush" "${message_payload}" "${message_json}"
message_session_id="$(json_get "${message_json}" "session_id")"
if [[ "${message_session_id}" != "${session_id}" ]]; then
  echo "CASE send_message_for_close_flush FAIL expected=${session_id} actual=${message_session_id}"
  exit 1
fi
echo "CASE send_message_for_close_flush PASS session_id=${message_session_id}"

close_json="$(curl -sS -X DELETE "${GATEWAY_URL}/v1/sessions/${session_id}" \
  -H "Authorization: Bearer ${AUTH_TOKEN}")"
log_case_json "close_session_for_memory_flush" "{\"session_id\":\"${session_id}\"}" "${close_json}"
close_success="$(json_get "${close_json}" "success")"
if [[ "${close_success}" != "true" && "${close_success}" != "True" ]]; then
  echo "CASE close_session_for_memory_flush FAIL response=${close_json}"
  exit 1
fi

post_size="${pre_size}"
for _ in $(seq 1 40); do
  if [[ -f "${DAY_FILE}" ]]; then
    post_size="$(wc -c < "${DAY_FILE}" | tr -d ' ')"
    if [[ "${post_size}" -gt "${pre_size}" ]]; then
      break
    fi
  fi
  sleep 0.25
done

if [[ ! -f "${DAY_FILE}" ]]; then
  echo "CASE close_session_for_memory_flush FAIL missing_day_file=${DAY_FILE}"
  exit 1
fi

if [[ "${post_size}" -le "${pre_size}" ]]; then
  echo "CASE close_session_for_memory_flush FAIL pre_size=${pre_size} post_size=${post_size}"
  exit 1
fi

echo "CASE close_session_for_memory_flush PASS pre_size=${pre_size} post_size=${post_size}"
