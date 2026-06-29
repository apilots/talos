#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

load_env
build_binaries
build_app_server_binaries
prepare_talos_home

export TALOS_APP_SERVER_BIN="${TALOS_RS_DIR}/target/debug/app-server"
export TALOS_APP_SERVER_TEST_MODEL="${TALOS_APP_SERVER_TEST_MODEL:-qwen3.6-flash}"

log_file="$(scenario_report_path "app_server_skills_e2e.log")"
if timeout "${TALOS_APP_SERVER_SCENARIO_TIMEOUT_SECS:-180}" \
  "${TALOS_RS_DIR}/target/debug/talos-app-server-test-client" skills-e2e >"${log_file}" 2>&1; then
  echo "CASE app_server_skills_e2e PASS"
else
  echo "CASE app_server_skills_e2e FAIL log=${log_file}"
  cat "${log_file}"
  exit 1
fi
