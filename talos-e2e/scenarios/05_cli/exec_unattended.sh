#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

case_dir="$(make_e2e_temp_dir exec-unattended)"
python3 "${TALOS_E2E_DIR}/scripts/exec_unattended.py" \
  "${REPO_ROOT}" "${case_dir}" "${SCENARIO_REPORT_DIR}"
