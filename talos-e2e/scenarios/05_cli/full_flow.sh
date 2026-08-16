#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

require_command "${TALOS_RS_DIR}/target/debug/talos"

build_binaries
prepare_talos_home

cli="${TALOS_RS_DIR}/target/debug/talos"

run_cli() {
  "${cli}" "$@" 2>&1
}

models_json="$(run_cli models --format json)"
log_case_json "cli_select_models" "{}" "${models_json}"
primary_model="$(select_e2e_model_from_cli_json "${models_json}")"
primary_provider="$(select_e2e_model_provider_from_cli_json "${models_json}" "${primary_model}")"
secondary_model="$(python3 -c 'import json,sys; payload=json.load(sys.stdin); arr=payload.get("models", []) if isinstance(payload, dict) else payload; print((arr[1].get("id") if len(arr) > 1 else (arr[0].get("id") if arr else "")))' <<< "${models_json}")"
secondary_provider="$(select_e2e_model_provider_from_cli_json "${models_json}" "${secondary_model}")"
if [[ -z "${primary_model}" || -z "${primary_provider}" || -z "${secondary_model}" || -z "${secondary_provider}" ]]; then
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

in_output="$(run_cli exec --session "${session_id}" --model "${primary_provider}:${primary_model}" "hello from cli scenario")"
log_case_json "cli_exec_in_session" "{\"command\":\"exec\",\"session_id\":\"${session_id}\",\"input\":\"hello from cli scenario\"}" "${in_output}"
if [[ -z "${in_output}" || "${in_output}" == Error:* ]]; then
  echo "CASE cli_exec_in_session FAIL output=${in_output}"
  exit 1
fi
echo "CASE cli_exec_in_session PASS"
echo "DETAIL cli_exec_in_session_output=${in_output}"

set_model_output="$(run_cli session set-model "${session_id}" "${secondary_provider}:${secondary_model}")"
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
