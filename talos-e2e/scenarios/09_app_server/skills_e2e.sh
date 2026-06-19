#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

load_env
build_binaries
prepare_talos_home

configure_skills_roots() {
  local config_path="${TALOS_HOME}/conf/talos.yaml"
  CONFIG_PATH="${config_path}" TALOS_HOME_PATH="${TALOS_HOME}" python3 - <<'PY'
import json
import os
from pathlib import Path

config_path = Path(os.environ["CONFIG_PATH"])
talos_home = Path(os.environ["TALOS_HOME_PATH"])
text = config_path.read_text()
skills_block = (
    "    skills:\n"
    "      enabled: true\n"
    "      roots:\n"
    f"        - {json.dumps(str(talos_home / 'skills'))}\n"
    f"        - {json.dumps(str(talos_home / 'agents-skills'))}\n"
    f"      install_root: {json.dumps(str(talos_home / 'skills'))}\n"
    f"      state_path: {json.dumps(str(talos_home / 'skills-state.json'))}\n"
    "      max_skill_md_bytes: 65536\n"
    "      max_available_summary_bytes: 8192\n"
)
marker = '    planner_mode: "built_in"\n'
if "    skills:\n" not in text:
    text = text.replace(marker, skills_block + marker)
config_path.write_text(text)
PY
}

configure_skills_roots

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
