#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

require_command curl
require_command python3
require_command ps

build_binaries
start_server

collector_pid=""
cleanup() {
  if [[ -n "${collector_pid}" ]] && kill -0 "${collector_pid}" >/dev/null 2>&1; then
    kill "${collector_pid}" >/dev/null 2>&1 || true
    wait "${collector_pid}" 2>/dev/null || true
  fi
  stop_server
}
trap cleanup EXIT

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
primary_model="$(select_e2e_model_from_json "${models_json}")"
primary_provider="$(select_e2e_model_provider_from_json "${models_json}" "${primary_model}")"
if [[ -z "${primary_model}" || -z "${primary_provider}" ]]; then
  echo "CASE turn_cancel_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
export PRIMARY_PROVIDER="${primary_provider}"

build_session_context() {
  python3 - <<'PY'
import json
import os

payload = {
    "context": {
        "source_type": "http",
        "provider": "http",
        "channel_id": "turn-cancellation-e2e",
        "user_id": "turn-cancellation-e2e-user",
        "thread_id": None,
        "message_id": "turn-cancellation-e2e-message",
        "trace_id": "turn-cancellation-e2e-trace",
    },
    "modelSelection": {
        "modelRef": {
            "provider": os.environ["PRIMARY_PROVIDER"],
            "id": os.environ["PRIMARY_MODEL"],
        },
        "reasoningEffort": None,
    },
}
print(json.dumps(payload, ensure_ascii=False))
PY
}

build_turn_payload() {
  python3 - <<'PY'
import json

text = (
    "You must call Bash exactly once with this exact command: "
    "sleep 30. "
    "Do not call any other tool and do not wait before calling Bash. "
    "After the tool result, respond with exactly TURN_CANCEL_UNEXPECTED_COMPLETION."
)
payload = {"input": [{"type": "text", "text": text}]}
print(json.dumps(payload, ensure_ascii=False))
PY
}

active_turn_json() {
  local response
  response="$(curl -sS "${GATEWAY_URL}/v1/sessions/${session_id}/active-turn")"
  api_data_json "${response}"
}

matching_sleep_processes() {
  SERVER_PID="${TALOS_SERVER_PID}" python3 - <<'PY'
import os
import subprocess

server_pid = int(os.environ["SERVER_PID"])
rows = []
for line in subprocess.check_output(
    ["ps", "-axo", "pid=,ppid=,command="], text=True
).splitlines():
    parts = line.strip().split(None, 2)
    if len(parts) < 3:
        continue
    rows.append((int(parts[0]), int(parts[1]), parts[2]))

descendants = {server_pid}
changed = True
while changed:
    changed = False
    for pid, parent_pid, _ in rows:
        if parent_pid in descendants and pid not in descendants:
            descendants.add(pid)
            changed = True

matches = [
    str(pid)
    for pid, _, command in rows
    if pid in descendants and command == "sleep 30"
]
print(" ".join(matches))
PY
}

wait_for_bash_process() {
  local attempt=0
  local matches=""
  while (( attempt < 480 )); do
    matches="$(matching_sleep_processes)"
    if [[ -n "${matches}" ]]; then
      printf '%s\n' "${matches}"
      return 0
    fi
    if ! kill -0 "${collector_pid}" >/dev/null 2>&1; then
      wait "${collector_pid}" || true
      collector_pid=""
      return 1
    fi
    attempt=$((attempt + 1))
    sleep 0.25
  done
  return 1
}

wait_for_process_cleanup() {
  local captured_pids="$1"
  local attempt=0
  local remaining=""
  while (( attempt < 100 )); do
    remaining=""
    for pid in ${captured_pids}; do
      if kill -0 "${pid}" >/dev/null 2>&1; then
        remaining="${remaining} ${pid}"
      fi
    done
    remaining="${remaining# }"
    if [[ -z "${remaining}" ]]; then
      return 0
    fi
    attempt=$((attempt + 1))
    sleep 0.1
  done
  echo "Captured Bash processes still running after cancellation: ${remaining}" >&2
  return 1
}

session_id="$(new_session_id)"
remember_session_context "${session_id}" "$(build_session_context)"
turn_payload="$(build_turn_payload)"
ack_json="$(send_message_ack "${session_id}" "${turn_payload}")"
turn_id="$(api_json_get "${ack_json}" "turn_id")"
if [[ -z "${turn_id}" ]]; then
  echo "CASE turn_cancel_start FAIL response=${ack_json}"
  exit 1
fi

events_log="$(scenario_report_path turn_cancel_events.log)"
collect_timeline_events "${session_id}" "${turn_id}" "${events_log}" &
collector_pid=$!

if ! bash_pids="$(wait_for_bash_process)"; then
  echo "CASE turn_cancel_bash_started FAIL turn_id=${turn_id} events=${events_log}"
  exit 1
fi

running_json="$(active_turn_json)"
database_path="${TALOS_HOME}/data/talos.db"
ACTIVE_JSON="${running_json}" DATABASE_PATH="${database_path}" SESSION_ID="${session_id}" TURN_ID="${turn_id}" python3 - <<'PY'
import json
import os
import sqlite3

