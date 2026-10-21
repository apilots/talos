#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/tui.sh"

require_command python3
require_command mktemp
require_command tmux
build_binaries

TEST_TALOS_HOME="$(make_e2e_temp_dir "talos-tui-session-controls-home")"
export TALOS_HOME="${TEST_TALOS_HOME}"
export TALOS_TEST_CONTEXT_COMPACTION_MAX_BYTES=1024
prepare_talos_home

WORKSPACE_DIR="${TALOS_HOME}/workspace-talos"
USER_WORKSPACE_DIR="${WORKSPACE_DIR}/anonymous"
DATABASE_PATH="${TALOS_HOME}/data/talos.db"
TALOS_LOG_FILE="${TALOS_HOME}/logs/talos.log"
PANE_SNAPSHOT="$(scenario_report_path "tui_session_controls_pane.txt")"
SESSION_STATE_SNAPSHOT="$(scenario_report_path "tui_session_controls_state.json")"
TUI_SESSION_NAME="talos-tui-session-controls-$$"

cleanup() {
  tui_stop "${TUI_SESSION_NAME:-}"
  rm -rf "${TEST_TALOS_HOME}"
}
trap cleanup EXIT

capture_tui() {
  tui_capture "${TUI_SESSION_NAME}"
}

dump_failure_context() {
  capture_tui >"${PANE_SNAPSHOT}" || true
  echo "CASE embedded_tui_session_controls DEBUG talos_home=${TALOS_HOME}"
  echo "CASE embedded_tui_session_controls DEBUG pane_snapshot=${PANE_SNAPSHOT}"
  cat "${PANE_SNAPSHOT}" || true
  echo "CASE embedded_tui_session_controls DEBUG state_snapshot=${SESSION_STATE_SNAPSHOT}"
  if [[ -f "${TALOS_LOG_FILE}" ]]; then
    echo "CASE embedded_tui_session_controls DEBUG talos_log_tail_begin"
    tail -n 160 "${TALOS_LOG_FILE}" || true
    echo "CASE embedded_tui_session_controls DEBUG talos_log_tail_end"
  fi
}

read_session_state() {
  local session_id="$1"
  python3 - "${DATABASE_PATH}" "${session_id}" "${SESSION_STATE_SNAPSHOT}" <<'PY'
import json
import sqlite3
import sys
from pathlib import Path

database_path = Path(sys.argv[1])
session_id = sys.argv[2]
snapshot_path = Path(sys.argv[3])
if not database_path.exists():
    print("{}")
    raise SystemExit(0)

connection = sqlite3.connect(str(database_path))
row = connection.execute(
    "SELECT title, message_count, usage_json, conf_json FROM sessions WHERE session_key = ?",
    (session_id,),
).fetchone()
if not row:
    print("{}")
    raise SystemExit(0)

payload = {
    "title": row[0],
    "message_count": row[1],
    "usage": json.loads(row[2] or "{}"),
    "model_selection": (json.loads(row[3] or "{}").get("modelSelection")),
}
snapshot_path.write_text(json.dumps(payload, indent=2), encoding="utf-8")
print(json.dumps(payload))
PY
}

session_model_selection() {
  local session_id="$1"
  local state
  state="$(read_session_state "${session_id}")"
  python3 - "${state}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1] or "{}")
print(json.dumps(payload.get("model_selection") or {}, separators=(",", ":")))
PY
}

session_message_count() {
  local session_id="$1"
  local state
  state="$(read_session_state "${session_id}")"
  python3 - "${state}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1] or "{}")
print(payload.get("message_count", -1))
PY
}

wait_for_message_count() {
  local session_id="$1"
  local minimum="$2"
  for _ in $(seq 1 360); do
    local count
    if capture_tui | grep -Fq "Stream error:"; then
      return 1
    fi
    count="$(session_message_count "${session_id}")"
    if [[ "${count}" -ge "${minimum}" ]]; then
      return 0
    fi
    sleep 0.25
  done
  return 1
}

