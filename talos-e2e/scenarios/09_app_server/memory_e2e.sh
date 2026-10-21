#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

build_app_server_binaries
export TALOS_APP_SERVER_BIN="${TALOS_E2E_APP_SERVER_BIN:-${TALOS_RS_DIR}/target/debug/app-server}"

prepare_talos_home
export TALOS_CONFIG="${TALOS_HOME}/conf/talos.yaml"

log_file="$(scenario_report_path "app_server_memory_e2e.log")"
if timeout "${TALOS_APP_SERVER_SCENARIO_TIMEOUT_SECS:-300}" \
  "${TALOS_E2E_APP_SERVER_TEST_CLIENT_BIN:-${TALOS_RS_DIR}/target/debug/talos-app-server-test-client}" memory-e2e >"${log_file}" 2>&1; then
  echo "CASE app_server_memory_e2e PASS"
else
  echo "CASE app_server_memory_e2e FAIL log=${log_file}"
  cat "${log_file}"
  exit 1
fi
