#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
export TALOS_SCENARIO_REPORT_DIR="${SCENARIO_REPORT_DIR}"
for command in cargo node pnpm python3; do require_command "${command}"; done
python3 -c 'import yaml'
node "${REPO_ROOT}/build/plugins/snapshot.mjs" "${REPO_ROOT}/plugins/apilots-mcps"
prepare_binary TALOS_E2E_APP_SERVER_BIN app-server app-server
[[ -x "${TALOS_E2E_APP_SERVER_BIN}" ]] || { echo "Current app-server binary is required" >&2; exit 1; }
pnpm --dir "${TALOS_APP_DIR}/desktop" build:main
node "${TALOS_E2E_DIR}/scripts/browser-worker/build-native.mjs" mcp
python3 "${TALOS_E2E_DIR}/scripts/browser-worker/native_mcp.py"
