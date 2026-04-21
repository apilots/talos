#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command curl

export TALOS_TEST_ENABLE_JWT_AUTH=1
export TALOS_HOME="$(scenario_report_path talos-home)"
rm -rf "${TALOS_HOME}"

request_json() {
  local method="$1"
  local url="$2"
  local body="${3:-}"
  local token="${4:-}"
  local response_file
  response_file="$(mktemp)"

  local -a args=(
    -sS
    -X "${method}"
    -o "${response_file}"
    -w "%{http_code}"
  )
  if [[ -n "${body}" ]]; then
    args+=(-H "Content-Type: application/json" -d "${body}")
  fi
  if [[ -n "${token}" ]]; then
    args+=(-H "Authorization: Bearer ${token}")
  fi
  args+=("${url}")

  HTTP_STATUS="$(curl "${args[@]}")"
  HTTP_BODY="$(cat "${response_file}")"
  rm -f "${response_file}"
}

json_array_len() {
  local json="$1"
  python3 -c '
import json
import sys

raw = sys.stdin.read()
try:
    data = json.loads(raw)
except Exception:
    print(0)
    raise SystemExit(0)
print(len(data.get("data", [])))
' <<< "${json}"
}

json_array_contains_field() {
  local json="$1"
  local field="$2"
  local expected="$3"
  FIELD_NAME="${field}" EXPECTED_VALUE="${expected}" python3 -c '
import json
import os
import sys

raw = sys.stdin.read()
field = os.environ.get("FIELD_NAME", "")
expected = os.environ.get("EXPECTED_VALUE", "")
try:
    data = json.loads(raw)
except Exception:
    print("false")
    raise SystemExit(0)

for item in data.get("data", []):
    value = item
    for part in field.split("."):
        if isinstance(value, dict):
            value = value.get(part)
        else:
            value = None
            break
    if str(value) == expected:
        print("true")
        raise SystemExit(0)
print("false")
' <<< "${json}"
}

start_server
trap stop_server EXIT

admin_register_payload='{"username":"alice-admin","password":"password123","email":"alice-admin@example.com"}'
request_json POST "${GATEWAY_URL}/v1/auth/register" "${admin_register_payload}"
log_case_json "auth_register_admin" "${admin_register_payload}" "${HTTP_BODY}"
if [[ "${HTTP_STATUS}" != "200" ]]; then
  echo "CASE auth_register_admin FAIL status=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi
admin_token="$(json_get "${HTTP_BODY}" "access_token")"
admin_role="$(json_get "${HTTP_BODY}" "user.role")"
admin_user_id="$(json_get "${HTTP_BODY}" "user.user_id")"
if [[ -z "${admin_token}" || "${admin_role}" != "admin" || -z "${admin_user_id}" ]]; then
  echo "CASE auth_register_admin FAIL body=${HTTP_BODY}"
  exit 1
fi
echo "CASE auth_register_admin PASS role=${admin_role}"

user_register_payload='{"username":"bob-user","password":"password123","email":"bob-user@example.com"}'
request_json POST "${GATEWAY_URL}/v1/auth/register" "${user_register_payload}"
log_case_json "auth_register_user" "${user_register_payload}" "${HTTP_BODY}"
if [[ "${HTTP_STATUS}" != "200" ]]; then
  echo "CASE auth_register_user FAIL status=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi
user_role="$(json_get "${HTTP_BODY}" "user.role")"
user_user_id="$(json_get "${HTTP_BODY}" "user.user_id")"
if [[ "${user_role}" != "user" || -z "${user_user_id}" ]]; then
  echo "CASE auth_register_user FAIL body=${HTTP_BODY}"
  exit 1
fi
echo "CASE auth_register_user PASS role=${user_role}"

admin_login_payload='{"username":"alice-admin","password":"password123"}'
request_json POST "${GATEWAY_URL}/v1/auth/login" "${admin_login_payload}"
log_case_json "auth_login_admin" "${admin_login_payload}" "${HTTP_BODY}"
if [[ "${HTTP_STATUS}" != "200" ]]; then
  echo "CASE auth_login_admin FAIL status=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi
admin_token="$(json_get "${HTTP_BODY}" "access_token")"
if [[ -z "${admin_token}" ]]; then
  echo "CASE auth_login_admin FAIL body=${HTTP_BODY}"
  exit 1
fi
echo "CASE auth_login_admin PASS"

user_login_payload='{"username":"bob-user","password":"password123"}'
request_json POST "${GATEWAY_URL}/v1/auth/login" "${user_login_payload}"
log_case_json "auth_login_user" "${user_login_payload}" "${HTTP_BODY}"
if [[ "${HTTP_STATUS}" != "200" ]]; then
  echo "CASE auth_login_user FAIL status=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi
