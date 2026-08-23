#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../scripts/common.sh"

require_command curl

if ! has_real_llm_key; then
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
  },
  "modelSelection": {
    "modelRef": {"provider": "deepseek", "id": "deepseek-v4-pro"},
    "reasoningEffort": null
  }
}'
create_id="$(new_session_id)"
remember_session_context "${create_id}" "${create_payload}"
create_json="{\"id\":\"${create_id}\"}"
log_case_json "messages_session_id" "${create_payload}" "${create_json}"
session_id="$(session_id_from_json "${create_json}")"
if [[ -z "${session_id}" ]]; then
  echo "CASE messages_session_id FAIL response=${create_json}"
  exit 1
fi
echo "CASE messages_session_id PASS session_id=${session_id}"

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
  "workspace_id": null
}'
msg_json="$(send_message_response "${session_id}" "${msg_payload}" "msg")"
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

model_usage_evidence=""
for _ in {1..20}; do
  model_usage_evidence="$(python3 - "${TALOS_HOME}/data/talos.db" "${session_id}" <<'PY'
import sqlite3
import sys

database_path, session_id = sys.argv[1:]
with sqlite3.connect(database_path) as connection:
    rows = connection.execute(
        """
        SELECT usage_kind, user_id, model_provider, model_id, input_tokens, output_tokens, total_tokens
        FROM profile_turn_usage
        WHERE session_id = ? AND usage_kind IN ('turn', 'title')
        ORDER BY id ASC
        """,
        (session_id,),
    ).fetchall()
by_kind = {row[0]: row[1:] for row in rows}
if "turn" in by_kind and "title" in by_kind:
    print("\t".join(str(value) for value in (*by_kind["turn"], *by_kind["title"])))
PY
)"
  if [[ -n "${model_usage_evidence}" ]]; then
    break
  fi
  sleep 1
done
IFS=$'\t' read -r turn_user turn_provider turn_model turn_input turn_output turn_total inner_user inner_provider inner_model inner_input inner_output inner_total <<< "${model_usage_evidence}"
if [[ -z "${turn_total}" || "${turn_user}" != "anonymous" || "${turn_provider}" != "deepseek" || "${turn_model}" != "deepseek-v4-pro" || "${turn_total}" -le 0 ]]; then
  echo "CASE session_model_turn_usage_db FAIL evidence=${model_usage_evidence}"
  exit 1
fi
echo "CASE session_model_turn_usage_db PASS user=${turn_user} kind=turn model=${turn_provider}:${turn_model} input=${turn_input} output=${turn_output} total=${turn_total}"
if [[ -z "${inner_total}" || "${inner_user}" != "anonymous" || "${inner_provider}" != "deepseek" || "${inner_model}" != "deepseek-v4-flash" || "${inner_total}" -le 0 || "${inner_model}" == "${turn_model}" ]]; then
  echo "CASE inner_model_title_usage_db FAIL evidence=${model_usage_evidence}"
  exit 1
fi
echo "CASE inner_model_title_usage_db PASS user=${inner_user} kind=title model=${inner_provider}:${inner_model} input=${inner_input} output=${inner_output} total=${inner_total}"

profile_json="$(curl -sS "${GATEWAY_URL}/v1/profile")"
log_case_json "inner_model_profile_usage" "{\"session_id\":\"${session_id}\"}" "${profile_json}"
profile_data_json="$(api_data_json "${profile_json}")"
profile_total="$(usage_total_from_json "${profile_data_json}")"
profile_inner_total="$(INNER_PROVIDER="${inner_provider}" INNER_MODEL="${inner_model}" python3 -c 'import json,os,sys; payload=json.load(sys.stdin); item=next((entry for entry in payload.get("modelUsage", []) if entry.get("modelRef") == {"provider":os.environ["INNER_PROVIDER"],"id":os.environ["INNER_MODEL"]}), {}); print(item.get("usage", {}).get("total_tokens", 0))' <<< "${profile_data_json}")"
profile_turn_total="$(TURN_PROVIDER="${turn_provider}" TURN_MODEL="${turn_model}" python3 -c 'import json,os,sys; payload=json.load(sys.stdin); item=next((entry for entry in payload.get("modelUsage", []) if entry.get("modelRef") == {"provider":os.environ["TURN_PROVIDER"],"id":os.environ["TURN_MODEL"]}), {}); print(item.get("usage", {}).get("total_tokens", 0))' <<< "${profile_data_json}")"
if [[ "${profile_total}" -lt "$((turn_total + inner_total))" || "${profile_inner_total}" -lt "${inner_total}" || "${profile_turn_total}" -lt "${turn_total}" ]]; then
  echo "CASE inner_model_profile_usage FAIL profile=${profile_json} turn_db_total=${turn_total} inner_db_total=${inner_total}"
  exit 1
fi
echo "CASE inner_model_profile_usage PASS profile_total=${profile_total} turn_model_total=${profile_turn_total} inner_model_total=${profile_inner_total}"

