#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
build_server_binary
pnpm --dir "${TALOS_APP_DIR}" --filter talos-web build
export TALOS_SCENARIO_REPORT_DIR="${SCENARIO_REPORT_DIR}"
python3 "${SCRIPT_DIR}/../../scripts/automation_web.py"
