#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/tui.sh"

require_command tmux
require_command python3
require_command sqlite3
build_binaries

TEST_TALOS_HOME="$(make_e2e_temp_dir "talos-tui-user-action-home")"
export TALOS_HOME="${TEST_TALOS_HOME}"
prepare_talos_home

TUI_SESSION_NAME="talos-tui-user-action-$$"
PANE_SNAPSHOT="$(scenario_report_path "tui_user_action_pane.txt")"
DATABASE_PATH="${TALOS_HOME}/data/talos.db"

cleanup() {
  tui_stop "${TUI_SESSION_NAME:-}"
  rm -rf "${TEST_TALOS_HOME}"
}
trap cleanup EXIT

dump_failure_context() {
  tui_capture "${TUI_SESSION_NAME}" >"${PANE_SNAPSHOT}" || true
  echo "CASE tui_user_action DEBUG pane_snapshot=${PANE_SNAPSHOT}"
  if [[ -f "${TALOS_HOME}/logs/talos.log" ]]; then
    tail -n 120 "${TALOS_HOME}/logs/talos.log" || true
  fi
}

latest_action_json() {
  python3 - "${DATABASE_PATH}" <<'PY'
import json
import sqlite3
import sys
from pathlib import Path

database = Path(sys.argv[1])
if not database.exists():
    print("{}")
    raise SystemExit(0)

connection = sqlite3.connect(str(database))
row = connection.execute(
    "SELECT action_id, state, action_json, response_json FROM user_actions ORDER BY updated_at DESC LIMIT 1"
).fetchone()
if not row:
    print("{}")
    raise SystemExit(0)
action = json.loads(row[2])
print(json.dumps({
    "action_id": row[0],
    "state": row[1],
    "params": action["params"],
    "response": json.loads(row[3]) if row[3] else None,
}))
PY
}

wait_for_session_completed() {
  local session_id="$1"
  for _ in $(seq 1 160); do
    local status
    status="$(sqlite3 -cmd ".timeout 5000" "${DATABASE_PATH}" "select status from active_turns where session_id='${session_id}';")"
    if [[ "${status}" == "completed" ]]; then
      return 0
    fi
    if [[ "${status}" == "failed" || "${status}" == "cancelled" ]]; then
      echo "Active turn reached terminal status ${status} while waiting for completed." >&2
      return 1
    fi
    sleep 0.25
  done
  return 1
}

wait_for_overlay_closed() {
  for _ in $(seq 1 120); do
    local snapshot
    snapshot="$(tui_capture "${TUI_SESSION_NAME}" || true)"
    if ! grep -Fq "Talos needs your input" <<<"${snapshot}" && ! grep -Fq "Choose what Talos is allowed to do" <<<"${snapshot}"; then
      return 0
    fi
    sleep 0.25
  done
  return 1
}

wait_for_action_state() {
  local expected_state="$1"
  local previous_action_id="${2:-}"
  for _ in $(seq 1 160); do
    local action_json action_id state
    action_json="$(latest_action_json)"
    action_id="$(json_get "${action_json}" "action_id")"
    state="$(json_get "${action_json}" "state")"
    if [[ -n "${action_id}" && "${action_id}" != "${previous_action_id}" && "${state}" == "${expected_state}" ]]; then
      printf '%s\n' "${action_json}"
      return 0
    fi
    sleep 0.25
  done
  return 1
}

tmux new-session -d -s "${TUI_SESSION_NAME}" \
  "cd '${TALOS_RS_DIR}' && TALOS_HOME='${TALOS_HOME}' TALOS_RESOURCE_DIR='${TALOS_RESOURCE_DIR}' ./target/debug/talos"
if ! tui_wait_for_text "${TUI_SESSION_NAME}" "Type a message, Ctrl+V an image, or use /help" 120 0.25 "${PANE_SNAPSHOT}"; then
  dump_failure_context
  echo "CASE tui_user_action_bootstrap FAIL"
  exit 1
fi

tui_send_input "${TUI_SESSION_NAME}" "Reply with exactly TUI_USER_ACTION_READY"
for _ in $(seq 1 160); do
  session_id="$(tui_latest_session_id "${DATABASE_PATH}")"
  if [[ -n "${session_id}" ]]; then
    break
  fi
  sleep 0.25
done
if [[ -z "${session_id:-}" ]]; then
  dump_failure_context
  echo "CASE tui_user_action_session FAIL"
  exit 1
fi
if ! wait_for_session_completed "${session_id}"; then
  dump_failure_context
  echo "CASE tui_user_action_session_idle FAIL"
  exit 1
fi
echo "CASE tui_user_action_session PASS session_id=${session_id}"

