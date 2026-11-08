#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
export TALOS_SCENARIO_REPORT_DIR="${SCENARIO_REPORT_DIR}"
: "${TALOS_E2E_K3D_CLUSTER:?Set TALOS_E2E_K3D_CLUSTER to the disposable test cluster name}"
pnpm --dir "${TALOS_APP_DIR}/desktop" build:main
export TALOS_E2E_TALO_RECOVERY=1
export TALOS_E2E_TALO_DESKTOP_PROBE="${TALOS_E2E_DIR}/scripts/managed-talo/native_desktop.py"
"${TALOS_E2E_TALO_DESKTOP_PROBE}" --check
python3 "${TALOS_E2E_DIR}/scripts/browser-worker/managed_talo_kubernetes.py"
