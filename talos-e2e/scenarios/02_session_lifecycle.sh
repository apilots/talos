#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../scripts/common.sh"

require_command curl

start_server
trap stop_server EXIT

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
log_case_json "select_models" "{}" "${models_json}"
models_data_json="$(api_data_json "${models_json}")"
default_selection="$(python3 -c 'import json,sys; payload=json.load(sys.stdin); print(json.dumps(payload.get("defaultModelSelection") or {}, separators=(",", ":")))' <<< "${models_data_json}")"
primary_provider="$(json_get "${default_selection}" "modelRef.provider")"
primary_model="$(json_get "${default_selection}" "modelRef.id")"
secondary_selection="$(PRIMARY_PROVIDER="${primary_provider}" python3 -c 'import json,os,sys; payload=json.load(sys.stdin); models=payload.get("models", []); item=next((model for model in models if model.get("provider") and model.get("provider") != os.environ["PRIMARY_PROVIDER"]), {}); effort=item.get("reasoning_effort") if item.get("reasoning") else None; print(json.dumps({"modelRef":{"provider":item.get("provider", ""),"id":item.get("id", "")},"reasoningEffort":effort}, separators=(",", ":")))' <<< "${models_data_json}")"
secondary_provider="$(json_get "${secondary_selection}" "modelRef.provider")"
secondary_model="$(json_get "${secondary_selection}" "modelRef.id")"
if [[ -z "${primary_model}" || -z "${primary_provider}" || -z "${secondary_provider}" || -z "${secondary_model}" ]]; then
  echo "CASE select_models FAIL models=${models_json}"
  exit 1
fi
echo "CASE select_models PASS primary=${primary_provider}:${primary_model} secondary=${secondary_provider}:${secondary_model}"

create_payload='{
  "context": {
    "source_type": "http",
    "provider": "http",
    "channel_id": "session",
    "user_id": "session-user",
    "thread_id": null,
    "message_id": "msg-session-lifecycle",
    "trace_id": "trace-session-lifecycle"
  }
}'
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
  "input": [{"type":"text","text":"Reply with exactly SESSION_READY."}]
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
detail_data_json="$(api_data_json "${detail_json}")"
detail_id="$(json_get "${detail_data_json}" "session.id")"
detail_selection="$(python3 -c 'import json,sys; payload=json.load(sys.stdin); print(json.dumps(payload.get("session", {}).get("modelSelection") or {}, separators=(",", ":")))' <<< "${detail_data_json}")"
if [[ "${detail_id}" != "${session_id}" ]] || ! json_equal "${detail_selection}" "${default_selection}"; then
  echo "CASE get_session FAIL expected_id=${session_id} actual_id=${detail_id} expected_selection=${default_selection} actual_selection=${detail_selection}"
  exit 1
fi
echo "CASE get_session PASS id=${detail_id}"

patch_json="$(curl -sS -X PATCH "${GATEWAY_URL}/v1/sessions/${session_id}" \
  -H "Content-Type: application/json" \
  -d "{\"conf\":{\"modelSelection\":${secondary_selection}}}")"
log_case_json "patch_session_model" "{\"session_id\":\"${session_id}\",\"conf\":{\"modelSelection\":${secondary_selection}}}" "${patch_json}"
patch_data_json="$(api_data_json "${patch_json}")"
patch_session_id="$(json_get "${patch_data_json}" "session.id")"
patch_selection="$(python3 -c 'import json,sys; payload=json.load(sys.stdin); print(json.dumps(payload.get("session", {}).get("modelSelection") or {}, separators=(",", ":")))' <<< "${patch_data_json}")"
if [[ "${patch_session_id}" != "${session_id}" ]] || ! json_equal "${patch_selection}" "${secondary_selection}"; then
  echo "CASE patch_session_model FAIL expected_selection=${secondary_selection} actual_selection=${patch_selection} response=${patch_json}"
  exit 1
fi
echo "CASE patch_session_model PASS session_id=${patch_session_id}"

list_json="$(curl -sS "${GATEWAY_URL}/v1/sessions?user_id=session-user&channel_id=session&provider=http")"
log_case_json "list_sessions_with_filter" "{\"user_id\":\"session-user\",\"channel_id\":\"session\",\"provider\":\"http\"}" "${list_json}"
list_data_json="$(api_data_json "${list_json}")"
list_count="$(python3 -c 'import json,sys; payload=json.load(sys.stdin); print(len(payload.get("sessions", [])))' <<< "${list_data_json}")"
if [[ "${list_count}" -lt 1 ]]; then
  echo "CASE list_sessions_with_filter FAIL expected=>=1 actual=${list_count}"
  exit 1
fi
echo "CASE list_sessions_with_filter PASS count=${list_count}"