wait_for_session_id() {
  for _ in $(seq 1 120); do
    local session_id
    session_id="$(tui_latest_session_id "${DATABASE_PATH}")"
    if [[ -n "${session_id}" ]]; then
      printf '%s\n' "${session_id}"
      return 0
    fi
    sleep 0.25
  done
  return 1
}

assert_summary_artifact() {
  local session_dir="$1"
  local session_id="$2"
  python3 - "${session_dir}" "${session_id}" <<'PY'
import json
import sys
from pathlib import Path

session_dir = Path(sys.argv[1])
session_id = sys.argv[2]
for artifact_path in sorted(session_dir.glob("*.jsonl")):
    for line in artifact_path.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        payload = json.loads(line)
        record = payload.get("record") or {}
        header = record.get("header") or {}
        if payload.get("kind") == "summary" and header.get("session_id") == session_id:
            raise SystemExit(0)
raise SystemExit(1)
PY
}

assert_cleared_database_state() {
  local session_id="$1"
  local state
  state="$(read_session_state "${session_id}")"
  python3 - "${state}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1] or "{}")
usage = payload.get("usage") or {}
tokens = usage.get("tokens") or {}
usage_total = tokens.get("total_tokens", tokens.get("totalTokens", 0))
valid = (
    payload.get("title") == "Untitled Session"
    and payload.get("message_count") == 0
    and usage_total == 0
)
raise SystemExit(0 if valid else 1)
PY
}

assert_cleared_artifacts() {
  local session_dir="$1"
  python3 - "${session_dir}" <<'PY'
import sys
from pathlib import Path

session_dir = Path(sys.argv[1])
if not session_dir.is_dir():
    raise SystemExit(1)
if any(session_dir.glob("*.jsonl")):
    raise SystemExit(1)
raise SystemExit(0)
PY
}

mkdir -p "${WORKSPACE_DIR}"
tmux new-session -d -s "${TUI_SESSION_NAME}" \
  "cd '${WORKSPACE_DIR}' && TALOS_HOME='${TALOS_HOME}' TALOS_RESOURCE_DIR='${TALOS_RESOURCE_DIR}' '${TALOS_E2E_CLI_BIN}'"

if ! tui_wait_for_text "${TUI_SESSION_NAME}" "Type a message, Ctrl+V an image, or use /help" 160 0.25 "${PANE_SNAPSHOT}"; then
  dump_failure_context
  echo "CASE embedded_tui_session_controls_bootstrap FAIL"
  exit 1
fi
echo "CASE embedded_tui_session_controls_bootstrap PASS"

message_marker="TUI_SESSION_CONTROLS_${RANDOM}_$$"
printf -v compaction_padding '%02048d' 0
tui_send_input "${TUI_SESSION_NAME}" "Remember marker ${message_marker} and context ${compaction_padding} for this session, then reply briefly without calling tools."

session_id="$(wait_for_session_id || true)"
if [[ -z "${session_id}" ]]; then
  dump_failure_context
  echo "CASE embedded_tui_session_controls_session FAIL"
  exit 1
fi
session_dir="${USER_WORKSPACE_DIR}/sessions/${session_id}"

if ! wait_for_message_count "${session_id}" 2; then
  dump_failure_context
  echo "CASE embedded_tui_session_controls_initial_turn FAIL session_id=${session_id}"
  exit 1
fi
capture_tui >"${PANE_SNAPSHOT}" || true
if grep -Fq "Stream error:" "${PANE_SNAPSHOT}"; then
  dump_failure_context
  echo "CASE embedded_tui_session_controls_initial_turn FAIL stream_error session_id=${session_id}"
  exit 1
fi
if [[ ! -d "${session_dir}" ]]; then
  dump_failure_context
  echo "CASE embedded_tui_session_controls_initial_turn FAIL missing_session_dir=${session_dir}"
  exit 1
fi
initial_message_count="$(session_message_count "${session_id}")"
initial_model_selection="$(session_model_selection "${session_id}")"
if [[ "$(json_get "${initial_model_selection}" "modelRef.provider")" == "" || "$(json_get "${initial_model_selection}" "modelRef.id")" == "" ]]; then
  dump_failure_context
  echo "CASE embedded_tui_session_controls_initial_turn FAIL missing_model_selection=${initial_model_selection}"
  exit 1
