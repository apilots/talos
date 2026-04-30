#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
build_binaries

timestamp="$(date '+%Y%m%d_%H%M%S')"
export TALOS_SCENARIO_REPORT_DIR="${ROOT_DIR}/tests/integration/reports/protocol_smoke_${timestamp}"
mkdir -p "${TALOS_SCENARIO_REPORT_DIR}"

"${ROOT_DIR}/tests/integration/scenarios/09_app_server/protocol_e2e.sh"