user_token="$(json_get "${HTTP_BODY}" "access_token")"
if [[ -z "${user_token}" ]]; then
  echo "CASE auth_login_user FAIL body=${HTTP_BODY}"
  exit 1
fi
echo "CASE auth_login_user PASS"

request_json GET "${GATEWAY_URL}/v1/user" "" "${user_token}"
log_case_json "auth_current_user" "{}" "${HTTP_BODY}"
if [[ "${HTTP_STATUS}" != "200" ]]; then
  echo "CASE auth_current_user FAIL status=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi
current_user_id="$(json_get "${HTTP_BODY}" "user_id")"
current_username="$(json_get "${HTTP_BODY}" "username")"
if [[ "${current_user_id}" != "${user_user_id}" || "${current_username}" != "bob-user" ]]; then
  echo "CASE auth_current_user FAIL body=${HTTP_BODY}"
  exit 1
fi
echo "CASE auth_current_user PASS user_id=${current_user_id}"

request_json GET "${GATEWAY_URL}/v1/models" "" "${user_token}"
log_case_json "auth_select_model" "{}" "${HTTP_BODY}"
if [[ "${HTTP_STATUS}" != "200" ]]; then
  echo "CASE auth_select_model FAIL status=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi
primary_model="$(python3 -c 'import json,sys; data=json.load(sys.stdin); arr=data.get("data",[]); print((arr[0].get("id") if arr else ""))' <<< "${HTTP_BODY}")"
if [[ -z "${primary_model}" ]]; then
  echo "CASE auth_select_model FAIL body=${HTTP_BODY}"
  exit 1
fi
echo "CASE auth_select_model PASS model=${primary_model}"

project_dir="$(scenario_report_path bob-project)"
mkdir -p "${project_dir}"
project_payload="$(printf '{"name":"Bob Project","directory":"%s"}' "${project_dir}")"
request_json POST "${GATEWAY_URL}/v1/projects" "${project_payload}" "${user_token}"
log_case_json "auth_create_project" "${project_payload}" "${HTTP_BODY}"
if [[ "${HTTP_STATUS}" != "200" ]]; then
  echo "CASE auth_create_project FAIL status=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi
project_id="$(json_get "${HTTP_BODY}" "project_id")"
project_owner_id="$(json_get "${HTTP_BODY}" "user_id")"
if [[ -z "${project_id}" || "${project_owner_id}" != "${user_user_id}" ]]; then
  echo "CASE auth_create_project FAIL body=${HTTP_BODY}"
  exit 1
fi
echo "CASE auth_create_project PASS project_id=${project_id}"

session_payload="$(cat <<JSON
{
  "context": {
    "source_type": "http",
    "provider": "http",
    "channel_id": "auth-session",
    "user_id": "spoofed-client-user",
    "thread_id": null,
    "message_id": "msg-auth-1",
    "trace_id": "trace-auth-1"
  },
  "model": "${primary_model}",
  "project_id": "${project_id}"
}
JSON
)"
request_json POST "${GATEWAY_URL}/v1/sessions" "${session_payload}" "${user_token}"
log_case_json "auth_create_session" "${session_payload}" "${HTTP_BODY}"
if [[ "${HTTP_STATUS}" != "200" ]]; then
  echo "CASE auth_create_session FAIL status=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi
session_id="$(session_id_from_json "${HTTP_BODY}")"
session_owner_id="$(json_get "${HTTP_BODY}" "user_id")"
session_context_user_id="$(json_get "${HTTP_BODY}" "context.user_id")"
if [[ -z "${session_id}" || "${session_owner_id}" != "${user_user_id}" || "${session_context_user_id}" != "${user_user_id}" ]]; then
  echo "CASE auth_create_session FAIL body=${HTTP_BODY}"
  exit 1
fi
echo "CASE auth_create_session PASS session_id=${session_id}"

request_json GET "${GATEWAY_URL}/v1/sessions" "" "${user_token}"
log_case_json "auth_user_list_sessions" "{}" "${HTTP_BODY}"
user_session_count="$(json_array_len "${HTTP_BODY}")"
contains_user_session="$(json_array_contains_field "${HTTP_BODY}" "context.session_id" "${session_id}")"
if [[ "${HTTP_STATUS}" != "200" || "${user_session_count}" -lt 1 || "${contains_user_session}" != "true" ]]; then
  echo "CASE auth_user_list_sessions FAIL status=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi
echo "CASE auth_user_list_sessions PASS count=${user_session_count}"

