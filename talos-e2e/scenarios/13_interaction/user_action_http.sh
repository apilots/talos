#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

require_command curl
require_command python3

if ! has_real_llm_key; then
  echo "CASE user_action_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE user_action_real_llm_precheck PASS key_detected=true"

build_binaries
start_server

APPROVAL_WORKSPACE="${TALOS_HOME}/workspace-talos/anonymous"
export APPROVAL_WORKSPACE
mkdir -p "${APPROVAL_WORKSPACE}"

cleanup() {
  stop_server
}
trap cleanup EXIT

get_active_turn_json() {
  api_data_json "$(curl -sS "${GATEWAY_URL}/v1/sessions/${session_id}/active-turn")"
}

get_session_history_json() {
  api_data_json "$(curl -sS "${GATEWAY_URL}/v1/sessions/${session_id}/history")"
}

submit_user_action_json() {
  local payload="$1"
  api_data_json "$(curl -sS -X POST "${GATEWAY_URL}/v1/turns/input" \
    -H "Content-Type: application/json" \
    -d "${payload}")"
}

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
primary_model="$(select_e2e_model_from_json "${models_json}")"
primary_provider="$(select_e2e_model_provider_from_json "${models_json}" "${primary_model}")"
if [[ -z "${primary_model}" || -z "${primary_provider}" ]]; then
  echo "CASE user_action_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
export PRIMARY_PROVIDER="${primary_provider}"
echo "CASE user_action_select_model PASS model=${primary_model}"

build_session_start_defaults() {
  python3 - <<'PY'
import json
import os

print(json.dumps({
    "modelSelection": {
        "modelRef": {
            "provider": os.environ["PRIMARY_PROVIDER"],
            "id": os.environ["PRIMARY_MODEL"],
        },
        "reasoningEffort": None,
    },
    "currentDirectory": os.environ["APPROVAL_WORKSPACE"],
}))
PY
}

build_question_payload() {
  local marker="$1"
  MARKER="${marker}" python3 - <<'PY'
import json
import os

text = (
    "Call AskUserQuestion exactly once. Use header Execution Mode, question Choose mode, "
    "and exactly these options: id plan, label Plan, description Explore before execution; "
    "id direct, label Direct, description Execute immediately. "
    f"After receiving the answer, respond with exactly {os.environ['MARKER']}"
)
print(json.dumps({"input": {"content": [{"type": "text", "text": text}]}}))
PY
}

build_approval_payload() {
  local command="$1"
  local marker="$2"
  COMMAND="${command}" MARKER="${marker}" python3 - <<'PY'
import json
import os

text = (
    "Call Bash exactly once with this exact command: "
    f"{os.environ['COMMAND']}. Do not call any other tool. "
    f"After the tool result, respond with exactly {os.environ['MARKER']}"
)
print(json.dumps({"input": {"content": [{"type": "text", "text": text}]}}))
PY
}

extract_output_text() {
  local response_json="$1"
  printf '%s' "${response_json}" | python3 -c '
import json
import sys

response = json.load(sys.stdin)
print("".join(
    item.get("text", "")
    for item in response.get("output", [])
    if item.get("type") == "output_text"
))
'
}

pending_action_from_events() {
  local events_path="$1"
  EVENTS_PATH="${events_path}" python3 - <<'PY'
import json
import os

pending = None
with open(os.environ["EVENTS_PATH"], encoding="utf-8") as handle:
    for line in handle:
        if not line.startswith("data: "):
            continue
        record = json.loads(line[6:])
        event = record.get("event") or {}
        payload = (event.get("data") or {}).get("payload") or {}
        data = payload.get("data") or {}
        if payload.get("type") == "waitingForInput" and data.get("type") == "userAction":
            pending = {"eventId": record.get("eventId"), "action": data.get("action")}
if pending is None:
    raise SystemExit("canonical waitingForInput UserAction event was not found")
print(json.dumps(pending, separators=(",", ":")))
PY
}

