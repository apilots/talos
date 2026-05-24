#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
build_binaries
prepare_talos_home

export TALOS_APP_SERVER_BIN="${ROOT_DIR}/target/debug/app-server"
export TALOS_LOCAL_LAUNCH_TOKEN="${TALOS_LOCAL_LAUNCH_TOKEN:-talos-cli-app-server-test-token}"

cli="${ROOT_DIR}/target/debug/talos-cli"

run_cli() {
  "${cli}" "$@" 2>&1 | sed '/^Command executed successfully$/d'
}

capture_cli() {
  local __out_var="$1"
  shift
  local output
  set +e
  output="$(run_cli "$@")"
  local status=$?
  set -e
  printf -v "${__out_var}" '%s' "${output}"
  return "${status}"
}

models_output="$(run_cli models)"
log_case_json "cli_app_server_models" "{\"command\":\"models\"}" "${models_output}"
if ! echo "${models_output}" | grep -q "Available models"; then
  echo "CASE cli_app_server_models FAIL output=${models_output}"
  exit 1
fi
echo "CASE cli_app_server_models PASS"

status_output="$(run_cli status)"
log_case_json "cli_app_server_status" "{\"command\":\"status\"}" "${status_output}"
if ! echo "${status_output}" | grep -q "App-server accessible"; then
  echo "CASE cli_app_server_status FAIL output=${status_output}"
  exit 1
fi
echo "CASE cli_app_server_status PASS"

create_output="$(run_cli session create)"
log_case_json "cli_app_server_session_create" "{\"command\":\"session create\"}" "${create_output}"
session_id="$(echo "${create_output}" | sed -n 's/^Session created: \([^ ]*\).*/\1/p')"
if [[ -z "${session_id}" ]]; then
  echo "CASE cli_app_server_session_create FAIL output=${create_output}"
  exit 1
fi
echo "CASE cli_app_server_session_create PASS session_id=${session_id}"

info_output="$(run_cli session info "${session_id}")"
log_case_json "cli_app_server_session_info" "{\"command\":\"session info\",\"session_id\":\"${session_id}\"}" "${info_output}"
if ! echo "${info_output}" | grep -q "${session_id}"; then
  echo "CASE cli_app_server_session_info FAIL output=${info_output}"
  exit 1
fi
echo "CASE cli_app_server_session_info PASS"

if ! capture_cli chat_output chat in "${session_id}" "Answer exactly: cli-app-server-ok"; then
  log_case_json "cli_app_server_chat_in" "{\"command\":\"chat in\",\"session_id\":\"${session_id}\"}" "${chat_output}"
  echo "CASE cli_app_server_chat_in FAIL output=${chat_output}"
  exit 1
fi
log_case_json "cli_app_server_chat_in" "{\"command\":\"chat in\",\"session_id\":\"${session_id}\"}" "${chat_output}"
if [[ -z "${chat_output}" || "${chat_output}" == Error:* ]]; then
  echo "CASE cli_app_server_chat_in FAIL output=${chat_output}"
  exit 1
fi
echo "CASE cli_app_server_chat_in PASS"
echo "DETAIL cli_app_server_chat_output=${chat_output}"

list_output="$(run_cli session list --limit 5)"
log_case_json "cli_app_server_session_list" "{\"command\":\"session list\",\"limit\":5}" "${list_output}"
if ! echo "${list_output}" | grep -q "${session_id}"; then
  echo "CASE cli_app_server_session_list FAIL output=${list_output}"
  exit 1
fi
echo "CASE cli_app_server_session_list PASS"

smoke_output="$(run_cli app-server-smoke --scenario smoke)"
log_case_json "cli_app_server_smoke" "{\"command\":\"app-server-smoke\",\"scenario\":\"smoke\"}" "${smoke_output}"
if ! echo "${smoke_output}" | grep -q "App-server scenario passed"; then
  echo "CASE cli_app_server_smoke FAIL output=${smoke_output}"
  exit 1
fi
echo "CASE cli_app_server_smoke PASS"
