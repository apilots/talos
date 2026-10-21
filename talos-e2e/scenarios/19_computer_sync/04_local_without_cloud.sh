#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
for command in cargo protoc python3; do require_command "${command}"; done
build_server_binary
export TALOS_SCENARIO_REPORT_DIR="${SCENARIO_REPORT_DIR}"
python3 "${SCRIPT_DIR}/../../scripts/computer_sync/local.py"