request_json GET "${GATEWAY_URL}/v1/projects" "" "${user_token}"
log_case_json "auth_user_list_projects" "{}" "${HTTP_BODY}"
user_project_count="$(json_array_len "${HTTP_BODY}")"
contains_user_project="$(json_array_contains_field "${HTTP_BODY}" "project_id" "${project_id}")"
if [[ "${HTTP_STATUS}" != "200" || "${user_project_count}" -lt 1 || "${contains_user_project}" != "true" ]]; then
  echo "CASE auth_user_list_projects FAIL status=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi
echo "CASE auth_user_list_projects PASS count=${user_project_count}"

request_json GET "${GATEWAY_URL}/v1/admin/sessions" "" "${user_token}"
log_case_json "auth_user_admin_sessions_forbidden" "{}" "${HTTP_BODY}"
if [[ "${HTTP_STATUS}" != "403" ]]; then
  echo "CASE auth_user_admin_sessions_forbidden FAIL status=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi
echo "CASE auth_user_admin_sessions_forbidden PASS status=${HTTP_STATUS}"

request_json GET "${GATEWAY_URL}/v1/admin/users" "" "${admin_token}"
log_case_json "auth_admin_list_users" "{}" "${HTTP_BODY}"
admin_user_count="$(json_array_len "${HTTP_BODY}")"
contains_bob_user="$(json_array_contains_field "${HTTP_BODY}" "user_id" "${user_user_id}")"
if [[ "${HTTP_STATUS}" != "200" || "${admin_user_count}" -lt 2 || "${contains_bob_user}" != "true" ]]; then
  echo "CASE auth_admin_list_users FAIL status=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi
echo "CASE auth_admin_list_users PASS count=${admin_user_count}"

request_json GET "${GATEWAY_URL}/v1/admin/sessions" "" "${admin_token}"
log_case_json "auth_admin_list_sessions" "{}" "${HTTP_BODY}"
admin_session_count="$(json_array_len "${HTTP_BODY}")"
admin_contains_user_session="$(json_array_contains_field "${HTTP_BODY}" "context.session_id" "${session_id}")"
if [[ "${HTTP_STATUS}" != "200" || "${admin_session_count}" -lt 1 || "${admin_contains_user_session}" != "true" ]]; then
  echo "CASE auth_admin_list_sessions FAIL status=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi
echo "CASE auth_admin_list_sessions PASS count=${admin_session_count}"

request_json GET "${GATEWAY_URL}/v1/admin/projects" "" "${admin_token}"
log_case_json "auth_admin_list_projects" "{}" "${HTTP_BODY}"
admin_project_count="$(json_array_len "${HTTP_BODY}")"
admin_contains_user_project="$(json_array_contains_field "${HTTP_BODY}" "project_id" "${project_id}")"
if [[ "${HTTP_STATUS}" != "200" || "${admin_project_count}" -lt 1 || "${admin_contains_user_project}" != "true" ]]; then
  echo "CASE auth_admin_list_projects FAIL status=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi
echo "CASE auth_admin_list_projects PASS count=${admin_project_count}"

request_json POST "${GATEWAY_URL}/v1/auth/logout" "" "${user_token}"
log_case_json "auth_logout_user" "{}" "${HTTP_BODY}"
logout_success="$(json_get "${HTTP_BODY}" "success")"
if [[ "${HTTP_STATUS}" != "200" || ( "${logout_success}" != "true" && "${logout_success}" != "True" ) ]]; then
  echo "CASE auth_logout_user FAIL status=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi
echo "CASE auth_logout_user PASS"

request_json GET "${GATEWAY_URL}/v1/user" "" "${user_token}"
log_case_json "auth_old_token_revoked" "{}" "${HTTP_BODY}"
if [[ "${HTTP_STATUS}" != "401" ]]; then
  echo "CASE auth_old_token_revoked FAIL status=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi
echo "CASE auth_old_token_revoked PASS status=${HTTP_STATUS}"

request_json POST "${GATEWAY_URL}/v1/auth/login" "${user_login_payload}"
log_case_json "auth_login_after_logout" "${user_login_payload}" "${HTTP_BODY}"
if [[ "${HTTP_STATUS}" != "200" ]]; then
  echo "CASE auth_login_after_logout FAIL status=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi
user_token_after_logout="$(json_get "${HTTP_BODY}" "access_token")"
if [[ -z "${user_token_after_logout}" ]]; then
  echo "CASE auth_login_after_logout FAIL body=${HTTP_BODY}"
  exit 1
fi

request_json GET "${GATEWAY_URL}/v1/user" "" "${user_token_after_logout}"
log_case_json "auth_new_token_works" "{}" "${HTTP_BODY}"
if [[ "${HTTP_STATUS}" != "200" || "$(json_get "${HTTP_BODY}" "user_id")" != "${user_user_id}" ]]; then
  echo "CASE auth_new_token_works FAIL status=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi
echo "CASE auth_new_token_works PASS user_id=${user_user_id}"
