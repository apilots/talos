#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command curl

start_server
trap stop_server EXIT

models_json="$(curl -sS -H "Authorization: Bearer ${AUTH_TOKEN}" "${GATEWAY_URL}/v1/models")"
log_case_json "select_models" "{}" "${models_json}"
primary_model="$(python3 -c 'import json,sys; data=json.load(sys.stdin); arr=data.get("data",[]); print((arr[0].get("id") if arr else ""))' <<< "${models_json}")"
secondary_model="$(python3 -c 'import json,sys; data=json.load(sys.stdin); arr=data.get("data",[]); print((arr[1].get("id") if len(arr) > 1 else (arr[0].get("id") if arr else "")))' <<< "${models_json}")"
if [[ -z "${primary_model}" || -z "${secondary_model}" ]]; then
  echo "CASE select_models FAIL models=${models_json}"
  exit 1
fi
echo "CASE select_models PASS primary=${primary_model} secondary=${secondary_model}"

create_payload='{
  "context": {
    "source_type": "api",
    "provider": "scenario",
    "channel_id": "session",
    "user_id": "session-user",
    "thread_id": null,
    "message_id": null,
    "trace_id": null
  },
  "model": "__PRIMARY_MODEL__",
  "metadata": {"case":"session_lifecycle"}
}'
create_payload="${create_payload/__PRIMARY_MODEL__/${primary_model}}"
create_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${create_payload}")"
log_case_json "create_session" "${create_payload}" "${create_json}"
session_id="$(json_get "${create_json}" "id")"
if [[ -z "${session_id}" ]]; then
  echo "CASE create_session FAIL response=${create_json}"
  exit 1
fi
echo "CASE create_session PASS session_id=${session_id}"

detail_json="$(curl -sS -H "Authorization: Bearer ${AUTH_TOKEN}" "${GATEWAY_URL}/v1/sessions/${session_id}")"
log_case_json "get_session" "{\"session_id\":\"${session_id}\"}" "${detail_json}"
detail_id="$(json_get "${detail_json}" "id")"
if [[ "${detail_id}" != "${session_id}" ]]; then
  echo "CASE get_session FAIL expected=${session_id} actual=${detail_id}"
  exit 1
fi
echo "CASE get_session PASS id=${detail_id}"

patch_json="$(curl -sS -X PATCH "${GATEWAY_URL}/v1/sessions/${session_id}" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "{\"model\":\"${secondary_model}\"}")"
log_case_json "patch_session_model" "{\"session_id\":\"${session_id}\",\"model\":\"${secondary_model}\"}" "${patch_json}"
patched_model="$(json_get "${patch_json}" "model")"
if [[ "${patched_model}" != "${secondary_model}" ]]; then
  echo "CASE patch_session_model FAIL expected=${secondary_model} actual=${patched_model}"
  exit 1
fi
echo "CASE patch_session_model PASS model=${patched_model}"

list_json="$(curl -sS -H "Authorization: Bearer ${AUTH_TOKEN}" "${GATEWAY_URL}/v1/sessions?user_id=session-user&channel_id=session&provider=scenario")"
log_case_json "list_sessions_with_filter" "{\"user_id\":\"session-user\",\"channel_id\":\"session\",\"provider\":\"scenario\"}" "${list_json}"
list_count="$(python3 -c 'import json,sys; print(len(json.load(sys.stdin).get("data", [])))' <<< "${list_json}")"
if [[ "${list_count}" -lt 1 ]]; then
  echo "CASE list_sessions_with_filter FAIL expected=>=1 actual=${list_count}"
  exit 1
fi
echo "CASE list_sessions_with_filter PASS count=${list_count}"
