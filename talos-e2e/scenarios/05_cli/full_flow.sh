#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

load_env
require_command "${TALOS_RS_DIR}/target/debug/talos-cli"

build_binaries
prepare_talos_home

export TALOS_APP_SERVER_BIN="${TALOS_RS_DIR}/target/debug/app-server"

cli="${TALOS_RS_DIR}/target/debug/talos-cli"

run_cli() {
  "${cli}" "$@" 2>&1 | sed '/^Command executed successfully$/d'
}

models_json="$(run_cli models --format json)"
log_case_json "cli_select_models" "{}" "${models_json}"
primary_model="$(select_e2e_model_from_json "${models_json}")"
secondary_model="$(python3 -c 'import json,sys; arr=json.load(sys.stdin); print((arr[1].get("id") if len(arr) > 1 else (arr[0].get("id") if arr else "")))' <<< "${models_json}")"
if [[ -z "${primary_model}" || -z "${secondary_model}" ]]; then
  echo "CASE cli_select_models FAIL models=${models_json}"
  exit 1
fi
echo "CASE cli_select_models PASS primary=${primary_model} secondary=${secondary_model}"

models_output="$(run_cli models)"
log_case_json "cli_models" "{\"command\":\"models\"}" "${models_output}"
if ! echo "${models_output}" | grep -q "Available models"; then
  echo "CASE cli_models FAIL output=${models_output}"
  exit 1
fi
echo "CASE cli_models PASS"

session_id="$(new_session_id)"
echo "CASE cli_session_id PASS session_id=${session_id}"

in_output="$(run_cli chat in "${session_id}" "hello from cli scenario" --model "${primary_model}")"
log_case_json "cli_chat_in_session" "{\"command\":\"chat in\",\"session_id\":\"${session_id}\",\"input\":\"hello from cli scenario\"}" "${in_output}"
if [[ -z "${in_output}" || "${in_output}" == Error:* ]]; then
  echo "CASE cli_chat_in_session FAIL output=${in_output}"
  exit 1
fi
echo "CASE cli_chat_in_session PASS"
echo "DETAIL cli_chat_in_session_output=${in_output}"

set_model_output="$(run_cli session set-model "${session_id}" "${secondary_model}")"
log_case_json "cli_set_model" "{\"command\":\"session set-model\",\"session_id\":\"${session_id}\",\"model\":\"${secondary_model}\"}" "${set_model_output}"
if ! echo "${set_model_output}" | grep -q "Session model updated"; then
  echo "CASE cli_set_model FAIL output=${set_model_output}"
  exit 1
fi
echo "CASE cli_set_model PASS"

list_output="$(run_cli session list --limit 5)"
log_case_json "cli_session_list" "{\"command\":\"session list\",\"limit\":5}" "${list_output}"
if ! echo "${list_output}" | grep -q "${session_id}"; then
  echo "CASE cli_session_list FAIL output=${list_output}"
  exit 1
fi
echo "CASE cli_session_list PASS"
