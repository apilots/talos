#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/../../scripts/common.sh"
export TALOS_E2E_CADDY_BIN="${TALOS_E2E_CADDY_BIN:-caddy}"
export TALOS_E2E_CHROME_BIN="${TALOS_E2E_CHROME_BIN:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
for command in node pnpm protoc openssl python3 "${TALOS_E2E_CADDY_BIN}" "${TALOS_E2E_CHROME_BIN}"; do require_command "${command}"; done
has_real_llm_key || { echo 'CASE ui_preflight FAIL real model key required'; exit 1; }
build_server_binary
build_worker_binary
(cd "${TALOS_RS_DIR}" && cargo build -q -p app-server)
(cd "${TALOS_CLOUD_REPO:-${REPO_ROOT}/../talos-cloud}/cloud-rs" && cargo build -q -p cloud-server)
if [[ -z "${TALOS_E2E_DESKTOP_APP:-}" ]]; then pnpm --dir "${TALOS_APP_DIR}" --filter talos-desktop build; fi
export TALOS_SCENARIO_REPORT_DIR="${SCENARIO_REPORT_DIR}"
python3 "${SCRIPT_DIR}/../../scripts/computer_workspace/ui.py"