active = json.loads(os.environ["ACTIVE_JSON"])["active_turn"]
assert active["turn_id"] == os.environ["TURN_ID"], active
assert active["status"] == "running", active

with sqlite3.connect(os.environ["DATABASE_PATH"]) as connection:
    row = connection.execute(
        "SELECT turn_id, status FROM active_turns WHERE session_id = ?",
        (os.environ["SESSION_ID"],),
    ).fetchone()
assert row == (os.environ["TURN_ID"], "running"), row
PY

cancel_payload="$(SESSION_ID="${session_id}" TURN_ID="${turn_id}" python3 - <<'PY'
import json
import os

print(json.dumps({"session_id": os.environ["SESSION_ID"], "turn_id": os.environ["TURN_ID"]}))
PY
)"
cancel_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/turns/interrupt" \
  -H "Content-Type: application/json" \
  -d "${cancel_payload}")"
if [[ "$(api_json_get "${cancel_json}" "success")" != "True" ]]; then
  echo "CASE turn_cancel_request FAIL response=${cancel_json}"
  exit 1
fi

wait "${collector_pid}"
collector_pid=""
wait_for_process_cleanup "${bash_pids}"

completed_json="$(active_turn_json)"
ACTIVE_JSON="${completed_json}" EVENTS_PATH="${events_log}" DATABASE_PATH="${database_path}" SESSION_ID="${session_id}" TURN_ID="${turn_id}" python3 - <<'PY'
import json
import os
import sqlite3
from pathlib import Path

turn_id = os.environ["TURN_ID"]
active = json.loads(os.environ["ACTIVE_JSON"])["active_turn"]
assert active["turn_id"] == turn_id, active
assert active["status"] == "completed", active

stream_events = []
for line in Path(os.environ["EVENTS_PATH"]).read_text().splitlines():
    if not line.startswith("data: "):
        continue
    record = json.loads(line[6:])
    stream_events.append(record.get("event", record))

tool_item_ids = {
    event["payload"]["item"]["item_id"]
    for event in stream_events
    if event.get("event_type") == "item_started"
    and event.get("payload", {}).get("item", {}).get("kind") == "tool_call"
    and event.get("payload", {}).get("item", {}).get("content", {}).get("name") == "Bash"
}
assert len(tool_item_ids) == 1, tool_item_ids
cancelled_indexes = [
    index
    for index, event in enumerate(stream_events)
    if event.get("event_type") == "item_completed"
    and event.get("payload", {}).get("item_id") in tool_item_ids
    and event.get("payload", {}).get("status") == "cancelled"
]
completed_indexes = [
    index
    for index, event in enumerate(stream_events)
    if event.get("event_type") == "turn_completed"
    and event.get("payload", {}).get("stop_reason") == "cancelled"
]
assert len(cancelled_indexes) == 1, cancelled_indexes
assert len(completed_indexes) == 1, completed_indexes
assert cancelled_indexes[0] < completed_indexes[0], (cancelled_indexes, completed_indexes)

with sqlite3.connect(os.environ["DATABASE_PATH"]) as connection:
    active_row = connection.execute(
        "SELECT turn_id, status FROM active_turns WHERE session_id = ?",
        (os.environ["SESSION_ID"],),
    ).fetchone()

assert active_row == (turn_id, "completed"), active_row
PY

SERVER_LOG="${SERVER_LOG}" SESSION_ID="${session_id}" TURN_ID="${turn_id}" python3 - <<'PY'
import os
import re
from pathlib import Path

lines = Path(os.environ["SERVER_LOG"]).read_text().splitlines()
session_id = os.environ["SESSION_ID"]
turn_id = os.environ["TURN_ID"]
relevant = [line for line in lines if session_id in line and turn_id in line]

registered_task_ids = {
    match.group(1)
    for line in relevant
    if "run_task_registered" in line
    if (match := re.search(r"task_id=(\d+)", line))
}
cleaned_task_ids = {
    match.group(1)
    for line in relevant
    if "run_task_cleanup_completed" in line
    if (match := re.search(r"task_id=(\d+)", line))
}

assert registered_task_ids, relevant
assert cleaned_task_ids == registered_task_ids, (registered_task_ids, cleaned_task_ids)
assert any(
    "run_cancellation_requested" in line and "active_tasks=" in line and "grace_ms=" in line
    for line in relevant
), relevant
assert any(
    "run_settlement_started" in line and "termination=Cancelled" in line
    for line in relevant
), relevant
assert any(
    "run_task_cleanup_completed" in line
    and "cleanup_outcome=Confirmed" in line
    for line in relevant
), relevant
assert any(
    "run_settled" in line
    and "settlement_kind=Cancelled(Cooperative)" in line
    and "cleanup_outcome=Confirmed" in line
    for line in relevant
), relevant
PY

echo "CASE turn_cancel_running_bash PASS turn_id=${turn_id} bash_pids=${bash_pids}"
echo "CASE turn_cancel_terminal_state PASS status=completed stop_reason=cancelled"
echo "CASE turn_cancel_cleanup PASS process=gone mode=Cooperative outcome=Confirmed"