assert_active_waiting() {
  local turn_id="$1"
  local active_json
  active_json="$(get_active_turn_json)"
  ACTIVE_JSON="${active_json}" TURN_ID="${turn_id}" python3 - <<'PY'
import json
import os

active = json.loads(os.environ["ACTIVE_JSON"])["activeTurn"]
assert active["identity"]["turnId"] == os.environ["TURN_ID"], active
assert active["state"] == "waitingForInput", active
assert active.get("lastEventId"), active
PY
}

assert_terminal_turn() {
  local case_name="$1"
  local turn_id="$2"
  local expected_status="$3"
  local active_json history_json history_file
  active_json="$(get_active_turn_json)"
  history_json="$(get_session_history_json)"
  history_file="$(scenario_report_path "${case_name}_history.json")"
  printf '%s' "${history_json}" > "${history_file}"
  ACTIVE_JSON="${active_json}" HISTORY_JSON_FILE="${history_file}" TURN_ID="${turn_id}" EXPECTED_STATUS="${expected_status}" python3 - <<'PY'
import json
import os
from pathlib import Path

active = json.loads(os.environ["ACTIVE_JSON"])["activeTurn"]
assert active is None, active
history = json.loads(Path(os.environ["HISTORY_JSON_FILE"]).read_text())
turns = [
    turn for turn in history["data"]
    if turn["identity"]["turnId"] == os.environ["TURN_ID"]
]
assert len(turns) == 1, turns
result = turns[0]["result"]
assert result["status"] == os.environ["EXPECTED_STATUS"], result
messages = result["result"]["messages"]
assert messages, result
assert all(message["status"] in ("completed", "failed", "cancelled") for message in messages)
assert all(
    block["status"] in ("completed", "failed", "cancelled")
    for message in messages
    for block in message["blocks"]
)
if os.environ["EXPECTED_STATUS"] == "completed":
    assert result["result"]["completion"]["stopReason"] == "endTurn", result
else:
    assert result["result"]["failure"]["error"]["code"], result
PY
  echo "CASE ${case_name}_terminal_history PASS turn_id=${turn_id}"
}

session_id="$(new_session_id)"
remember_session_start_defaults "${session_id}" "$(build_session_start_defaults)"
echo "CASE user_action_session_id PASS session_id=${session_id}"

bootstrap_payload='{"input":{"content":[{"type":"text","text":"Reply with exactly INTERACTION_SESSION_READY"}]}}'
bootstrap_json="$(send_message_response "${session_id}" "${bootstrap_payload}" "interaction_session_bootstrap")"
if [[ "$(extract_output_text "${bootstrap_json}")" != "INTERACTION_SESSION_READY" ]]; then
  echo "CASE user_action_session_bootstrap FAIL response=${bootstrap_json}"
  exit 1
fi
echo "CASE user_action_session_bootstrap PASS"

for mode in askForApproval approveForMe fullAccess; do
  patch_json="$(curl -sS -X PATCH "${GATEWAY_URL}/v1/sessions/${session_id}" \
    -H "Content-Type: application/json" \
    -d "{\"conf\":{\"userPermissionMode\":\"${mode}\"}}")"
  patch_data_json="$(api_data_json "${patch_json}")"
  if [[ "$(json_get "${patch_data_json}" "session.effectiveMode")" != "${mode}" ]]; then
    echo "CASE session_permission_${mode} FAIL response=${patch_json}"
    exit 1
  fi
  echo "CASE session_permission_${mode} PASS"
done

