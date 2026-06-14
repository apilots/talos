#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../scripts/common.sh"

load_env
require_command curl

start_server
trap stop_server EXIT

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
log_case_json "select_models" "{}" "${models_json}"
primary_model="$(select_e2e_model_from_json "${models_json}")"
secondary_model="$(python3 -c 'import json,sys; data=json.load(sys.stdin); arr=data.get("data",[]); print((arr[1].get("id") if len(arr) > 1 else (arr[0].get("id") if arr else "")))' <<< "${models_json}")"
if [[ -z "${primary_model}" || -z "${secondary_model}" ]]; then
  echo "CASE select_models FAIL models=${models_json}"
  exit 1
fi
echo "CASE select_models PASS primary=${primary_model} secondary=${secondary_model}"

removed_create_body="$(scenario_report_path removed_create_session_response.json)"
removed_create_status="$(curl -sS -o "${removed_create_body}" -w "%{http_code}" \
  -X POST "${GATEWAY_URL}/v1/sessions" \
  -H "Content-Type: application/json" \
  -d '{}')"
log_case_json "explicit_session_create_removed" "{}" "$(cat "${removed_create_body}")"
if [[ "${removed_create_status}" != "404" && "${removed_create_status}" != "405" ]]; then
  echo "CASE explicit_session_create_removed FAIL status=${removed_create_status} body=$(cat "${removed_create_body}")"
  exit 1
fi
echo "CASE explicit_session_create_removed PASS status=${removed_create_status}"

create_payload='{
  "context": {
    "source_type": "http",
    "provider": "http",
    "channel_id": "session",
    "user_id": "session-user",
    "thread_id": null,
    "message_id": "msg-session-lifecycle",
    "trace_id": "trace-session-lifecycle"
  },
  "model": "__PRIMARY_MODEL__"
}'
create_payload="${create_payload/__PRIMARY_MODEL__/${primary_model}}"
create_id="$(new_session_id)"
remember_session_context "${create_id}" "${create_payload}"
create_json="{\"id\":\"${create_id}\"}"
log_case_json "session_id" "${create_payload}" "${create_json}"
session_id="$(session_id_from_json "${create_json}")"
if [[ -z "${session_id}" ]]; then
  echo "CASE session_id FAIL response=${create_json}"
  exit 1
fi
echo "CASE session_id PASS session_id=${session_id}"

first_message_payload='{
  "input": [{"type":"text","text":"Reply with exactly SESSION_READY."}],
  "workspace_id": null
}'
first_message_json="$(send_message_response "${session_id}" "${first_message_payload}" "session_lifecycle_first_message")"
log_case_json "first_message" "${first_message_payload}" "${first_message_json}"
first_message_session_id="$(json_get "${first_message_json}" "session_id")"
first_message_stop="$(json_get "${first_message_json}" "stop_reason")"
if [[ "${first_message_session_id}" != "${session_id}" || "${first_message_stop}" == "error" ]]; then
  echo "CASE first_message FAIL response=${first_message_json}"
  exit 1
fi
echo "CASE first_message PASS session_id=${first_message_session_id}"

detail_json="$(curl -sS "${GATEWAY_URL}/v1/sessions/${session_id}")"
log_case_json "get_session" "{\"session_id\":\"${session_id}\"}" "${detail_json}"
detail_id="$(session_id_from_json "${detail_json}")"
if [[ "${detail_id}" != "${session_id}" ]]; then
  echo "CASE get_session FAIL expected=${session_id} actual=${detail_id}"
  exit 1
fi
echo "CASE get_session PASS id=${detail_id}"

patch_json="$(curl -sS -X PATCH "${GATEWAY_URL}/v1/sessions/${session_id}" \
  -H "Content-Type: application/json" \
  -d "{\"conf\":{\"agent.model\":\"${secondary_model}\"}}")"
log_case_json "patch_session_model" "{\"session_id\":\"${session_id}\",\"conf\":{\"agent.model\":\"${secondary_model}\"}}" "${patch_json}"
patch_success="$(json_get "${patch_json}" "success")"
if [[ "${patch_success}" != "true" && "${patch_success}" != "True" ]]; then
  echo "CASE patch_session_model FAIL response=${patch_json}"
  exit 1
fi
echo "CASE patch_session_model PASS success=${patch_success}"

list_json="$(curl -sS "${GATEWAY_URL}/v1/sessions?user_id=session-user&channel_id=session&provider=http")"
log_case_json "list_sessions_with_filter" "{\"user_id\":\"session-user\",\"channel_id\":\"session\",\"provider\":\"http\"}" "${list_json}"
list_count="$(python3 -c 'import json,sys; print(len(json.load(sys.stdin).get("data", [])))' <<< "${list_json}")"
if [[ "${list_count}" -lt 1 ]]; then
  echo "CASE list_sessions_with_filter FAIL expected=>=1 actual=${list_count}"
  exit 1
fi
echo "CASE list_sessions_with_filter PASS count=${list_count}"