session_after_msg_json="$(curl -sS "${GATEWAY_URL}/v1/sessions/${session_id}")"
log_case_json "messages_session_usage_after_non_stream" "{\"session_id\":\"${session_id}\"}" "${session_after_msg_json}"
session_after_msg_data_json="$(api_data_json "${session_after_msg_json}")"
session_usage_total="$(usage_total_from_json "${session_after_msg_data_json}")"
context_used_tokens="$(json_get "${session_after_msg_data_json}" "session.usage.context.usedTokens")"
context_window_tokens="$(json_get "${session_after_msg_data_json}" "session.usage.context.contextWindowTokens")"
if [[ -z "${session_usage_total}" || "${session_usage_total}" -le 0 || -z "${context_used_tokens}" || "${context_used_tokens}" -le 0 || -z "${context_window_tokens}" || "${context_window_tokens}" -le 0 ]]; then
  echo "CASE messages_session_usage_after_non_stream FAIL response=${session_after_msg_json}"
  exit 1
fi
echo "CASE messages_session_usage_after_non_stream PASS total_tokens=${session_usage_total} context_used_tokens=${context_used_tokens} context_window_tokens=${context_window_tokens}"

sse_log="$(scenario_report_path sse_events.log)"
sse_payload='{
  "input": [{"type":"text","text":"please answer with one short sentence"}],
  "workspace_id": null
}'
send_message_sse_log "${session_id}" "${sse_payload}" "${sse_log}"
log_case_json "messages_sse_stream" "${sse_payload}" "$(cat "${sse_log}")"

started_count="$(grep -c '^event: timeline.turn.started' "${sse_log}" || true)"
completed_count="$(grep -c '^event: timeline.turn.completed' "${sse_log}" || true)"
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
            record = json.loads(line[6:])
            event = record.get("event") if isinstance(record.get("event"), dict) else record
        except Exception:
            continue
        if event.get("event_type") != "turn_completed":
            continue
        usage = (
            event.get("payload", {})
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

gone_session_id="$(new_session_id)"
remember_session_context "${gone_session_id}" "${create_payload}"
gone_session_json="{\"id\":\"${gone_session_id}\"}"
gone_session_id="$(session_id_from_json "${gone_session_json}")"
if [[ -z "${gone_session_id}" ]]; then
  echo "CASE messages_events_cursor_gone_session_id FAIL response=${gone_session_json}"
  exit 1
fi
gone_msg_json="$(send_message_response "${gone_session_id}" "${msg_payload}" "cursor_gone_msg")"
gone_msg_id="$(json_get "${gone_msg_json}" "message_id")"
if [[ -z "${gone_msg_id}" ]]; then
  echo "CASE messages_events_cursor_gone_seed FAIL response=${gone_msg_json}"
  exit 1
fi
gone_body="$(scenario_report_path cursor_gone_response.json)"
gone_status="$(curl -sS -o "${gone_body}" -w "%{http_code}" \
  -H "Accept: text/event-stream" \
  "${GATEWAY_URL}/v1/sessions/${gone_session_id}/events?after_event_id=ev_00000000000000000001")"
log_case_json "messages_events_cursor_gone" "{\"session_id\":\"${gone_session_id}\",\"after_event_id\":\"ev_00000000000000000001\"}" "$(cat "${gone_body}")"
if [[ "${gone_status}" != "410" ]]; then
  echo "CASE messages_events_cursor_gone FAIL status=${gone_status} body=$(cat "${gone_body}")"
  exit 1
fi
echo "CASE messages_events_cursor_gone PASS status=${gone_status}"

first_request="$(send_message_response "${session_id}" "${msg_payload}" "messages_idempotency_first" "scenario-idem-1")"
second_request="$(send_message_response "${session_id}" "${msg_payload}" "messages_idempotency_second" "scenario-idem-1")"
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

recovery_json="$(curl -sS "${GATEWAY_URL}/v1/sessions/${session_id}/timeline/recovery")"
log_case_json "timeline_recovery" "{\"session_id\":\"${session_id}\"}" "${recovery_json}"
RECOVERY_JSON="$(api_data_json "${recovery_json}")" python3 - <<'PY'
import json, os, sys
raw = os.environ.get("RECOVERY_JSON", "")
try:
    data = json.loads(raw)
except Exception as e:
    print(f"CASE timeline_recovery FAIL parse_error={e}")
    sys.exit(1)
snapshot = data.get("snapshot", data)
events = snapshot.get("replay", {}).get("events", [])
has_user_input = False
has_final_answer = False
assistant_items = {}
for envelope in events:
    event = envelope.get("event", {})
    payload = event.get("payload", {})
    payload_type = payload.get("type")
    if payload_type == "item_started":
        item = payload.get("item", {})
        item_id = item.get("item_id", "")
        if item.get("kind") == "user_input" and item.get("phase") == "input":
            has_user_input = True
        if item.get("kind") == "assistant_text" and item_id:
            assistant_items[item_id] = item.get("phase", "")
            if item.get("phase") == "final_answer":
                has_final_answer = True
    elif payload_type == "assistant_text_committed":
        item_id = payload.get("item_id", "")
        if item_id in assistant_items and payload.get("phase") == "final_answer":
            has_final_answer = True
if len(events) < 1 or not has_user_input or not has_final_answer:
    print(f"CASE timeline_recovery FAIL event_count={len(events)} has_user_input={has_user_input} has_final_answer={has_final_answer}")
    sys.exit(1)
print(f"CASE timeline_recovery PASS event_count={len(events)} has_user_input={has_user_input} has_final_answer={has_final_answer}")
PY
