#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

build_app_server_binaries
prepare_talos_home

export TALOS_APP_SERVER_BIN="${TALOS_RS_DIR}/target/debug/app-server"
export TALOS_APP_SERVER_TEST_MODEL="${TALOS_APP_SERVER_TEST_MODEL:-qwen3.6-flash}"
export TALOS_PLAYWRIGHT_MCP_BROWSER="${TALOS_PLAYWRIGHT_MCP_BROWSER:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"

playwright_output_dir="$(scenario_report_path "playwright-mcp")"
mkdir -p "${playwright_output_dir}"
export TALOS_PLAYWRIGHT_MCP_E2E_OUTPUT="${playwright_output_dir}"

log_file="$(scenario_report_path "app_server_mcp_e2e.log")"
if timeout "${TALOS_APP_SERVER_MCP_SCENARIO_TIMEOUT_SECS:-300}" \
  "${TALOS_RS_DIR}/target/debug/talos-app-server-test-client" mcp-e2e >"${log_file}" 2>&1; then
  echo "CASE app_server_mcp_e2e PASS"
  echo "CASE app_server_mcp_e2e ARTIFACTS ${playwright_output_dir}"
else
  echo "CASE app_server_mcp_e2e FAIL log=${log_file}"
  cat "${log_file}"
  exit 1
fi
