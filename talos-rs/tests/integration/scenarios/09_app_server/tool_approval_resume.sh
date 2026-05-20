#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command python3
build_binaries
prepare_talos_home
mkdir -p "${SCENARIO_REPORT_DIR}"

export TALOS_LOCAL_LAUNCH_TOKEN="${TALOS_LOCAL_LAUNCH_TOKEN:-talos-app-server-test-token}"
export TALOS_APP_SERVER_BIN="${ROOT_DIR}/target/debug/app-server"
export TALOS_APP_SERVER_TEST_CLIENT_BIN="${ROOT_DIR}/target/debug/talos-app-server-test-client"
export TALOS_APP_SERVER_TEST_MODEL="${TALOS_APP_SERVER_TEST_MODEL:-MiniMax-M2.5}"

log_file="$(scenario_report_path "app_server_tool_approval_resume.log")"
if timeout "${TALOS_APP_SERVER_SCENARIO_TIMEOUT_SECS:-180}" \
  "${ROOT_DIR}/target/debug/talos-app-server-test-client" approval-resume \
  >"${log_file}" 2>&1; then
  echo "CASE app_server_tool_approval_resume PASS"
else
  echo "CASE app_server_tool_approval_resume FAIL log=${log_file}"
  cat "${log_file}"
  exit 1
fi
