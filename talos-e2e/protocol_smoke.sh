#!/usr/bin/env bash
set -euo pipefail

TALOS_E2E_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${TALOS_E2E_DIR}/scripts/common.sh"

load_env
build_binaries

timestamp="$(date '+%Y%m%d_%H%M%S')"
export TALOS_SCENARIO_REPORT_DIR="${REPORT_ROOT}/protocol_smoke_${timestamp}"
mkdir -p "${TALOS_SCENARIO_REPORT_DIR}"

"${TALOS_E2E_DIR}/scenarios/09_app_server/protocol_e2e.sh"