tui_send_command "${TUI_SESSION_NAME}" "/permissions"
if ! tui_wait_for_text "${TUI_SESSION_NAME}" "Choose what Talos is allowed to do" 80 0.25 "${PANE_SNAPSHOT}"; then
  dump_failure_context
  echo "CASE tui_permissions_picker FAIL"
  exit 1
fi
tmux send-keys -t "${TUI_SESSION_NAME}" Up Up Enter >/dev/null
for _ in $(seq 1 80); do
  effective_mode="$(sqlite3 -cmd ".timeout 5000" "${DATABASE_PATH}" "select json_extract(conf_json, '$.userPermissionMode') from sessions where session_key='${session_id}';")"
  if [[ "${effective_mode}" == "askForApproval" ]]; then
    break
  fi
  sleep 0.25
done
if [[ "${effective_mode:-}" != "askForApproval" ]]; then
  dump_failure_context
  echo "CASE tui_permissions_mode FAIL mode=${effective_mode:-}"
  exit 1
fi
if ! wait_for_overlay_closed; then
  dump_failure_context
  echo "CASE tui_permissions_close FAIL"
  exit 1
fi
echo "CASE tui_permissions_mode PASS mode=${effective_mode}"

question_prompt="You must call AskUserQuestion exactly once. Use header TUI Choice, question Choose delivery, and exactly these options: id now, label Now, description Deliver now; id later, label Later, description Deliver later. After receiving the answer, respond with exactly TUI_QUESTION_OPTION_OK"
tui_send_input "${TUI_SESSION_NAME}" "${question_prompt}"
if ! first_pending="$(wait_for_action_state "pending")"; then
  dump_failure_context
  echo "CASE tui_question_option_waiting FAIL"
  exit 1
fi
first_action_id="$(json_get "${first_pending}" "action_id")"
if ! tui_wait_for_text "${TUI_SESSION_NAME}" "Talos needs your input" 80 0.25 "${PANE_SNAPSHOT}"; then
  dump_failure_context
  echo "CASE tui_question_option_overlay FAIL"
  exit 1
fi
tui_stop "${TUI_SESSION_NAME}"
tmux new-session -d -s "${TUI_SESSION_NAME}" \
  "cd '${TALOS_RS_DIR}' && TALOS_HOME='${TALOS_HOME}' TALOS_RESOURCE_DIR='${TALOS_RESOURCE_DIR}' ./target/debug/talos"
if ! tui_wait_for_text "${TUI_SESSION_NAME}" "Type a message, Ctrl+V an image, or use /help" 120 0.25 "${PANE_SNAPSHOT}"; then
  dump_failure_context
  echo "CASE tui_question_reconnect_bootstrap FAIL"
  exit 1
fi
tui_send_command "${TUI_SESSION_NAME}" "/sessions"
if ! tui_wait_for_text "${TUI_SESSION_NAME}" "Resume a previous session" 80 0.25 "${PANE_SNAPSHOT}"; then
  dump_failure_context
  echo "CASE tui_question_reconnect_picker FAIL"
  exit 1
fi
if ! tui_wait_for_text "${TUI_SESSION_NAME}" "1 / 1" 80 0.25 "${PANE_SNAPSHOT}"; then
  dump_failure_context
  echo "CASE tui_question_reconnect_session_list FAIL"
  exit 1
fi
tmux send-keys -t "${TUI_SESSION_NAME}" Enter >/dev/null
if ! tui_wait_for_text "${TUI_SESSION_NAME}" "Talos needs your input" 120 0.25 "${PANE_SNAPSHOT}"; then
  dump_failure_context
  echo "CASE tui_question_reconnect_overlay FAIL"
  exit 1
fi
recovered_pending="$(latest_action_json)"
if [[ "$(json_get "${recovered_pending}" "action_id")" != "${first_action_id}" || "$(json_get "${recovered_pending}" "state")" != "pending" ]]; then
  dump_failure_context
  echo "CASE tui_question_reconnect_identity FAIL action=${recovered_pending}"
  exit 1
fi
echo "CASE tui_question_reconnect PASS action_id=${first_action_id}"
tmux send-keys -t "${TUI_SESSION_NAME}" Enter >/dev/null
if ! first_resolved="$(wait_for_action_state "resolved" "")"; then
  dump_failure_context
  echo "CASE tui_question_option_submit FAIL"
  exit 1
fi
FIRST_RESOLVED="${first_resolved}" FIRST_ACTION_ID="${first_action_id}" python3 - <<'PY'
import json
import os

