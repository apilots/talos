#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export TALOS_TEST_PORT="${TALOS_AUTH_E2E_TALOS_PORT:-18081}"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

require_command cargo
require_command curl
require_command python3
build_binaries

CLOUD_REPO="${TALOS_CLOUD_REPO:-$(cd "${REPO_ROOT}/.." && pwd)/talos-cloud}"
CLOUD_RS_DIR="${CLOUD_REPO}/cloud-rs"
CLOUD_BIND_HOST="${TALOS_AUTH_E2E_CLOUD_HOST:-127.0.0.1}"
CLOUD_BIND_PORT="${TALOS_AUTH_E2E_CLOUD_PORT:-18881}"
CLOUD_BIND_ADDR="${CLOUD_BIND_HOST}:${CLOUD_BIND_PORT}"
CLOUD_BASE_URL="http://${CLOUD_BIND_ADDR}"
TMP_DIR="$(make_e2e_temp_dir "talos-auth-cloud-gateway")"
CLOUD_DB_PATH="${TMP_DIR}/cloud.db"
CLOUD_CONFIG_PATH="${TMP_DIR}/cloud.yaml"
CLOUD_SERVER_LOG="${TMP_DIR}/cloud-server.log"
CLOUD_SERVER_PID=""
LAST_BODY=""
LAST_STATUS=""

cleanup() {
  stop_server
  if [[ -n "${CLOUD_SERVER_PID}" ]] && kill -0 "${CLOUD_SERVER_PID}" 2>/dev/null; then
    kill "${CLOUD_SERVER_PID}" 2>/dev/null || true
    wait "${CLOUD_SERVER_PID}" 2>/dev/null || true
  fi
  if [[ "${TALOS_AUTH_E2E_KEEP_TMP:-0}" != "1" ]]; then
    rm -rf "${TMP_DIR}"
  else
    echo "kept temp directory: ${TMP_DIR}"
  fi
}
trap cleanup EXIT

fail_case() {
  echo "CASE cloud_gateway_auth FAIL $*" >&2
  echo "DETAIL cloud_server_log=${CLOUD_SERVER_LOG}" >&2
  if [[ -f "${CLOUD_SERVER_LOG}" ]]; then
    tail -n 80 "${CLOUD_SERVER_LOG}" >&2 || true
  fi
  exit 1
}

request_json() {
  local base_url="$1"
  local method="$2"
  local path="$3"
  local expected_status="$4"
  local data="${5:-}"
  local token="${6:-}"
  local response
  local curl_args=(
    -sS
    -w $'\n%{http_code}'
    -X "${method}"
    -H "accept: application/json"
  )

  if [[ -n "${data}" ]]; then
    curl_args+=(-H "content-type: application/json" --data "${data}")
  fi
  if [[ -n "${token}" ]]; then
    curl_args+=(-H "authorization: Bearer ${token}")
  fi

  response="$(curl "${curl_args[@]}" "${base_url}${path}")" \
    || fail_case "request failed: ${method} ${base_url}${path}"
  LAST_STATUS="${response##*$'\n'}"
  LAST_BODY="${response%$'\n'*}"
  if [[ "${LAST_STATUS}" != "${expected_status}" ]]; then
    fail_case "unexpected status for ${method} ${path}: expected=${expected_status} actual=${LAST_STATUS} body=${LAST_BODY}"
  fi
}

wait_for_cloud() {
  for _ in $(seq 1 120); do
    if curl -fsS "${CLOUD_BASE_URL}/v1/auth/config" >/dev/null 2>&1; then
      return 0
    fi
    if ! kill -0 "${CLOUD_SERVER_PID}" 2>/dev/null; then
      fail_case "cloud-server exited before ready"
    fi
    sleep 0.25
  done
  fail_case "cloud-server did not become ready at ${CLOUD_BASE_URL}"
}

