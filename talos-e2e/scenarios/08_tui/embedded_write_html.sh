#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/tui.sh"

require_command tmux
require_command python3
build_binaries

TEST_TALOS_HOME="$(make_e2e_temp_dir "talos-tui-write-home")"
export TALOS_HOME="${TEST_TALOS_HOME}"
prepare_talos_home

TARGET_DIR="$(scenario_report_path "tui-write-target")"
TARGET_FILE="${TARGET_DIR}/calculator.html"
PANE_SNAPSHOT="$(scenario_report_path "tui_write_pane.txt")"
SESSION_SNAPSHOT="$(scenario_report_path "tui_write_session.json")"
TALOS_LOG_FILE="${TALOS_HOME}/logs/talos.log"
TUI_SESSION_NAME="talos-tui-write-$$"
MARKER="TUI_EMBEDDED_WRITE_${RANDOM}_$$"
WARMUP_MARKER="TUI_EMBEDDED_WARMUP_${RANDOM}_$$"

cleanup() {
  tui_stop "${TUI_SESSION_NAME:-}"
  rm -rf "${TEST_TALOS_HOME}"
}
trap cleanup EXIT

capture_tui() {
  tui_capture "${TUI_SESSION_NAME}"
}

send_tui_input() {
  local text="$1"
  tui_send_input "${TUI_SESSION_NAME}" "${text}"
}

dump_failure_context() {
  capture_tui >"${PANE_SNAPSHOT}" || true
  echo "CASE embedded_tui_write DEBUG talos_home=${TALOS_HOME}"
  echo "CASE embedded_tui_write DEBUG target_file=${TARGET_FILE}"
  echo "CASE embedded_tui_write DEBUG pane_snapshot=${PANE_SNAPSHOT}"
  if [[ -f "${TALOS_LOG_FILE}" ]]; then
    echo "CASE embedded_tui_write DEBUG talos_log_tail_begin"
    tail -n 120 "${TALOS_LOG_FILE}" || true
    echo "CASE embedded_tui_write DEBUG talos_log_tail_end"
  fi
}

wait_for_tui_ready() {
  if tui_wait_for_text "${TUI_SESSION_NAME}" "Type a message, Ctrl+V an image, or use /help" 120 0.25; then
    return 0
  fi
  dump_failure_context
  echo "CASE embedded_tui_write_bootstrap FAIL"
  exit 1
}

read_latest_session_snapshot() {
  python3 - "${TALOS_HOME}/data/talos.db" "${SESSION_SNAPSHOT}" <<'PY'
import json
import sqlite3
import sys
from pathlib import Path

db_path = Path(sys.argv[1])
out_path = Path(sys.argv[2])
if not db_path.exists():
    print("{}")
    raise SystemExit(0)

conn = sqlite3.connect(str(db_path))
row = conn.execute(
    "SELECT session_key, current_directory, environment_json, conf_json, message_count FROM sessions ORDER BY created_at DESC LIMIT 1"
).fetchone()
if not row:
    print("{}")
    raise SystemExit(0)

payload = {
    "session_id": row[0],
    "current_directory": row[1],
    "environment": json.loads(row[2] or "null") or {},
    "model_selection": json.loads(row[3] or "{}").get("modelSelection"),
    "message_count": row[4],
}
out_path.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")
print(json.dumps(payload, ensure_ascii=False))
PY
}

mkdir -p "${TARGET_DIR}"
tmux new-session -d -s "${TUI_SESSION_NAME}" \
  "cd '${TALOS_RS_DIR}' && TALOS_HOME='${TALOS_HOME}' TALOS_RESOURCE_DIR='${TALOS_RESOURCE_DIR}' ./target/debug/talos"
wait_for_tui_ready

warmup_prompt="Reply exactly TUI_EMBEDDED_WARMUP_OK:${WARMUP_MARKER}. Do not call any tools."
send_tui_input "${warmup_prompt}"

warmup_completed="false"
for _ in $(seq 1 120); do
  capture_tui >"${PANE_SNAPSHOT}" || true
  if grep -Fq "Stream error:" "${PANE_SNAPSHOT}"; then
    break
  fi
  warmup_snapshot="$(read_latest_session_snapshot)"
  warmup_message_count="$(python3 - "${warmup_snapshot}" <<'PY'
import json
import sys

print((json.loads(sys.argv[1] or "{}") or {}).get("message_count", 0))
PY
)"
  if [[ "${warmup_message_count}" -ge 2 ]]; then
    warmup_completed="true"
    echo "CASE embedded_tui_write_warmup PASS marker=${WARMUP_MARKER}"
    break
  fi
  sleep 0.5
done

if [[ "${warmup_completed}" != "true" ]]; then
  dump_failure_context
  echo "CASE embedded_tui_write_warmup FAIL marker=${WARMUP_MARKER}"
  exit 1
fi

prompt="Use the Write tool exactly once to create the absolute path ${TARGET_FILE}. The file content must be a minimal standalone HTML calculator page, include marker ${MARKER}, include buttons for 1, +, 2, =, and show result 3 for 1+2. After the tool call completes, reply exactly TUI_EMBEDDED_WRITE_OK:${MARKER}. Do not only describe the file; actually write it."
send_tui_input "${prompt}"

for _ in $(seq 1 180); do
  capture_tui >"${PANE_SNAPSHOT}" || true
  if [[ -f "${TARGET_FILE}" ]] && grep -Fq "${MARKER}" "${TARGET_FILE}"; then
    echo "CASE embedded_tui_write_file PASS path=${TARGET_FILE}"
    break
  fi
  sleep 0.5
done

if [[ ! -f "${TARGET_FILE}" ]]; then
  dump_failure_context
  echo "CASE embedded_tui_write_file FAIL missing_file path=${TARGET_FILE}"
  exit 1
fi

if ! grep -Fq "${MARKER}" "${TARGET_FILE}"; then
  dump_failure_context
  echo "CASE embedded_tui_write_file FAIL missing_marker path=${TARGET_FILE}"
  exit 1
fi

if ! grep -Eiq "<html|<!doctype html" "${TARGET_FILE}"; then
  dump_failure_context
  echo "CASE embedded_tui_write_file FAIL not_html path=${TARGET_FILE}"
  exit 1
fi

session_snapshot="$(read_latest_session_snapshot)"
if ! python3 - "${session_snapshot}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1] or "{}")
environment = payload.get("environment") or {}
ok = (
    bool(payload.get("model_selection"))
    and payload.get("current_directory")
    and environment.get("connection_mode") == "embedded"
)
raise SystemExit(0 if ok else 1)
PY
then
  dump_failure_context
  echo "CASE embedded_tui_write_session FAIL snapshot=${SESSION_SNAPSHOT} data=${session_snapshot}"
  exit 1
fi

echo "CASE embedded_tui_write_session PASS snapshot=${SESSION_SNAPSHOT}"