action = json.loads(os.environ["FIRST_RESOLVED"])
assert action["action_id"] == os.environ["FIRST_ACTION_ID"], action
assert action["response"] == {
    "type": "question",
    "response": {"type": "selectedOption", "optionId": "now"},
}, action
PY
echo "CASE tui_question_option PASS action_id=${first_action_id}"
if ! wait_for_session_completed "${session_id}"; then
  dump_failure_context
  echo "CASE tui_question_option_resume FAIL"
  exit 1
fi
if ! wait_for_overlay_closed; then
  dump_failure_context
  echo "CASE tui_question_option_close FAIL"
  exit 1
fi

other_prompt="You must call AskUserQuestion exactly once. Use header TUI Choice, question Choose delivery, and exactly these options: id now, label Now, description Deliver now; id later, label Later, description Deliver later. After receiving the answer, respond with exactly TUI_QUESTION_OTHER_OK"
tui_send_input "${TUI_SESSION_NAME}" "${other_prompt}"
if ! second_pending="$(wait_for_action_state "pending" "${first_action_id}")"; then
  dump_failure_context
  echo "CASE tui_question_other_waiting FAIL"
  exit 1
fi
second_action_id="$(json_get "${second_pending}" "action_id")"
tmux send-keys -t "${TUI_SESSION_NAME}" Down Down Enter >/dev/null
sleep 0.25
tmux send-keys -t "${TUI_SESSION_NAME}" -l "Use a staged rollout" >/dev/null
tmux send-keys -t "${TUI_SESSION_NAME}" Enter >/dev/null
if ! second_resolved="$(wait_for_action_state "resolved" "${first_action_id}")"; then
  dump_failure_context
  echo "CASE tui_question_other_submit FAIL"
  exit 1
fi
SECOND_RESOLVED="${second_resolved}" SECOND_ACTION_ID="${second_action_id}" python3 - <<'PY'
import json
import os

action = json.loads(os.environ["SECOND_RESOLVED"])
assert action["action_id"] == os.environ["SECOND_ACTION_ID"], action
assert action["response"] == {
    "type": "question",
    "response": {"type": "freeForm", "text": "Use a staged rollout"},
}, action
PY
if ! wait_for_session_completed "${session_id}"; then
  dump_failure_context
  echo "CASE tui_question_other_resume FAIL"
  exit 1
fi
echo "CASE tui_question_other PASS action_id=${second_action_id}"

approval_marker="${TEST_TALOS_HOME}/tui-denied-marker"
approval_prompt="You must call Bash exactly once with this exact command: printf tui-denied > '${approval_marker}'. Do not call any other tool. After the tool result, respond with exactly TUI_APPROVAL_DENY_OK"
tui_send_input "${TUI_SESSION_NAME}" "${approval_prompt}"
if ! approval_pending="$(wait_for_action_state "pending" "${second_action_id}")"; then
  dump_failure_context
  echo "CASE tui_approval_deny_waiting FAIL"
  exit 1
fi
approval_action_id="$(json_get "${approval_pending}" "action_id")"
APPROVAL_PENDING="${approval_pending}" python3 - <<'PY'
import json
import os

action = json.loads(os.environ["APPROVAL_PENDING"])
assert action["params"]["type"] == "sandboxApproval", action
assert action["params"]["params"]["availableDecisions"] == ["allowOnce", "deny", "cancel"], action
PY
if ! tui_wait_for_text "${TUI_SESSION_NAME}" "Permission required for Bash" 80 0.25 "${PANE_SNAPSHOT}"; then
  dump_failure_context
  echo "CASE tui_approval_deny_overlay FAIL"
  exit 1
fi
tmux send-keys -t "${TUI_SESSION_NAME}" Down Enter >/dev/null
if ! approval_resolved="$(wait_for_action_state "resolved" "${second_action_id}")"; then
  dump_failure_context
  echo "CASE tui_approval_deny_submit FAIL"
  exit 1
fi
APPROVAL_RESOLVED="${approval_resolved}" APPROVAL_ACTION_ID="${approval_action_id}" python3 - <<'PY'
import json
import os

action = json.loads(os.environ["APPROVAL_RESOLVED"])
assert action["action_id"] == os.environ["APPROVAL_ACTION_ID"], action
assert action["response"] == {
    "type": "sandboxApproval",
    "response": "deny",
}, action
PY
if ! wait_for_session_completed "${session_id}"; then
  dump_failure_context
  echo "CASE tui_approval_deny_resume FAIL"
  exit 1
fi
if [[ -e "${approval_marker}" ]]; then
  dump_failure_context
  echo "CASE tui_approval_deny_side_effect FAIL"
  exit 1
fi
echo "CASE tui_approval_deny PASS action_id=${approval_action_id}"
