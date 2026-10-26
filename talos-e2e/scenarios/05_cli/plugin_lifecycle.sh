#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
build_server_binary
build_cli_binary
node "${REPO_ROOT}/build/plugins/snapshot.mjs" "${REPO_ROOT}/plugins/apilots-mcps"
start_server
trap stop_server EXIT
export TALOS_RESOURCE_DIR="${REPO_ROOT}"
export TALOS_SCENARIO_REPORT_DIR="${SCENARIO_REPORT_DIR}"
python3 "${TALOS_E2E_DIR}/scripts/plugin_cli.py" prepare "${GATEWAY_URL}"
stop_server
python3 "${TALOS_E2E_DIR}/scripts/plugin_cli.py" run
echo "CASE plugin_cli_lifecycle PASS"
