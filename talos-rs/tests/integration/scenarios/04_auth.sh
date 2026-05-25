#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command curl

export TALOS_HOME="$(scenario_report_path talos-home)"
rm -rf "${TALOS_HOME}"

request_json() {
  local method="$1"
  local url="$2"
  local body="${3:-}"
  local response_file
  response_file="$(mktemp)"

  local -a args=(-sS -X "${method}" -o "${response_file}" -w "%{http_code}")
  if [[ -n "${body}" ]]; then
    args+=(-H "Content-Type: application/json" -d "${body}")
  fi
  args+=("${url}")

  HTTP_STATUS="$(curl "${args[@]}")"
  HTTP_BODY="$(cat "${response_file}")"
  rm -f "${response_file}"
}

assert_local_auth_removed() {
  local case_name="$1"
  local method="$2"
  local path="$3"
  local body="${4:-}"

  request_json "${method}" "${GATEWAY_URL}${path}" "${body}"
  log_case_json "${case_name}" "${body:-{}}" "${HTTP_BODY}"
  if [[ "${HTTP_STATUS}" != "410" ]]; then
    echo "CASE ${case_name} FAIL expected=410 actual=${HTTP_STATUS} body=${HTTP_BODY}"
    exit 1
  fi

  code="$(json_get "${HTTP_BODY}" "code")"
  if [[ "${code}" != "local_auth_removed" ]]; then
    echo "CASE ${case_name} FAIL expected_code=local_auth_removed body=${HTTP_BODY}"
    exit 1
  fi
  echo "CASE ${case_name} PASS status=${HTTP_STATUS}"
}

start_server
trap stop_server EXIT

assert_local_auth_removed \
  "local_auth_register_removed" \
  POST \
  "/v1/auth/register" \
  '{"username":"alice","password":"password123","email":"alice@example.com"}'

assert_local_auth_removed \
  "local_auth_login_removed" \
  POST \
  "/v1/auth/login" \
  '{"username":"alice","password":"password123"}'

request_json GET "${GATEWAY_URL}/v1/models"
log_case_json "auth_disabled_anonymous_models" "{}" "${HTTP_BODY}"
if [[ "${HTTP_STATUS}" != "200" ]]; then
  echo "CASE auth_disabled_anonymous_models FAIL expected=200 actual=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi
echo "CASE auth_disabled_anonymous_models PASS"