fi
echo "CASE embedded_tui_session_controls_initial_turn PASS session_id=${session_id} message_count=${initial_message_count} model_selection=${initial_model_selection}"

tui_send_command "${TUI_SESSION_NAME}" "/compact"
if ! tui_wait_for_text "${TUI_SESSION_NAME}" "Session compaction completed" 480 0.25 "${PANE_SNAPSHOT}"; then
  dump_failure_context
  echo "CASE embedded_tui_session_controls_compact FAIL missing_tui_result session_id=${session_id}"
  exit 1
fi
if ! assert_summary_artifact "${session_dir}" "${session_id}"; then
  dump_failure_context
  echo "CASE embedded_tui_session_controls_compact FAIL missing_summary session_id=${session_id}"
  exit 1
fi
compact_message_count="$(session_message_count "${session_id}")"
compact_model_selection="$(session_model_selection "${session_id}")"
if [[ "${compact_message_count}" -ne "${initial_message_count}" ]] || ! json_equal "${compact_model_selection}" "${initial_model_selection}"; then
  dump_failure_context
  echo "CASE embedded_tui_session_controls_compact FAIL message_count_before=${initial_message_count} message_count_after=${compact_message_count} selection_before=${initial_model_selection} selection_after=${compact_model_selection}"
  exit 1
fi
echo "CASE embedded_tui_session_controls_compact PASS session_id=${session_id} message_count=${compact_message_count}"

tui_send_command "${TUI_SESSION_NAME}" "/clear"
if ! tui_wait_for_text "${TUI_SESSION_NAME}" "Session context cleared" 120 0.25 "${PANE_SNAPSHOT}"; then
  dump_failure_context
  echo "CASE embedded_tui_session_controls_clear FAIL missing_tui_result session_id=${session_id}"
  exit 1
fi
clear_model_selection="$(session_model_selection "${session_id}")"
if ! assert_cleared_artifacts "${session_dir}" || ! assert_cleared_database_state "${session_id}" || ! json_equal "${clear_model_selection}" "${initial_model_selection}"; then
  dump_failure_context
  echo "CASE embedded_tui_session_controls_clear FAIL session_id=${session_id} session_dir=${session_dir} selection_before=${initial_model_selection} selection_after=${clear_model_selection}"
  exit 1
fi
echo "CASE embedded_tui_session_controls_clear PASS session_id=${session_id}"

post_clear_marker="TUI_POST_CLEAR_${RANDOM}_$$"
tui_send_input "${TUI_SESSION_NAME}" "Reply briefly with marker ${post_clear_marker} and do not call tools."
if ! wait_for_message_count "${session_id}" 2; then
  dump_failure_context
  echo "CASE embedded_tui_session_controls_post_clear_turn FAIL session_id=${session_id}"
  exit 1
fi
capture_tui >"${PANE_SNAPSHOT}" || true
if grep -Fq "Stream error:" "${PANE_SNAPSHOT}"; then
  dump_failure_context
  echo "CASE embedded_tui_session_controls_post_clear_turn FAIL stream_error session_id=${session_id}"
  exit 1
fi
echo "CASE embedded_tui_session_controls_post_clear_turn PASS session_id=${session_id}"

tui_send_command "${TUI_SESSION_NAME}" "/clear"
for _ in $(seq 1 120); do
  if assert_cleared_artifacts "${session_dir}" && assert_cleared_database_state "${session_id}"; then
    break
  fi
  sleep 0.25
done
if ! assert_cleared_artifacts "${session_dir}" || ! assert_cleared_database_state "${session_id}"; then
  dump_failure_context
  echo "CASE embedded_tui_session_controls_cleanup_clear FAIL session_id=${session_id}"
  exit 1
fi

tui_send_command "${TUI_SESSION_NAME}" "/quit"
if ! tui_wait_for_exit "${TUI_SESSION_NAME}" 280 0.25; then
  dump_failure_context
  echo "CASE embedded_tui_session_controls_exit FAIL session_id=${session_id}"
  exit 1
fi
echo "CASE embedded_tui_session_controls_exit PASS session_id=${session_id}"
