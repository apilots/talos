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

create_payload='{
  "context": {
    "source_type": "http",
    "provider": "http",
    "channel_id": "session-messages",
    "user_id": "msg-user",
    "thread_id": null,
    "message_id": "msg-messages-http",
    "trace_id": "trace-messages-http"
  }
}'
create_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${create_payload}")"
log_case_json "messages_create_session" "${create_payload}" "${create_json}"
session_id="$(session_id_from_json "${create_json}")"
if [[ -z "${session_id}" ]]; then
  echo "CASE messages_create_session FAIL response=${create_json}"
  exit 1
fi
echo "CASE messages_create_session PASS session_id=${session_id}"

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

msg_payload='{
  "input": [{"type":"text","text":"please answer with one short sentence"}],
  "stream": false,
  "workspace_id": null
}'
msg_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions/${session_id}/messages" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${msg_payload}")"
log_case_json "messages_non_stream" "${msg_payload}" "${msg_json}"
output_text="$(extract_output_text "${msg_json}")"
message_id="$(json_get "${msg_json}" "message_id")"
stop_reason="$(json_get "${msg_json}" "stop_reason")"
message_usage_total="$(usage_total_from_json "${msg_json}")"
if [[ -z "${output_text}" || -z "${message_id}" || "${stop_reason}" == "error" || "${output_text}" == Error:* || -z "${message_usage_total}" || "${message_usage_total}" -le 0 ]]; then
  echo "CASE messages_non_stream FAIL response=${msg_json}"
  exit 1
fi
echo "CASE messages_non_stream PASS message_id=${message_id} session_id=${session_id}"
echo "DETAIL messages_non_stream_output=${output_text}"
echo "DETAIL messages_non_stream_usage_total=${message_usage_total}"

session_after_msg_json="$(curl -sS -H "Authorization: Bearer ${AUTH_TOKEN}" "${GATEWAY_URL}/v1/sessions/${session_id}")"
log_case_json "messages_session_usage_after_non_stream" "{\"session_id\":\"${session_id}\"}" "${session_after_msg_json}"
session_usage_total="$(usage_total_from_json "${session_after_msg_json}")"
if [[ -z "${session_usage_total}" || "${session_usage_total}" -le 0 ]]; then
  echo "CASE messages_session_usage_after_non_stream FAIL response=${session_after_msg_json}"
  exit 1
fi
echo "CASE messages_session_usage_after_non_stream PASS total_tokens=${session_usage_total}"

sse_log="${ROOT_DIR}/tests/integration/reports/sse_events.log"
sse_payload='{
  "input": [{"type":"text","text":"please answer with one short sentence"}],
  "stream": true,
  "workspace_id": null
}'
curl -sS -N -X POST "${GATEWAY_URL}/v1/sessions/${session_id}/messages" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${sse_payload}" > "${sse_log}"
log_case_json "messages_sse_stream" "${sse_payload}" "$(cat "${sse_log}")"

started_count="$(grep -c '^event: message.started' "${sse_log}" || true)"
completed_count="$(grep -c '^event: message.completed' "${sse_log}" || true)"
stream_usage_total="$(python3 - "${sse_log}" <<'PY'
import json
import sys

path = sys.argv[1]
usage = 0
with open(path, "r", encoding="utf-8") as handle:
    for line in handle:
        if not line.startswith("data: "):
            continue
        try:
            event = json.loads(line[6:])
        except Exception:
            continue
        if event.get("event_type") != "message_completed":
            continue
        usage = (
            event.get("data", {})
            .get("usage", {})
            .get("total_tokens", 0)
        )
print(usage)
PY
)"
if [[ "${started_count}" -lt 1 || "${completed_count}" -lt 1 ]]; then
  echo "CASE messages_sse_stream FAIL started=${started_count} completed=${completed_count}"
  exit 1
fi
echo "CASE messages_sse_stream PASS started=${started_count} completed=${completed_count} usage_total=${stream_usage_total}"
echo "DETAIL messages_sse_events_begin"
head -n 40 "${sse_log}"
echo "DETAIL messages_sse_events_end"

first_request="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions/${session_id}/messages" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -H "Idempotency-Key: scenario-idem-1" \
  -d "${msg_payload}")"
second_request="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions/${session_id}/messages" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -H "Idempotency-Key: scenario-idem-1" \
  -d "${msg_payload}")"
log_case_json "messages_idempotency_first" "${msg_payload}" "${first_request}"
log_case_json "messages_idempotency_second" "${msg_payload}" "${second_request}"
id1="$(json_get "${first_request}" "message_id")"
id2="$(json_get "${second_request}" "message_id")"
first_stop="$(json_get "${first_request}" "stop_reason")"
first_output="$(extract_output_text "${first_request}")"
second_output="$(extract_output_text "${second_request}")"
if [[ -z "${id1}" || "${id1}" != "${id2}" || "${first_stop}" == "error" ]]; then
  echo "CASE messages_idempotency FAIL first=${id1} second=${id2}"
  exit 1
fi
echo "CASE messages_idempotency PASS message_id=${id1}"
echo "DETAIL messages_idempotency_first_output=${first_output}"
echo "DETAIL messages_idempotency_second_output=${second_output}"
