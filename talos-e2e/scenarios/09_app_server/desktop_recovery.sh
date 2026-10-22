#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
build_server_binary
prepare_binary TALOS_E2E_APP_SERVER_BIN app-server app-server
recovery_home="$(make_e2e_temp_dir desktop-recovery)"
cleanup_recovery() {
  local log_file
  : > "${SCENARIO_REPORT_DIR}/app-server.log"
  for log_file in "${recovery_home}/logs/talos.log" "${recovery_home}"/logs/talos.log.daily.*; do
    if [[ -f "${log_file}" ]]; then
      cat "${log_file}" >> "${SCENARIO_REPORT_DIR}/app-server.log"
    fi
  done
  rm -rf "${recovery_home}"
}
trap cleanup_recovery EXIT
export TALOS_HOME="${recovery_home}"
export TALOS_CONFIG="${recovery_home}/conf/talos.yaml"
mkdir -p "${recovery_home}/conf"
python3 - "${REPO_ROOT}/conf/talos-dev.yaml" "${TALOS_CONFIG}" <<'PY'
import json, sys, yaml
from pathlib import Path
Path(sys.argv[2]).write_text(json.dumps(yaml.safe_load(Path(sys.argv[1]).read_text())))
PY
export TALOS_SERVER_BIN="${TALOS_E2E_SERVER_BIN:-${TALOS_RS_DIR}/target/debug/talos-server}"
export TALOS_APP_SERVER_BIN="${TALOS_E2E_APP_SERVER_BIN:-${TALOS_RS_DIR}/target/debug/app-server}"
export TALOS_SCENARIO_REPORT_DIR="${SCENARIO_REPORT_DIR}"
pnpm --dir "${TALOS_APP_DIR}/desktop" exec vite build --config "${SCRIPT_DIR}/../../scripts/desktop-recovery.vite.config.ts"
timeout 180 node "${SCENARIO_REPORT_DIR}/desktop-recovery.mjs"