start_cloud_server() {
  [[ -d "${CLOUD_RS_DIR}" ]] || fail_case "missing talos-cloud repo: ${CLOUD_RS_DIR}"
  (cd "${CLOUD_RS_DIR}" && cargo build -q -p cloud-server)
cat >"${CLOUD_CONFIG_PATH}" <<EOF
server:
  bind_addr: "${CLOUD_BIND_ADDR}"
  api_base_url: "${CLOUD_BASE_URL}"
  cloud_base_url: "${CLOUD_BASE_URL}"

database:
  type: sqlite
  url: "sqlite://${CLOUD_DB_PATH}?mode=rwc"
EOF
  TALOS_CLOUD_CONFIG_PATH="${CLOUD_CONFIG_PATH}" \
    "${CLOUD_RS_DIR}/target/debug/cloud-server" >"${CLOUD_SERVER_LOG}" 2>&1 &
  CLOUD_SERVER_PID="$!"
  wait_for_cloud
}

stop_cloud_server() {
  if [[ -n "${CLOUD_SERVER_PID}" ]] && kill -0 "${CLOUD_SERVER_PID}" 2>/dev/null; then
    kill "${CLOUD_SERVER_PID}" 2>/dev/null || true
    wait "${CLOUD_SERVER_PID}" 2>/dev/null || true
  fi
  CLOUD_SERVER_PID=""
}

write_cloud_session_cache() {
  local token="$1"
  local user_id="$2"
  local username="$3"
  local email="$4"
  local role="$5"
  local user_type="$6"
  mkdir -p "${TALOS_HOME}/auth"
  CLOUD_TOKEN="${token}" \
  CLOUD_BASE_URL="${CLOUD_BASE_URL}" \
  CLOUD_USER_ID="${user_id}" \
  CLOUD_USERNAME="${username}" \
  CLOUD_EMAIL="${email}" \
  CLOUD_ROLE="${role}" \
  CLOUD_USER_TYPE="${user_type}" \
  CACHE_PATH="${TALOS_HOME}/auth/cloud-session.json" \
  python3 - <<'PY'
import json
import os
import time
from pathlib import Path

now_ms = int(time.time() * 1000)
entry = {
    "schema_version": 2,
    "cloud_api_base_url": os.environ["CLOUD_BASE_URL"],
    "access_token": os.environ["CLOUD_TOKEN"],
    "token_type": "Bearer",
    "expires_at_ms": now_ms + 15 * 24 * 60 * 60 * 1000,
    "last_used_at_ms": now_ms - 25 * 60 * 60 * 1000,
    "user": {
        "user_id": os.environ["CLOUD_USER_ID"],
        "username": os.environ["CLOUD_USERNAME"],
        "email": os.environ["CLOUD_EMAIL"],
        "role": os.environ["CLOUD_ROLE"],
        "type": os.environ["CLOUD_USER_TYPE"],
    },
}
path = Path(os.environ["CACHE_PATH"])
path.write_text(json.dumps(entry, indent=2), encoding="utf-8")
path.chmod(0o600)
PY
}

timestamp="$(date '+%s')"
ADMIN_EMAIL="talos-auth-e2e-${timestamp}@example.com"
ADMIN_USERNAME="talos-auth-e2e-${timestamp}"
ADMIN_PASSWORD="AdminSecret123!"

export TALOS_HOME="${TMP_DIR}/talos-home"
export TALOS_TEST_GATEWAY_AUTH="true"
export TALOS_TEST_CLOUD_API_BASE_URL="${CLOUD_BASE_URL}"
export TALOS_TEST_CLOUD_WEB_BASE_URL="${CLOUD_BASE_URL}"

start_cloud_server

request_json "${CLOUD_BASE_URL}" POST "/v1/auth/register" 200 \
  "{\"email\":\"${ADMIN_EMAIL}\",\"username\":\"${ADMIN_USERNAME}\",\"password\":\"${ADMIN_PASSWORD}\"}"
request_json "${CLOUD_BASE_URL}" POST "/v1/auth/login" 200 \
  "{\"identifier\":\"${ADMIN_EMAIL}\",\"password\":\"${ADMIN_PASSWORD}\"}"
CLOUD_TOKEN="$(json_get "${LAST_BODY}" "access_token")"
CLOUD_USER_ID="$(json_get "${LAST_BODY}" "user.user_id")"
CLOUD_USERNAME="$(json_get "${LAST_BODY}" "user.username")"
CLOUD_ROLE="$(json_get "${LAST_BODY}" "user.role")"
CLOUD_USER_TYPE="$(json_get "${LAST_BODY}" "user.type")"

