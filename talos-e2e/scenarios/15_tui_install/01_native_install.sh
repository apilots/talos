#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/tui_install.sh"

SCENARIO_ROOT="$(make_e2e_temp_dir "tui-install-native")"

cleanup() {
  rm -rf "${SCENARIO_ROOT}"
}
trap cleanup EXIT

run_installed_tui_acceptance "${SCENARIO_ROOT}"
