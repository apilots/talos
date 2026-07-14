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

TEST_TALOS_HOME="$(make_e2e_temp_dir "talos-tui-quit-home")"
cleanup() {
  tui_stop "${TUI_SESSION_NAME:-}"
  rm -rf "${TEST_TALOS_HOME}"
}
export TALOS_HOME="${TEST_TALOS_HOME}"

prepare_talos_home
trap cleanup EXIT

WORKSPACE_DIR="${TALOS_HOME}/workspace-talos"
USER_WORKSPACE_DIR="${WORKSPACE_DIR}/users/local"
ROOT_MEMORY_FILE="${USER_WORKSPACE_DIR}/MEMORY.md"
DAY_FILE="${USER_WORKSPACE_DIR}/memory/$(date '+%Y-%m-%d').md"
TALOS_LOG_FILE="${TALOS_HOME}/logs/talos.log"
QUIT_PANE_HISTORY="$(scenario_report_path "tui_quit_pane_history.txt")"
mkdir -p "${WORKSPACE_DIR}"
TUI_SESSION_NAME="talos-tui-quit-$$"

capture_tui() {
  tui_capture "${TUI_SESSION_NAME}"
}

get_session_id_from_sqlite() {
  SESSION_ID_FROM_SQLITE="$(tui_latest_session_id "${TALOS_HOME}/data/talos.db")"
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
  tui_send_input "${TUI_SESSION_NAME}" "${text}"
}

send_tui_command() {
  local text="$1"
  tui_send_command "${TUI_SESSION_NAME}" "${text}"
}

wait_for_tui_ready() {
  if tui_wait_for_text "${TUI_SESSION_NAME}" "Type a message or use /help" 120 0.25; then
    return 0
  fi
  echo "CASE tui_bootstrap FAIL timeout waiting for ready prompt"
  dump_failure_context
  exit 1
}

pre_size=0
if [[ -f "${DAY_FILE}" ]]; then
  pre_size="$(wc -c < "${DAY_FILE}" | tr -d ' ')"
fi

tmux new-session -d -s "${TUI_SESSION_NAME}" \
  "cd '${WORKSPACE_DIR}' && TALOS_HOME='${TALOS_HOME}' '${TALOS_RS_DIR}/target/debug/talos'"
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

turn_completed="false"
for _ in $(seq 1 360); do
  snapshot="$(capture_tui || true)"
  if echo "${snapshot}" | grep -Fq "Stream error:"; then
    break
  fi
  if echo "${snapshot}" | grep -Fq "${message_marker}" && echo "${snapshot}" | grep -Fq "Brewed for"; then
    turn_completed="true"
    break
  fi
  sleep 0.25
done
if [[ "${turn_completed}" != "true" ]]; then
  dump_failure_context
  echo "CASE embedded_tui_quit_turn FAIL marker=${message_marker}"
  exit 1
fi
echo "CASE embedded_tui_quit_turn PASS marker=${message_marker}"

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

: >"${QUIT_PANE_HISTORY}"
send_tui_command "/quit"
session_exited="false"
for _ in $(seq 1 280); do
  if ! tmux has-session -t "${TUI_SESSION_NAME}" 2>/dev/null; then
    session_exited="true"
    break
  fi
  snapshot="$(capture_tui || true)"
  printf '%s\n' "${snapshot}" >>"${QUIT_PANE_HISTORY}"
  if echo "${snapshot}" | grep -Fq "Session close timed out"; then
    dump_failure_context
    echo "CASE embedded_tui_quit_no_false_timeout FAIL pane_history=${QUIT_PANE_HISTORY}"
    exit 1
  fi
  sleep 0.25
done
if [[ "${session_exited}" != "true" ]]; then
  dump_failure_context
  echo "CASE embedded_tui_quit_exit FAIL pane_history=${QUIT_PANE_HISTORY}"
  exit 1
fi
echo "CASE embedded_tui_quit_no_false_timeout PASS pane_history=${QUIT_PANE_HISTORY}"
echo "CASE embedded_tui_quit_exit PASS session_id=${session_id}"

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

if [[ ! -f "${DAY_FILE}" ]]; then
  dump_failure_context
  echo "CASE close_session_for_embedded_tui_quit FAIL missing_memory_file path=${DAY_FILE}"
  exit 1
fi

if ! grep -Fq "${message_marker}" "${DAY_FILE}"; then
  dump_failure_context
  echo "CASE close_session_for_embedded_tui_quit FAIL missing_memory_marker marker=${message_marker} path=${DAY_FILE}"
  exit 1
fi

if [[ "${post_size}" -le "${pre_size}" ]]; then
  dump_failure_context
  echo "CASE close_session_for_embedded_tui_quit FAIL memory_file_not_grown pre_size=${pre_size} post_size=${post_size}"
  exit 1
fi

background_drain_started_count="$(grep -Fc "background task drain started" "${TALOS_LOG_FILE}" || true)"
background_drain_completed_count="$(grep -Fc "background task drain completed" "${TALOS_LOG_FILE}" || true)"
if [[ "${background_drain_started_count}" -ne 1 || "${background_drain_completed_count}" -ne 1 ]]; then
  dump_failure_context
  echo "CASE close_session_for_embedded_tui_quit FAIL unexpected_drain_count started=${background_drain_started_count} completed=${background_drain_completed_count}"
  exit 1
fi

echo "CASE close_session_for_embedded_tui_quit_memory PASS marker=${message_marker} path=${DAY_FILE}"
echo "CASE close_session_for_embedded_tui_quit_single_drain PASS started=${background_drain_started_count} completed=${background_drain_completed_count}"
echo "CASE close_session_for_embedded_tui_quit PASS session_id=${session_id} marker=${message_marker} pre_size=${pre_size} post_size=${post_size}"
