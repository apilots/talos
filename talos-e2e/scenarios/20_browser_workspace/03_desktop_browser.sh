#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
export TALOS_SCENARIO_REPORT_DIR="${SCENARIO_REPORT_DIR}"
for command in node pnpm python3; do require_command "${command}"; done
pnpm --dir "${TALOS_APP_DIR}/desktop" build:main
node "${TALOS_E2E_DIR}/scripts/browser-worker/build-native.mjs" engine
python3 "${TALOS_E2E_DIR}/scripts/browser-worker/native_engine.py"
