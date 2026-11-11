#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

build_app_server_binaries
prepare_talos_home

export TALOS_APP_SERVER_BIN="${TALOS_E2E_APP_SERVER_BIN}"
TALOS_E2E_PLUGIN_CATALOG="$(python3 "${TALOS_E2E_DIR}/scripts/plugin_fixture.py")"
export TALOS_E2E_PLUGIN_CATALOG

log_file="$(scenario_report_path "plugin-api-acceptance.log")"
if timeout "${TALOS_APP_SERVER_SCENARIO_TIMEOUT_SECS:-180}" \
  "${TALOS_E2E_APP_SERVER_TEST_CLIENT_BIN}" plugin-lifecycle >"${log_file}" 2>&1; then
  echo "CASE plugin_api_lifecycle PASS"
else
  cat "${log_file}"
  exit 1
fi
