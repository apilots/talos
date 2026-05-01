#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command curl
require_command python3
require_command mktemp
require_command tmux
build_binaries

TEST_TALOS_HOME="$(mktemp -d "${TMPDIR:-/tmp}/talos-tui-quit-home.XXXXXX")"
cleanup() {
  if [[ -n "${TUI_SESSION_NAME:-}" ]] && tmux has-session -t "${TUI_SESSION_NAME}" 2>/dev/null; then
    tmux kill-session -t "${TUI_SESSION_NAME}" || true
  fi
  stop_server
  rm -rf "${TEST_TALOS_HOME}"
}
export TALOS_HOME="${TEST_TALOS_HOME}"

export TALOS_APP_SERVER_BIN="${ROOT_DIR}/target/debug/talos"
export TALOS_LOCAL_LAUNCH_TOKEN="${TALOS_LOCAL_LAUNCH_TOKEN:-embedded-tui-quit-token}"

start_server
trap cleanup EXIT

WORKSPACE_DIR="${TALOS_HOME}/workspace-talos"
ROOT_MEMORY_FILE="${WORKSPACE_DIR}/MEMORY.md"
DAY_FILE="${WORKSPACE_DIR}/memory/$(date '+%Y-%m-%d').md"
mkdir -p "${WORKSPACE_DIR}"
TUI_SESSION_NAME="talos-tui-quit-$$"

capture_tui() {
  tmux capture-pane -t "${TUI_SESSION_NAME}" -p
}

get_session_id_from_http() {
  local sessions_json
  sessions_json="$(curl -sS -H "Authorization: Bearer ${AUTH_TOKEN}" \
    "${GATEWAY_URL}/v1/sessions" || true)"
  SESSION_ID_FROM_HTTP="$(python3 - "$sessions_json" <<'PY'
import json
import sys

raw = sys.argv[1]
if not raw:
    print("")
    sys.exit(0)
try:
    payload = json.loads(raw)
except Exception:
    print("")
    sys.exit(0)

sessions = []
if isinstance(payload, dict):
    sessions = payload.get("data") or payload.get("sessions") or []
elif isinstance(payload, list):
    sessions = payload

if not sessions:
    if isinstance(payload, dict):
        fallback = payload.get("session_id") or payload.get("id")
        if fallback:
            print(fallback)
            sys.exit(0)
    print("")
    sys.exit(0)
print(sessions[-1].get("context", {}).get("session_id", ""))
PY
)"
  if [[ -z "${SESSION_ID_FROM_HTTP}" ]]; then
    echo "embedded_tui_quit_get_session_list_payload=$(printf '%s' "${sessions_json}" | tr -d '\\n')"
  fi
}

send_tui_input() {
  local text="$1"
  tmux send-keys -t "${TUI_SESSION_NAME}" C-u >/dev/null
  tmux send-keys -t "${TUI_SESSION_NAME}" -l "${text}" >/dev/null
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

for _ in $(seq 1 240); do
  snapshot="$(capture_tui || true)"
  if echo "${snapshot}" | grep -Fq "${message_marker}"; then
    break
  fi
  sleep 0.25
done

session_id=""
for _ in $(seq 1 120); do
  get_session_id_from_http
  if [[ -n "${SESSION_ID_FROM_HTTP:-}" ]]; then
    session_id="${SESSION_ID_FROM_HTTP}"
    break
  fi
  sleep 0.25
done
if [[ -z "${session_id}" ]]; then
  echo "CASE embedded_tui_quit_get_session FAIL snapshot=$(capture_tui || true)"
  exit 1
fi
echo "CASE embedded_tui_quit_get_session PASS session_id=${session_id}"

send_tui_input "/quit"
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

found_session_entry="false"
flush_log_seen="false"
post_size="${pre_size}"
for _ in $(seq 1 480); do
  if [[ -f "${DAY_FILE}" ]]; then
    post_size="$(wc -c < "${DAY_FILE}" | tr -d ' ')"
    if grep -Fq "session: ${session_id}" "${DAY_FILE}"; then
      found_session_entry="true"
    fi
  fi
  if grep -Fq "memory flush completed session_id=${session_id}" "${SERVER_LOG}"; then
    flush_log_seen="true"
  fi
  if [[ "${found_session_entry}" == "true" && "${flush_log_seen}" == "true" ]]; then
    break
  fi
  sleep 0.25
done

if [[ "${found_session_entry}" != "true" ]]; then
  echo "CASE close_session_for_embedded_tui_quit FAIL session_id=${session_id} marker=${message_marker} pre_size=${pre_size} post_size=${post_size}"
  echo "CASE close_session_for_embedded_tui_quit FAIL day_file_missing=$(test -f "${DAY_FILE}" || echo missing)"
  exit 1
fi

if [[ "${flush_log_seen}" != "true" ]]; then
  echo "CASE close_session_for_embedded_tui_quit FAIL missing_flush_log session_id=${session_id}"
  exit 1
fi

if [[ ! -f "${DAY_FILE}" ]]; then
  echo "CASE close_session_for_embedded_tui_quit FAIL missing_day_file=${DAY_FILE}"
  exit 1
fi

if [[ "${post_size}" -le "${pre_size}" ]]; then
  echo "CASE close_session_for_embedded_tui_quit FAIL memory_file_not_increased pre_size=${pre_size} post_size=${post_size}"
  exit 1
fi

echo "CASE close_session_for_embedded_tui_quit PASS session_id=${session_id} marker=${message_marker} pre_size=${pre_size} post_size=${post_size}"
