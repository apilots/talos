#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
export TALOS_SCENARIO_REPORT_DIR="${SCENARIO_REPORT_DIR}"
export TALOS_E2E_SERVER_IMAGE="${TALOS_E2E_SERVER_IMAGE:-talos-server:browser-e2e}"
prepare_binary TALOS_E2E_APP_SERVER_BIN app-server app-server
pnpm --dir "${TALOS_APP_DIR}/desktop" exec vite build --config "${TALOS_E2E_DIR}/scripts/computer_workspace/vite.config.ts"
python3 "${TALOS_E2E_DIR}/scripts/browser-worker/owner_controls.py"
