#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
build_binaries
prepare_talos_home

export TALOS_LOCAL_LAUNCH_TOKEN="${TALOS_LOCAL_LAUNCH_TOKEN:-talos-app-server-test-token}"
export TALOS_APP_SERVER_BIN="${ROOT_DIR}/target/debug/talos"
export TALOS_APP_SERVER_TEST_CLIENT_BIN="${ROOT_DIR}/target/debug/talos-app-server-test-client"
export TALOS_APP_SERVER_TEST_MODEL="${TALOS_APP_SERVER_TEST_MODEL:-MiniMax-M2.5}"

run_client_scenario() {
  local scenario="$1"
  local log_file
  log_file="$(scenario_report_path "app_server_${scenario}.log")"
  if timeout "${TALOS_APP_SERVER_SCENARIO_TIMEOUT_SECS:-180}" \
    "${ROOT_DIR}/target/debug/talos-app-server-test-client" "${scenario}" >"${log_file}" 2>&1; then
    echo "CASE app_server_${scenario} PASS"
  else
    echo "CASE app_server_${scenario} FAIL log=${log_file}"
    cat "${log_file}"
    exit 1
  fi
}

run_client_scenario "smoke"
run_client_scenario "malformed"
run_client_scenario "replay"
run_client_scenario "multi-subscriber"
run_client_scenario "turn-smoke"
run_client_scenario "active-turn-conflict"
run_client_scenario "resource-matrix"

cli_log_file="$(scenario_report_path "app_server_cli_smoke.log")"
if timeout "${TALOS_APP_SERVER_SCENARIO_TIMEOUT_SECS:-180}" \
  "${ROOT_DIR}/target/debug/talos-client" app-server-smoke --scenario turn-smoke >"${cli_log_file}" 2>&1; then
  echo "CASE app_server_cli_smoke PASS"
else
  echo "CASE app_server_cli_smoke FAIL log=${cli_log_file}"
  cat "${cli_log_file}"
  exit 1
fi
