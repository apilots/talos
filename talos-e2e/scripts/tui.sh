#!/usr/bin/env bash

tui_capture() {
  local session_name="$1"
  tmux capture-pane -t "${session_name}" -p
}

tui_send_input() {
  local session_name="$1"
  local text="$2"
  tmux send-keys -t "${session_name}" C-u >/dev/null
  tmux send-keys -t "${session_name}" -l "${text}" >/dev/null
  tmux send-keys -t "${session_name}" C-m >/dev/null
}

tui_send_command() {
  local session_name="$1"
  local command="$2"
  tui_send_input "${session_name}" "${command}"
  # The first Enter may accept command autocomplete; the second executes it.
  tmux send-keys -t "${session_name}" C-m >/dev/null
}

tui_wait_for_text() {
  local session_name="$1"
  local expected="$2"
  local max_attempts="${3:-120}"
  local delay_seconds="${4:-0.25}"
  local snapshot_path="${5:-}"

  for _ in $(seq 1 "${max_attempts}"); do
    local snapshot
    snapshot="$(tui_capture "${session_name}" || true)"
    if [[ -n "${snapshot_path}" ]]; then
      printf '%s\n' "${snapshot}" >"${snapshot_path}"
    fi
    if echo "${snapshot}" | grep -Fq "${expected}"; then
      return 0
    fi
    sleep "${delay_seconds}"
  done
  return 1
}

tui_wait_for_exit() {
  local session_name="$1"
  local max_attempts="${2:-280}"
  local delay_seconds="${3:-0.25}"

  for _ in $(seq 1 "${max_attempts}"); do
    if ! tmux has-session -t "${session_name}" 2>/dev/null; then
      return 0
    fi
    sleep "${delay_seconds}"
  done
  return 1
}

tui_stop() {
  local session_name="$1"
  if [[ -n "${session_name}" ]] && tmux has-session -t "${session_name}" 2>/dev/null; then
    tmux kill-session -t "${session_name}" || true
  fi
}

tui_latest_session_id() {
  local database_path="$1"
  python3 - "${database_path}" <<'PY'
import sqlite3
import sys
from pathlib import Path

database_path = Path(sys.argv[1])
if not database_path.exists():
    print("")
    raise SystemExit(0)

try:
    connection = sqlite3.connect(str(database_path))
    row = connection.execute(
        "SELECT session_key FROM sessions ORDER BY created_at DESC LIMIT 1"
    ).fetchone()
except Exception:
    print("")
    raise SystemExit(0)

print(row[0] if row else "")
PY
}