request_json "${CLOUD_BASE_URL}" POST "/v1/auth/introspect" 200 "{\"token\":\"${CLOUD_TOKEN}\"}"
cloud_active="$(json_get "${LAST_BODY}" "active")"
[[ "${cloud_active}" == "true" || "${cloud_active}" == "True" ]] \
  || fail_case "cloud token is not active body=${LAST_BODY}"
echo "CASE cloud_token_introspection PASS"

start_server

health_json="$(curl -sS "${HEALTH_URL}")"
health_cloud_auth_url="$(json_get "${health_json}" "cloud_auth_url")"
if [[ "${health_cloud_auth_url}" != "${CLOUD_BASE_URL}" ]]; then
  fail_case "health cloud_auth_url mismatch expected=${CLOUD_BASE_URL} actual=${health_cloud_auth_url}"
fi
echo "CASE talos_health_cloud_auth_url PASS"

request_json "${GATEWAY_URL}" GET "/v1/user" 401
echo "CASE talos_user_requires_bearer_token PASS"

request_json "${GATEWAY_URL}" GET "/v1/user" 401 "" "invalid-token"
echo "CASE talos_user_rejects_invalid_cloud_token PASS"

request_json "${GATEWAY_URL}" GET "/v1/user" 200 "" "${CLOUD_TOKEN}"
talos_user_id="$(api_json_get "${LAST_BODY}" "user.user_id")"
talos_user_role="$(api_json_get "${LAST_BODY}" "user.role")"
talos_user_type="$(api_json_get "${LAST_BODY}" "user.type")"
if [[ "${talos_user_id}" != "${CLOUD_USER_ID}" || "${talos_user_role}" != "${CLOUD_ROLE}" || "${talos_user_type}" != "${CLOUD_USER_TYPE}" ]]; then
  fail_case "talos /v1/user mismatch body=${LAST_BODY}"
fi
log_case_json "talos_cloud_user" "{\"path\":\"/v1/user\"}" "${LAST_BODY}"
echo "CASE talos_user_accepts_real_cloud_token PASS user_id=${talos_user_id} type=${talos_user_type}"

stop_server

write_cloud_session_cache \
  "${CLOUD_TOKEN}" \
  "${CLOUD_USER_ID}" \
  "${CLOUD_USERNAME}" \
  "${ADMIN_EMAIL}" \
  "${CLOUD_ROLE}" \
  "${CLOUD_USER_TYPE}"

build_app_server_binaries
export TALOS_APP_SERVER_BIN="${TALOS_RS_DIR}/target/debug/app-server"

if timeout 90 "${TALOS_RS_DIR}/target/debug/talos-app-server-test-client" cloud-session-cache >"$(scenario_report_path "app_server_cloud_session_cache.log")" 2>&1; then
  echo "CASE app_server_existing_session_token PASS"
else
  cat "$(scenario_report_path "app_server_cloud_session_cache.log")"
  fail_case "app-server failed to use verified cloud session cache"
fi

if timeout 180 "${TALOS_RS_DIR}/target/debug/talos" exec --session "auth-cache-${timestamp}" "Reply exactly: ok" >"$(scenario_report_path "cli_cloud_session_cache.log")" 2>&1; then
  echo "CASE cli_existing_session_token PASS"
else
  cat "$(scenario_report_path "cli_cloud_session_cache.log")"
  fail_case "CLI failed to use verified cloud session cache"
fi

stop_cloud_server

if timeout 45 "${TALOS_RS_DIR}/target/debug/talos" exec --session "auth-cache-offline-${timestamp}" "Reply exactly: ok" >"$(scenario_report_path "cli_cloud_session_cache_offline.log")" 2>&1; then
  cat "$(scenario_report_path "cli_cloud_session_cache_offline.log")"
  fail_case "CLI authorized from cached cloud session while Talos Cloud was offline"
fi
echo "CASE cli_cached_session_requires_cloud_validation PASS"

echo "CASE cloud_gateway_auth PASS"
