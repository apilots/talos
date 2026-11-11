#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
build_cli_binary
prepare_talos_home
node "${REPO_ROOT}/build/plugins/snapshot.mjs" "${REPO_ROOT}/plugins/apilots-mcps"
export TALOS_SCENARIO_REPORT_DIR="${SCENARIO_REPORT_DIR}"
python3 "${TALOS_E2E_DIR}/scripts/plugin_cli.py"
echo "CASE plugin_cli_lifecycle PASS"
