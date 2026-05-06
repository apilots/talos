#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command python3
require_command mktemp
require_command tmux
build_binaries

TEST_TALOS_HOME="$(mktemp -d "${TMPDIR:-/tmp}/talos-tui-quit-home.XXXXXX")"
cleanup() {
  if [[ -n "${TUI_SESSION_NAME:-}" ]] && tmux has-session -t "${TUI_SESSION_NAME}" 2>/dev/null; then
    tmux kill-session -t "${TUI_SESSION_NAME}" || true
  fi
  rm -rf "${TEST_TALOS_HOME}"
}
export TALOS_HOME="${TEST_TALOS_HOME}"

export TALOS_APP_SERVER_BIN="${ROOT_DIR}/target/debug/talos"
export TALOS_LOCAL_LAUNCH_TOKEN="${TALOS_LOCAL_LAUNCH_TOKEN:-embedded-tui-quit-token}"

prepare_talos_home
trap cleanup EXIT

WORKSPACE_DIR="${TALOS_HOME}/workspace-talos"
ROOT_MEMORY_FILE="${WORKSPACE_DIR}/MEMORY.md"
DAY_FILE="${WORKSPACE_DIR}/memory/$(date '+%Y-%m-%d').md"
TALOS_LOG_FILE="${TALOS_HOME}/logs/talos.log"
mkdir -p "${WORKSPACE_DIR}"
TUI_SESSION_NAME="talos-tui-quit-$$"

capture_tui() {
  tmux capture-pane -t "${TUI_SESSION_NAME}" -p
}

get_session_id_from_sqlite() {
  SESSION_ID_FROM_SQLITE="$(python3 - "${TALOS_HOME}/data/talos.db" <<'PY'
import sqlite3
import sys
from pathlib import Path

db_path = Path(sys.argv[1])
if not db_path.exists():
    print("")
    raise SystemExit(0)
try:
    conn = sqlite3.connect(str(db_path))
    row = conn.execute(
        "SELECT session_key FROM sessions ORDER BY created_at DESC LIMIT 1"
    ).fetchone()
except Exception:
    print("")
    raise SystemExit(0)
print(row[0] if row else "")
PY
)"
}

log_line_contains() {
  local message="$1"
  local field="$2"
  if [[ ! -f "${TALOS_LOG_FILE}" ]]; then
    return 1
  fi
  grep -F "${message}" "${TALOS_LOG_FILE}" | grep -Fq "${field}"
}

log_contains() {
  local message="$1"
  if [[ ! -f "${TALOS_LOG_FILE}" ]]; then
    return 1
  fi
  grep -Fq "${message}" "${TALOS_LOG_FILE}"
}

dump_failure_context() {
  echo "CASE close_session_for_embedded_tui_quit DEBUG talos_home=${TALOS_HOME}"
  echo "CASE close_session_for_embedded_tui_quit DEBUG tui_snapshot_begin"
  capture_tui || true
  echo "CASE close_session_for_embedded_tui_quit DEBUG tui_snapshot_end"
  if [[ -f "${TALOS_LOG_FILE}" ]]; then
    echo "CASE close_session_for_embedded_tui_quit DEBUG talos_log_tail_begin"
    tail -n 120 "${TALOS_LOG_FILE}" || true
    echo "CASE close_session_for_embedded_tui_quit DEBUG talos_log_tail_end"
  else
    echo "CASE close_session_for_embedded_tui_quit DEBUG missing_talos_log=${TALOS_LOG_FILE}"
  fi
}

send_tui_input() {
  local text="$1"
  tmux send-keys -t "${TUI_SESSION_NAME}" C-u >/dev/null
  tmux send-keys -t "${TUI_SESSION_NAME}" -l "${text}" >/dev/null
  tmux send-keys -t "${TUI_SESSION_NAME}" C-m >/dev/null
}

send_tui_command() {
  local text="$1"
  tmux send-keys -t "${TUI_SESSION_NAME}" C-u >/dev/null
  tmux send-keys -t "${TUI_SESSION_NAME}" -l "${text}" >/dev/null
  tmux send-keys -t "${TUI_SESSION_NAME}" C-m >/dev/null
  tmux send-keys -t "${TUI_SESSION_NAME}" C-m >/dev/null
}

wait_for_tui_ready() {
  local i
  for i in $(seq 1 120); do
    local snapshot
    snapshot="$(capture_tui || true)"
    if echo "${snapshot}" | grep -Fq "Type a message or use /help"; then
      return 0
    fi
    sleep 0.25
  done
  echo "CASE tui_bootstrap FAIL timeout waiting for ready prompt"
  echo "CASE tui_bootstrap FAIL snapshot=$(capture_tui || true)"
  exit 1
}

