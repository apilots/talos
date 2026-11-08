#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
export TALOS_SCENARIO_REPORT_DIR="${SCENARIO_REPORT_DIR}"
: "${TALOS_E2E_K3D_CLUSTER:?Set TALOS_E2E_K3D_CLUSTER to the disposable test cluster name}"
python3 "${TALOS_E2E_DIR}/scripts/browser-worker/managed_talo_kubernetes.py"