run_question() {
  local case_name="$1"
  local answer_json="$2"
  local marker="$3"
  local payload ack_json turn_id waiting_events waiting_response pending action_id waiting_event_id
  local recovery_json recovery_file submit_payload submit_json resumed_events resumed_response

  payload="$(build_question_payload "${marker}")"
  ack_json="$(send_message_ack "${session_id}" "${payload}")"
  turn_id="$(api_json_get "${ack_json}" "turnId")"
  waiting_events="$(scenario_report_path "${case_name}_waiting_events.log")"
  collect_timeline_events "${session_id}" "${turn_id}" "${waiting_events}" "" "waitingForInput,turnFailed"
  waiting_response="$(message_response_from_events "${ack_json}" "${waiting_events}")"
  pending="$(pending_action_from_events "${waiting_events}")"
  printf '%s' "${waiting_response}" > "$(scenario_report_path "${case_name}_waiting.json")"
  PENDING="${pending}" RESPONSE_FILE="$(scenario_report_path "${case_name}_waiting.json")" python3 - <<'PY'
import json
import os
from pathlib import Path

response = json.loads(Path(os.environ["RESPONSE_FILE"]).read_text())
assert response["stop_reason"] == "waiting_user_action", response
action = json.loads(os.environ["PENDING"])["action"]
assert action["state"] == "pending", action
params = action["params"]
assert params["type"] == "question", params
question = params["params"]
assert question["header"] == "Execution Mode", question
assert [option["id"] for option in question["options"]] == ["plan", "direct"], question
PY
  assert_active_waiting "${turn_id}"

  recovery_json="$(api_data_json "$(curl -sS "${GATEWAY_URL}/v1/sessions/${session_id}/timeline/recovery")")"
  recovery_file="$(scenario_report_path "${case_name}_recovery.json")"
  printf '%s' "${recovery_json}" > "${recovery_file}"
  RECOVERY_FILE="${recovery_file}" TURN_ID="${turn_id}" python3 - <<'PY'
import json
import os
from pathlib import Path

recovery = json.loads(Path(os.environ["RECOVERY_FILE"]).read_text())
active = recovery["activeTurn"]
assert active["identity"]["turnId"] == os.environ["TURN_ID"], active
assert active["state"] == "waitingForInput", active
assert any(
    item["event"]["data"]["payload"]["type"] == "waitingForInput"
    for item in recovery["replay"]["events"]
), recovery
PY

  action_id="$(json_get "${pending}" "action.actionId")"
  waiting_event_id="$(json_get "${pending}" "eventId")"
  submit_payload="{\"identity\":{\"sessionId\":\"${session_id}\",\"turnId\":\"${turn_id}\"},\"input\":{\"type\":\"userActionResponse\",\"actionId\":\"${action_id}\",\"response\":{\"type\":\"question\",\"response\":${answer_json}}},\"clientRequestId\":null}"
  submit_json="$(submit_user_action_json "${submit_payload}")"
  if [[ "$(json_get "${submit_json}" "identity.turnId")" != "${turn_id}" || -z "$(json_get "${submit_json}" "messageId")" ]]; then
    echo "CASE ${case_name}_submit FAIL response=${submit_json}"
    exit 1
  fi

  resumed_events="$(scenario_report_path "${case_name}_resumed_events.log")"
  collect_timeline_events "${session_id}" "${turn_id}" "${resumed_events}" "${waiting_event_id}"
  resumed_response="$(message_response_from_events "${ack_json}" "${resumed_events}")"
  if [[ "$(extract_output_text "${resumed_response}")" != "${marker}" ]]; then
    echo "CASE ${case_name}_resume FAIL response=${resumed_response}"
    exit 1
  fi
  assert_terminal_turn "${case_name}" "${turn_id}" "completed"
  echo "CASE ${case_name} PASS action_id=${action_id}"
}

run_question "user_action_question_option" '{"type":"selectedOption","optionId":"plan"}' "QUESTION_OPTION_OK"
run_question "user_action_question_other" '{"type":"freeForm","text":"Use a staged rollout"}' "QUESTION_OTHER_OK"

approval_mode_json="$(curl -sS -X PATCH "${GATEWAY_URL}/v1/sessions/${session_id}" \
  -H "Content-Type: application/json" \
  -d '{"conf":{"userPermissionMode":"askForApproval"}}')"
if [[ "$(json_get "$(api_data_json "${approval_mode_json}")" "session.effectiveMode")" != "askForApproval" ]]; then
  echo "CASE sandbox_approval_mode FAIL response=${approval_mode_json}"
  exit 1
fi
echo "CASE sandbox_approval_mode PASS"