pre_size=0
if [[ -f "${DAY_FILE}" ]]; then
  pre_size="$(wc -c < "${DAY_FILE}" | tr -d ' ')"
fi

tmux new-session -d -s "${TUI_SESSION_NAME}" \
  "cd '${ROOT_DIR}' && TALOS_HOME='${TALOS_HOME}' TALOS_APP_SERVER_BIN='${TALOS_APP_SERVER_BIN}' TALOS_LOCAL_LAUNCH_TOKEN='${TALOS_LOCAL_LAUNCH_TOKEN}' ./target/debug/talos-ui-tui --app-server-bin ./target/debug/talos"
wait_for_tui_ready

for _ in $(seq 1 160); do
  if [[ -f "${ROOT_MEMORY_FILE}" ]]; then
    break
  fi
  sleep 0.25
done
if [[ ! -f "${ROOT_MEMORY_FILE}" ]]; then
  echo "CASE embedded_tui_quit_bootstrap_root_memory WARN missing=${ROOT_MEMORY_FILE}"
else
  echo "CASE embedded_tui_quit_bootstrap_root_memory PASS path=${ROOT_MEMORY_FILE}"
fi

message_marker="tui-quit-memory-${RANDOM}-$(date '+%Y%m%d%H%M%S')"
send_tui_input "Please remember this marker for flush: ${message_marker}. This is an explicit memory target, and we should write it to today's daily memory file on session close."

for _ in $(seq 1 360); do
  snapshot="$(capture_tui || true)"
  if echo "${snapshot}" | grep -Fq "${message_marker}" && echo "${snapshot}" | grep -Fq "Brewed for"; then
    break
  fi
  sleep 0.25
done

session_id=""
for _ in $(seq 1 120); do
  get_session_id_from_sqlite
  if [[ -n "${SESSION_ID_FROM_SQLITE:-}" ]]; then
    session_id="${SESSION_ID_FROM_SQLITE}"
    break
  fi
  sleep 0.25
done
if [[ -z "${session_id}" ]]; then
  echo "CASE embedded_tui_quit_get_session FAIL snapshot=$(capture_tui || true)"
  exit 1
fi
echo "CASE embedded_tui_quit_get_session PASS session_id=${session_id}"

send_tui_command "/quit"
for _ in $(seq 1 240); do
  if ! tmux has-session -t "${TUI_SESSION_NAME}" 2>/dev/null; then
    break
  fi
  snapshot="$(capture_tui || true)"
  if echo "${snapshot}" | grep -Fq "Shutting down TUI. Closed session"; then
    break
  fi
  sleep 0.25
done

flush_started_seen="false"
flush_completed_seen="false"
flush_drain_completed_seen="false"
background_drain_completed_seen="false"
post_size="${pre_size}"
for _ in $(seq 1 480); do
  if [[ -f "${DAY_FILE}" ]]; then
    post_size="$(wc -c < "${DAY_FILE}" | tr -d ' ')"
  fi
  if log_line_contains "memory flush started" "session_id=${session_id}"; then
    flush_started_seen="true"
  fi
  if log_line_contains "memory flush completed" "session_id=${session_id}"; then
    flush_completed_seen="true"
  fi
  if log_contains "coordinator memory flush drain completed"; then
    flush_drain_completed_seen="true"
  fi
  if log_contains "background task drain completed"; then
    background_drain_completed_seen="true"
  fi
  if [[ "${flush_started_seen}" == "true" && "${flush_completed_seen}" == "true" && "${flush_drain_completed_seen}" == "true" && "${background_drain_completed_seen}" == "true" ]]; then
    break
  fi
  sleep 0.25
done

if [[ "${flush_started_seen}" != "true" ]]; then
  dump_failure_context
  echo "CASE close_session_for_embedded_tui_quit FAIL missing_flush_started_log session_id=${session_id}"
  exit 1
fi

if [[ "${flush_completed_seen}" != "true" ]]; then
  dump_failure_context
  echo "CASE close_session_for_embedded_tui_quit FAIL missing_flush_completed_log session_id=${session_id}"
  exit 1
fi

if [[ "${flush_drain_completed_seen}" != "true" ]]; then
  dump_failure_context
  echo "CASE close_session_for_embedded_tui_quit FAIL missing_flush_drain_completed_log session_id=${session_id}"
  exit 1
fi

if [[ "${background_drain_completed_seen}" != "true" ]]; then
  dump_failure_context
  echo "CASE close_session_for_embedded_tui_quit FAIL missing_background_drain_completed_log session_id=${session_id}"
  exit 1
fi

echo "CASE close_session_for_embedded_tui_quit PASS session_id=${session_id} marker=${message_marker} pre_size=${pre_size} post_size=${post_size}"