run_sandbox_approval() {
  local case_name="$1"
  local decision="$2"
  local marker="$3"
  local side_effect="${APPROVAL_WORKSPACE}/${case_name}-marker"
  local command payload ack_json turn_id waiting_events waiting_response pending action_id waiting_event_id
  local submit_payload submit_json resumed_events resumed_response expected_terminal

  rm -f "${side_effect}"
  command="test ! -s '${TALOS_HOME}/.env' && printf '${case_name}' > '${side_effect}'"
  payload="$(build_approval_payload "${command}" "${marker}")"
  ack_json="$(send_message_ack "${session_id}" "${payload}")"
  turn_id="$(api_json_get "${ack_json}" "turnId")"
  waiting_events="$(scenario_report_path "${case_name}_waiting_events.log")"
  collect_timeline_events "${session_id}" "${turn_id}" "${waiting_events}" "" "waitingForInput,turnFailed"
  waiting_response="$(message_response_from_events "${ack_json}" "${waiting_events}")"
  pending="$(pending_action_from_events "${waiting_events}")"
  printf '%s' "${waiting_response}" > "$(scenario_report_path "${case_name}_waiting.json")"
  PENDING="${pending}" RESPONSE_FILE="$(scenario_report_path "${case_name}_waiting.json")" COMMAND="${command}" python3 - <<'PY'
import json
import os
from pathlib import Path

response = json.loads(Path(os.environ["RESPONSE_FILE"]).read_text())
assert response["stop_reason"] == "waiting_user_action", response
action = json.loads(os.environ["PENDING"])["action"]
params = action["params"]
assert params["type"] == "sandboxApproval", params
approval = params["params"]
assert approval["toolName"] == "Bash", approval
assert approval["availableDecisions"] == ["allowOnce", "deny", "cancel"], approval
presentation = approval["presentation"]
assert presentation["type"] == "command", presentation
assert presentation["command"] == os.environ["COMMAND"], presentation
PY
  assert_active_waiting "${turn_id}"
  if [[ -e "${side_effect}" ]]; then
    echo "CASE ${case_name}_waiting FAIL side_effect_ran_before_approval"
    exit 1
  fi

  action_id="$(json_get "${pending}" "action.actionId")"
  waiting_event_id="$(json_get "${pending}" "eventId")"
  submit_payload="{\"identity\":{\"sessionId\":\"${session_id}\",\"turnId\":\"${turn_id}\"},\"input\":{\"type\":\"userActionResponse\",\"actionId\":\"${action_id}\",\"response\":{\"type\":\"sandboxApproval\",\"response\":\"${decision}\"}},\"clientRequestId\":null}"
  submit_json="$(submit_user_action_json "${submit_payload}")"
  expected_terminal="completed"
  if [[ "$(json_get "${submit_json}" "identity.turnId")" != "${turn_id}" || -z "$(json_get "${submit_json}" "messageId")" ]]; then
    echo "CASE ${case_name}_submit FAIL response=${submit_json}"
    exit 1
  fi

  resumed_events="$(scenario_report_path "${case_name}_resumed_events.log")"
  collect_timeline_events "${session_id}" "${turn_id}" "${resumed_events}" "${waiting_event_id}"
  resumed_response="$(message_response_from_events "${ack_json}" "${resumed_events}")"
  if [[ "$(extract_output_text "${resumed_response}")" != "${marker}" ]]; then
    echo "CASE ${case_name}_resume FAIL response=${resumed_response}"
    exit 1
  fi
  if [[ "${decision}" == "allowOnce" ]]; then
    if [[ "$(cat "${side_effect}")" != "${case_name}" ]]; then
      echo "CASE ${case_name} FAIL side_effect_not_exactly_once"
      exit 1
    fi
  elif [[ -e "${side_effect}" ]]; then
    echo "CASE ${case_name} FAIL rejected_side_effect_exists"
    exit 1
  fi
  assert_terminal_turn "${case_name}" "${turn_id}" "${expected_terminal}"
  echo "CASE ${case_name} PASS action_id=${action_id}"
}

run_sandbox_approval "sandbox_approval_allow_once" "allowOnce" "SANDBOX_ALLOW_ONCE_OK"
run_sandbox_approval "sandbox_approval_deny" "deny" "SANDBOX_DENY_OK"
run_sandbox_approval "sandbox_approval_cancel" "cancel" "SANDBOX_CANCEL_UNUSED"
