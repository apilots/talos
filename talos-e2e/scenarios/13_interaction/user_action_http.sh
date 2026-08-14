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

APPROVAL_WORKSPACE="$(make_e2e_temp_dir "user-action-approval-workspace")"
export APPROVAL_WORKSPACE
MODE_SMOKE_FILE_1="${APPROVAL_WORKSPACE}/mode-smoke-1.txt"
MODE_SMOKE_FILE_2="${APPROVAL_WORKSPACE}/mode-smoke-2.txt"
MODE_SMOKE_FILE_3="${APPROVAL_WORKSPACE}/mode-smoke-3.txt"
MULTI_APPROVAL_FILE="${APPROVAL_WORKSPACE}/multi-approval-marker"
export MODE_SMOKE_FILE_1 MODE_SMOKE_FILE_2 MODE_SMOKE_FILE_3 MULTI_APPROVAL_FILE
printf 'MODE_SMOKE_CONTENT_1\n' >"${MODE_SMOKE_FILE_1}"
printf 'MODE_SMOKE_CONTENT_2\n' >"${MODE_SMOKE_FILE_2}"
printf 'MODE_SMOKE_CONTENT_3\n' >"${MODE_SMOKE_FILE_3}"
build_binaries
start_server
trap stop_server EXIT

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

build_session_context() {
  python3 - <<'PY'
import json
import os

payload = {
    "context": {
        "source_type": "http",
        "provider": "http",
        "channel_id": "interaction-user-action",
        "user_id": "interaction-user-action-user",
        "thread_id": None,
        "message_id": "msg-interaction-user-action",
        "trace_id": "trace-interaction-user-action",
        "environment": {
            "directory": os.environ["APPROVAL_WORKSPACE"],
            "connection_mode": "embedded",
        },
    },
    "modelSelection": {
        "modelRef": {
            "provider": os.environ["PRIMARY_PROVIDER"],
            "id": os.environ["PRIMARY_MODEL"],
        },
        "reasoningEffort": None,
    },
    "current_directory": os.environ["APPROVAL_WORKSPACE"],
}
print(json.dumps(payload, ensure_ascii=False))
PY
}

build_turn_payload() {
  local marker="$1"
  MARKER="${marker}" python3 - <<'PY'
import json
import os

marker = os.environ["MARKER"]
text = (
    "You must call AskUserQuestion exactly once. Use header Execution Mode, "
    "question Choose mode, and exactly these options: "
    "id plan, label Plan, description Explore before execution; "
    "id direct, label Direct, description Execute immediately. "
    f"After receiving the answer, respond with exactly {marker}"
)
payload = {"input": [{"type": "text", "text": text}], "workspace_id": None}
print(json.dumps(payload, ensure_ascii=False))
PY
}

build_approval_payload() {
  local command="$1"
  local marker="$2"
  COMMAND="${command}" MARKER="${marker}" python3 - <<'PY'
import json
import os

text = (
    "You must call Bash exactly once with this exact command: "
    f"{os.environ['COMMAND']}. Do not call any other tool. "
    f"After the tool result, respond with exactly {os.environ['MARKER']}"
)
payload = {"input": [{"type": "text", "text": text}], "workspace_id": None}
print(json.dumps(payload, ensure_ascii=False))
PY
}

build_multi_approval_payload() {
  local marker="$1"
  MARKER="${marker}" python3 - <<'PY'
import json
import os

path = os.environ["MULTI_APPROVAL_FILE"]
commands = [
    f"printf 'MULTI_1\\n' >> '{path}'",
    f"printf 'MULTI_2\\n' >> '{path}'",
    f"printf 'MULTI_3\\n' >> '{path}'",
]
text = (
    "You must make exactly three independent Bash tool calls, one for each exact command. "
    "The order does not matter: "
    + "; ".join(commands)
    + ". Do not call any other tool. After all three tool results, respond with exactly "
    + os.environ["MARKER"]
)
payload = {"input": [{"type": "text", "text": text}], "workspace_id": None}
print(json.dumps(payload, ensure_ascii=False))
PY
}

build_mode_smoke_payload() {
  local marker="$1"
  MARKER="${marker}" python3 - <<'PY'
import json
import os

text = (
    "You must make exactly three independent Read tool calls, one for each exact path "
    "in this order: "
    f"{os.environ['MODE_SMOKE_FILE_1']}, "
    f"{os.environ['MODE_SMOKE_FILE_2']}, "
    f"{os.environ['MODE_SMOKE_FILE_3']}. Do not call any other tool. "
    f"After the tool result, respond with exactly {os.environ['MARKER']}"
)
payload = {"input": [{"type": "text", "text": text}], "workspace_id": None}
print(json.dumps(payload, ensure_ascii=False))
PY
}

extract_output_text() {
  local response_json="$1"
  RESPONSE_JSON="${response_json}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["RESPONSE_JSON"])
for item in data.get("output", []):
    if item.get("type") == "output_text":
        print(item.get("text", ""))
        break
PY
}

assert_terminal_turn() {
  local case_name="$1"
  local turn_id="$2"
  local message_id="$3"
  local active_json history_json
  active_json="$(curl -sS "${GATEWAY_URL}/v1/sessions/${session_id}/active-turn")"
  history_json="$(curl -sS "${GATEWAY_URL}/v1/sessions/${session_id}/history")"
  ACTIVE_JSON="${active_json}" HISTORY_JSON="${history_json}" TURN_ID="${turn_id}" MESSAGE_ID="${message_id}" python3 - <<'PY'
import json
import os

active = json.loads(os.environ["ACTIVE_JSON"])["active_turn"]
assert active["turn_id"] == os.environ["TURN_ID"], active
assert active["status"] == "completed", active
history = json.loads(os.environ["HISTORY_JSON"])["history"]
messages = [
    message
    for message in history["messages"]
    if message["message_id"] == os.environ["MESSAGE_ID"]
]
assert len(messages) == 1, messages
message = messages[0]
assert message["turn_id"] == os.environ["TURN_ID"], message
assert message["stop_reason"] == "end_turn", message
assert all(
    item["status"] in ("completed", "failed", "cancelled")
    for item in message["items"]
), message
PY
  echo "CASE ${case_name}_terminal_history PASS turn_id=${turn_id}"
}

session_id="$(new_session_id)"
remember_session_context "${session_id}" "$(build_session_context)"
echo "CASE user_action_session_id PASS session_id=${session_id}"

bootstrap_payload='{"input":[{"type":"text","text":"Reply with exactly INTERACTION_SESSION_READY"}],"workspace_id":null}'
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
  actual_mode="$(json_get "${patch_json}" "session.effectiveMode")"
  if [[ "${actual_mode}" != "${mode}" ]]; then
    echo "CASE session_permission_${mode} FAIL response=${patch_json}"
    exit 1
  fi
  echo "CASE session_permission_${mode} PASS"

  mode_marker="MODE_${mode}_OK"
  mode_payload="$(build_mode_smoke_payload "${mode_marker}")"
  mode_response="$(send_message_response "${session_id}" "${mode_payload}" "session_permission_${mode}_turn")"
  MODE_RESPONSE="${mode_response}" MODE_MARKER="${mode_marker}" python3 - <<'PY'
import json
import os

response = json.loads(os.environ["MODE_RESPONSE"])
tool_calls = [item for item in response.get("output", []) if item.get("type") == "tool_call"]
output_text = [item.get("text", "") for item in response.get("output", []) if item.get("type") == "output_text"]
assert response.get("stop_reason") == "end_turn", response
assert len(tool_calls) == 3, tool_calls
assert [call.get("name") for call in tool_calls] == ["Read", "Read", "Read"], tool_calls
metrics = response.get("metrics") or {}
assert metrics.get("tool_calls") == 3, metrics
assert metrics.get("tool_successes") == 3, metrics
assert output_text == [os.environ["MODE_MARKER"]], output_text
PY
  echo "CASE session_permission_${mode}_turn PASS"
done

approve_for_me_json="$(curl -sS -X PATCH "${GATEWAY_URL}/v1/sessions/${session_id}" \
  -H "Content-Type: application/json" \
  -d '{"conf":{"userPermissionMode":"approveForMe"}}')"
if [[ "$(json_get "${approve_for_me_json}" "session.effectiveMode")" != "approveForMe" ]]; then
  echo "CASE approve_for_me_real_risk_mode FAIL response=${approve_for_me_json}"
  exit 1
fi

safe_command="printf APPROVE_FOR_ME_MODEL_SAFE"
safe_payload="$(build_approval_payload "${safe_command}" "APPROVE_FOR_ME_MODEL_SAFE_OK")"
safe_response="$(send_message_response "${session_id}" "${safe_payload}" "approve_for_me_real_safe")"
SAFE_RESPONSE="${safe_response}" SAFE_COMMAND="${safe_command}" python3 - <<'PY'
import json
import os

response = json.loads(os.environ["SAFE_RESPONSE"])
tool_calls = [item for item in response.get("output", []) if item.get("type") == "tool_call"]
output_text = [item.get("text", "") for item in response.get("output", []) if item.get("type") == "output_text"]
assert response.get("stop_reason") == "end_turn", response
assert len(tool_calls) == 1, tool_calls
assert tool_calls[0].get("name") == "Bash", tool_calls[0]
assert tool_calls[0].get("arguments", {}).get("command") == os.environ["SAFE_COMMAND"], tool_calls[0]
assert output_text == ["APPROVE_FOR_ME_MODEL_SAFE_OK"], output_text
metrics = response.get("metrics") or {}
assert metrics.get("tool_calls") == 1, metrics
assert metrics.get("tool_successes") == 1, metrics
PY
echo "CASE approve_for_me_real_safe PASS"

RISK_TARGET="${APPROVAL_WORKSPACE}/approve-for-me-risk-target"
mkdir -p "${RISK_TARGET}"
printf 'must-remain\n' >"${RISK_TARGET}/sentinel"
risky_command="python3 -c \"import shutil; shutil.rmtree('${RISK_TARGET}')\""
risky_payload="$(build_approval_payload "${risky_command}" "APPROVE_FOR_ME_MODEL_RISKY_OK")"
risky_ack_json="$(send_message_ack "${session_id}" "${risky_payload}")"
risky_turn_id="$(json_get "${risky_ack_json}" "turn_id")"
risky_message_id="$(json_get "${risky_ack_json}" "message_id")"
risky_events="$(scenario_report_path "approve_for_me_real_risky_initial_events.log")"
collect_timeline_events "${session_id}" "${risky_turn_id}" "${risky_events}"
risky_response="$(message_response_from_events "${risky_ack_json}" "${risky_events}")"
risky_active_json="$(curl -sS "${GATEWAY_URL}/v1/sessions/${session_id}/active-turn")"
RISKY_ACTIVE_JSON="${risky_active_json}" RISKY_RESPONSE="${risky_response}" RISKY_TURN_ID="${risky_turn_id}" RISKY_COMMAND="${risky_command}" python3 - <<'PY'
import json
import os

response = json.loads(os.environ["RISKY_RESPONSE"])
tool_calls = [item for item in response.get("output", []) if item.get("type") == "tool_call"]
assert response.get("stop_reason") == "waiting_user_action", response
assert len(tool_calls) == 1 and tool_calls[0].get("name") == "Bash", tool_calls
active = json.loads(os.environ["RISKY_ACTIVE_JSON"])["active_turn"]
assert active["turn_id"] == os.environ["RISKY_TURN_ID"], active
assert active["status"] == "waiting_user_action", active
action = active["pending_user_action"]
assert action["state"] == "pending", action
params = action["params"]
assert params["type"] == "sandboxApproval", params
approval = params["params"]
assert approval["reason"] == "riskyCommand", approval
presentation = approval["presentation"]
assert presentation["type"] == "command", presentation
assert presentation["command"] == os.environ["RISKY_COMMAND"], presentation
summary = presentation.get("riskSummary", "").strip()
assert summary, presentation
assert "timed out" not in summary.lower(), summary
assert "unavailable" not in summary.lower(), summary
assert "invalid command risk assessment" not in summary.lower(), summary
PY
if [[ ! -f "${RISK_TARGET}/sentinel" ]]; then
  echo "CASE approve_for_me_real_risky_waiting FAIL destructive_side_effect_exists"
  exit 1
fi
risky_action_id="$(json_get "${risky_active_json}" "active_turn.pending_user_action.actionId")"
risky_last_event_id="$(json_get "${risky_active_json}" "active_turn.last_event_id")"
risky_submit_payload="{\"sessionId\":\"${session_id}\",\"turnId\":\"${risky_turn_id}\",\"actionId\":\"${risky_action_id}\",\"response\":{\"type\":\"sandboxApproval\",\"response\":\"deny\"}}"
risky_submit_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/user-actions/submit" \
  -H "Content-Type: application/json" \
  -d "${risky_submit_payload}")"
if [[ "$(json_get "${risky_submit_json}" "state")" != "resuming" ]]; then
  echo "CASE approve_for_me_real_risky_submit FAIL response=${risky_submit_json}"
  exit 1
fi
risky_resumed_events="$(scenario_report_path "approve_for_me_real_risky_resumed_events.log")"
collect_timeline_events "${session_id}" "${risky_turn_id}" "${risky_resumed_events}" "${risky_last_event_id}"
risky_resumed_response="$(message_response_from_events "${risky_ack_json}" "${risky_resumed_events}")"
if [[ "$(extract_output_text "${risky_resumed_response}")" != "APPROVE_FOR_ME_MODEL_RISKY_OK" || ! -f "${RISK_TARGET}/sentinel" ]]; then
  echo "CASE approve_for_me_real_risky_resume FAIL response=${risky_resumed_response}"
  exit 1
fi
assert_terminal_turn "approve_for_me_real_risky" "${risky_turn_id}" "${risky_message_id}"
echo "CASE approve_for_me_real_risky PASS action_id=${risky_action_id}"

run_question() {
  local case_name="$1"
  local answer_type="$2"
  local marker="$3"
  local payload ack_json turn_id message_id initial_events initial_response
  local active_json recovery_json action_id option_id last_event_id answer_json submit_payload submit_json
  local resumed_events resumed_response final_output

  payload="$(build_turn_payload "${marker}")"
  ack_json="$(send_message_ack "${session_id}" "${payload}")"
  turn_id="$(json_get "${ack_json}" "turn_id")"
  message_id="$(json_get "${ack_json}" "message_id")"
  if [[ -z "${turn_id}" || -z "${message_id}" ]]; then
    echo "CASE ${case_name}_start FAIL response=${ack_json}"
    exit 1
  fi

  initial_events="$(scenario_report_path "${case_name}_initial_events.log")"
  collect_timeline_events "${session_id}" "${turn_id}" "${initial_events}"
  initial_response="$(message_response_from_events "${ack_json}" "${initial_events}")"
  log_case_json "${case_name}_waiting" "${payload}" "${initial_response}"

  active_json="$(curl -sS "${GATEWAY_URL}/v1/sessions/${session_id}/active-turn")"
  ACTIVE_JSON="${active_json}" TURN_ID="${turn_id}" python3 - <<'PY'
import json
import os

active = json.loads(os.environ["ACTIVE_JSON"])["active_turn"]
assert active["turn_id"] == os.environ["TURN_ID"], active
assert active["status"] == "waiting_user_action", active
action = active["pending_user_action"]
assert action["state"] == "pending", action
params = action["params"]
assert params["type"] == "question", params
question = params["params"]
assert question["header"] == "Execution Mode", question
assert len(question["options"]) == 2, question
assert [option["id"] for option in question["options"]] == ["plan", "direct"], question
PY

  recovery_json="$(curl -sS "${GATEWAY_URL}/v1/sessions/${session_id}/timeline/recovery")"
  RECOVERY_JSON="${recovery_json}" TURN_ID="${turn_id}" python3 - <<'PY'
import json
import os

snapshot = json.loads(os.environ["RECOVERY_JSON"])["snapshot"]
active = snapshot["active_turn"]
assert active["turn_id"] == os.environ["TURN_ID"], active
assert active["status"] == "waiting_user_action", active
action = active["pending_user_action"]
assert action["state"] == "pending", action
assert action["params"]["type"] == "question", action
PY

  action_id="$(json_get "${active_json}" "active_turn.pending_user_action.actionId")"
  option_id="plan"
  last_event_id="$(json_get "${active_json}" "active_turn.last_event_id")"
  if [[ "${answer_type}" == "selectedOption" ]]; then
    answer_json="{\"type\":\"selectedOption\",\"optionId\":\"${option_id}\"}"
  else
    answer_json='{"type":"freeForm","text":"Use a staged rollout"}'
  fi
  submit_payload="{\"sessionId\":\"${session_id}\",\"turnId\":\"${turn_id}\",\"actionId\":\"${action_id}\",\"response\":{\"type\":\"question\",\"response\":${answer_json}}}"
  submit_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/user-actions/submit" \
    -H "Content-Type: application/json" \
    -d "${submit_payload}")"
  if [[ "$(json_get "${submit_json}" "actionId")" != "${action_id}" || "$(json_get "${submit_json}" "state")" != "resuming" ]]; then
    echo "CASE ${case_name}_submit FAIL response=${submit_json}"
    exit 1
  fi

  resumed_events="$(scenario_report_path "${case_name}_resumed_events.log")"
  collect_timeline_events "${session_id}" "${turn_id}" "${resumed_events}" "${last_event_id}"
  resumed_response="$(message_response_from_events "${ack_json}" "${resumed_events}")"
  log_case_json "${case_name}_resumed" "${submit_payload}" "${resumed_response}"
  final_output="$(extract_output_text "${resumed_response}")"
  if [[ "${final_output}" != "${marker}" || "$(json_get "${resumed_response}" "stop_reason")" == "error" ]]; then
    echo "CASE ${case_name}_resume FAIL response=${resumed_response}"
    exit 1
  fi
  assert_terminal_turn "${case_name}" "${turn_id}" "${message_id}"
  echo "CASE ${case_name} PASS action_id=${action_id}"
  echo "CASE ${case_name}_recovery PASS action_id=${action_id}"
}

run_question "user_action_question_option" "selectedOption" "QUESTION_OPTION_OK"
run_question "user_action_question_other" "freeForm" "QUESTION_OTHER_OK"

approval_mode_json="$(curl -sS -X PATCH "${GATEWAY_URL}/v1/sessions/${session_id}" \
  -H "Content-Type: application/json" \
  -d '{"conf":{"userPermissionMode":"askForApproval"}}')"
if [[ "$(json_get "${approval_mode_json}" "session.effectiveMode")" != "askForApproval" ]]; then
  echo "CASE sandbox_approval_mode FAIL response=${approval_mode_json}"
  exit 1
fi
echo "CASE sandbox_approval_mode PASS"

run_sandbox_approval() {
  local case_name="$1"
  local decision="$2"
  local marker="$3"
  local side_effect="$4"
  local submission_mode="${5:-single}"
  local command payload ack_json turn_id message_id initial_events initial_response
  local active_json action_id call_id last_event_id submit_payload submit_json expected_state
  local submit_one_path submit_two_path submit_one submit_two submit_one_pid submit_two_pid
  local resumed_events resumed_response final_output terminal_json history_json

  rm -f "${side_effect}"
  command="printf '${case_name}' >> '${side_effect}'"
  payload="$(build_approval_payload "${command}" "${marker}")"
  ack_json="$(send_message_ack "${session_id}" "${payload}")"
  turn_id="$(json_get "${ack_json}" "turn_id")"
  message_id="$(json_get "${ack_json}" "message_id")"
  if [[ -z "${turn_id}" || -z "${message_id}" ]]; then
    echo "CASE ${case_name}_start FAIL response=${ack_json}"
    exit 1
  fi

  initial_events="$(scenario_report_path "${case_name}_initial_events.log")"
  collect_timeline_events "${session_id}" "${turn_id}" "${initial_events}"
  initial_response="$(message_response_from_events "${ack_json}" "${initial_events}")"
  log_case_json "${case_name}_waiting" "${payload}" "${initial_response}"
  active_json="$(curl -sS "${GATEWAY_URL}/v1/sessions/${session_id}/active-turn")"
  ACTIVE_JSON="${active_json}" TURN_ID="${turn_id}" COMMAND="${command}" python3 - <<'PY'
import json
import os

active = json.loads(os.environ["ACTIVE_JSON"])["active_turn"]
assert active["turn_id"] == os.environ["TURN_ID"], active
assert active["status"] == "waiting_user_action", active
action = active["pending_user_action"]
assert action["state"] == "pending", action
params = action["params"]
assert params["type"] == "sandboxApproval", params
approval = params["params"]
assert approval["toolName"] == "Bash", approval
assert approval["availableDecisions"] == ["allowOnce", "deny", "cancel"], approval
presentation = approval["presentation"]
assert presentation["type"] == "command", presentation
assert presentation["command"] == os.environ["COMMAND"], presentation
PY

  action_id="$(json_get "${active_json}" "active_turn.pending_user_action.actionId")"
  call_id="$(json_get "${active_json}" "active_turn.pending_user_action.callId")"
  last_event_id="$(json_get "${active_json}" "active_turn.last_event_id")"
  submit_payload="{\"sessionId\":\"${session_id}\",\"turnId\":\"${turn_id}\",\"actionId\":\"${action_id}\",\"response\":{\"type\":\"sandboxApproval\",\"response\":\"${decision}\"}}"
  if [[ "${submission_mode}" == "concurrent" ]]; then
    submit_one_path="$(scenario_report_path "${case_name}_submit_one.json")"
    submit_two_path="$(scenario_report_path "${case_name}_submit_two.json")"
    curl -sS -X POST "${GATEWAY_URL}/v1/user-actions/submit" \
      -H "Content-Type: application/json" \
      -d "${submit_payload}" >"${submit_one_path}" &
    submit_one_pid=$!
    curl -sS -X POST "${GATEWAY_URL}/v1/user-actions/submit" \
      -H "Content-Type: application/json" \
      -d "${submit_payload}" >"${submit_two_path}" &
    submit_two_pid=$!
    wait "${submit_one_pid}"
    wait "${submit_two_pid}"
    submit_one="$(cat "${submit_one_path}")"
    submit_two="$(cat "${submit_two_path}")"
    SUBMIT_ONE="${submit_one}" SUBMIT_TWO="${submit_two}" ACTION_ID="${action_id}" python3 - <<'PY'
import json
import os

responses = [json.loads(os.environ["SUBMIT_ONE"]), json.loads(os.environ["SUBMIT_TWO"])]
assert all(response["actionId"] == os.environ["ACTION_ID"] for response in responses), responses
states = [response["state"] for response in responses]
assert all(state in ("responseAccepted", "resuming") for state in states), responses
assert "resuming" in states, responses
PY
    submit_json="${submit_one}"
  else
    submit_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/user-actions/submit" \
      -H "Content-Type: application/json" \
      -d "${submit_payload}")"
  fi
  expected_state="resuming"
  if [[ "${decision}" == "cancel" ]]; then
    expected_state="cancelled"
  fi
  if [[ "$(json_get "${submit_json}" "actionId")" != "${action_id}" || "$(json_get "${submit_json}" "state")" != "${expected_state}" ]]; then
    echo "CASE ${case_name}_submit FAIL response=${submit_json}"
    exit 1
  fi

  if [[ "${decision}" == "cancel" ]]; then
    terminal_json="$(curl -sS "${GATEWAY_URL}/v1/sessions/${session_id}/active-turn")"
    if [[ "$(json_get "${terminal_json}" "active_turn.status")" != "failed" || -e "${side_effect}" ]]; then
      echo "CASE ${case_name} FAIL active_turn=${terminal_json}"
      exit 1
    fi
    echo "CASE ${case_name} PASS action_id=${action_id}"
    return
  fi

  resumed_events="$(scenario_report_path "${case_name}_resumed_events.log")"
  collect_timeline_events "${session_id}" "${turn_id}" "${resumed_events}" "${last_event_id}"
  resumed_response="$(message_response_from_events "${ack_json}" "${resumed_events}")"
  log_case_json "${case_name}_resumed" "${submit_payload}" "${resumed_response}"
  final_output="$(extract_output_text "${resumed_response}")"
  if [[ "${final_output}" != "${marker}" ]]; then
    echo "CASE ${case_name}_resume FAIL response=${resumed_response}"
    exit 1
  fi
  if [[ "${decision}" == "allowOnce" ]]; then
    if [[ "$(cat "${side_effect}")" != "${case_name}" ]]; then
      echo "CASE ${case_name} FAIL side_effect_not_exactly_once"
      exit 1
    fi
  elif [[ -e "${side_effect}" ]]; then
    echo "CASE ${case_name} FAIL denied_side_effect_exists"
    exit 1
  fi
  assert_terminal_turn "${case_name}" "${turn_id}" "${message_id}"
  history_json="$(curl -sS "${GATEWAY_URL}/v1/sessions/${session_id}/history")"
  HISTORY_JSON="${history_json}" CALL_ID="${call_id}" python3 - <<'PY'
import json
import os

history = json.loads(os.environ["HISTORY_JSON"])["history"]
matching_messages = [
    message
    for message in history["messages"]
    if any(
        item["content"]["type"] == "tool_call"
        and item["content"]["call_id"] == os.environ["CALL_ID"]
        for item in message["items"]
    )
]
tool_calls = [
    item
    for message in matching_messages
    for item in message["items"]
    if item["content"]["type"] == "tool_call"
    and item["content"]["call_id"] == os.environ["CALL_ID"]
]
assert len(matching_messages) == 1, matching_messages
assert len(tool_calls) == 1, tool_calls
assert tool_calls[0]["content"]["result"] is not None, tool_calls[0]
assert matching_messages[0]["stop_reason"] == "end_turn", matching_messages[0]
assert all(
    item["status"] in ("completed", "failed", "cancelled")
    for item in matching_messages[0]["items"]
), matching_messages[0]
PY
  echo "CASE ${case_name} PASS action_id=${action_id}"
}

run_multi_sandbox_approval() {
  local case_name="sandbox_approval_three_allow_once"
  local marker="SANDBOX_THREE_ALLOW_ONCE_OK"
  local payload ack_json turn_id message_id events response active_json action_id last_event_id
  local submit_payload submit_json step history_json

  rm -f "${MULTI_APPROVAL_FILE}"
  payload="$(build_multi_approval_payload "${marker}")"
  ack_json="$(send_message_ack "${session_id}" "${payload}")"
  turn_id="$(json_get "${ack_json}" "turn_id")"
  message_id="$(json_get "${ack_json}" "message_id")"

  events="$(scenario_report_path "${case_name}_initial_events.log")"
  collect_timeline_events "${session_id}" "${turn_id}" "${events}"
  response="$(message_response_from_events "${ack_json}" "${events}")"
  RESPONSE="${response}" python3 - <<'PY'
import json
import os

response = json.loads(os.environ["RESPONSE"])
tool_calls = [item for item in response["output"] if item["type"] == "tool_call"]
assert response["stop_reason"] == "waiting_user_action", response
assert len(tool_calls) == 3, tool_calls
assert [item["name"] for item in tool_calls] == ["Bash", "Bash", "Bash"], tool_calls
assert response["metrics"]["tool_calls"] == 3, response["metrics"]
PY

  for step in 1 2 3; do
    active_json="$(curl -sS "${GATEWAY_URL}/v1/sessions/${session_id}/active-turn")"
    ACTIVE_JSON="${active_json}" TURN_ID="${turn_id}" python3 - <<'PY'
import json
import os

active = json.loads(os.environ["ACTIVE_JSON"])["active_turn"]
assert active["turn_id"] == os.environ["TURN_ID"], active
assert active["status"] == "waiting_user_action", active
action = active["pending_user_action"]
assert action["state"] == "pending", action
assert action["params"]["type"] == "sandboxApproval", action
assert action["params"]["params"]["toolName"] == "Bash", action
PY
    action_id="$(json_get "${active_json}" "active_turn.pending_user_action.actionId")"
    last_event_id="$(json_get "${active_json}" "active_turn.last_event_id")"
    submit_payload="{\"sessionId\":\"${session_id}\",\"turnId\":\"${turn_id}\",\"actionId\":\"${action_id}\",\"response\":{\"type\":\"sandboxApproval\",\"response\":\"allowOnce\"}}"
    submit_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/user-actions/submit" \
      -H "Content-Type: application/json" \
      -d "${submit_payload}")"
    if [[ "$(json_get "${submit_json}" "state")" != "resuming" ]]; then
      echo "CASE ${case_name}_step_${step}_submit FAIL response=${submit_json}"
      exit 1
    fi

    events="$(scenario_report_path "${case_name}_step_${step}_events.log")"
    collect_timeline_events "${session_id}" "${turn_id}" "${events}" "${last_event_id}"
    response="$(message_response_from_events "${ack_json}" "${events}")"
    if [[ "${step}" -lt 3 && "$(json_get "${response}" "stop_reason")" != "waiting_user_action" ]]; then
      echo "CASE ${case_name}_step_${step} FAIL response=${response}"
      exit 1
    fi
  done

  if [[ "$(extract_output_text "${response}")" != "${marker}" ]]; then
    echo "CASE ${case_name}_final FAIL response=${response}"
    exit 1
  fi
  if [[ "$(sort "${MULTI_APPROVAL_FILE}")" != $'MULTI_1\nMULTI_2\nMULTI_3' ]]; then
    echo "CASE ${case_name}_side_effects FAIL"
    exit 1
  fi
  assert_terminal_turn "${case_name}" "${turn_id}" "${message_id}"

  history_json="$(curl -sS "${GATEWAY_URL}/v1/sessions/${session_id}/history")"
  HISTORY_JSON="${history_json}" MESSAGE_ID="${message_id}" python3 - <<'PY'
import json
import os

history = json.loads(os.environ["HISTORY_JSON"])["history"]
message = next(item for item in history["messages"] if item["message_id"] == os.environ["MESSAGE_ID"])
tool_calls = [item for item in message["items"] if item["content"]["type"] == "tool_call"]
assert len(tool_calls) == 3, tool_calls
assert len({item["content"]["call_id"] for item in tool_calls}) == 3, tool_calls
assert all(item["content"]["result"] is not None for item in tool_calls), tool_calls
metrics = message["metrics"]
assert metrics["tool_calls"] == 3, metrics
assert metrics["tool_successes"] == 3, metrics
assert metrics["tool_errors"] == 0, metrics
PY
  echo "CASE ${case_name} PASS turn_id=${turn_id}"
}

run_sandbox_approval \
  "sandbox_approval_allow_once" "allowOnce" "SANDBOX_ALLOW_ONCE_OK" \
  "${APPROVAL_WORKSPACE}/allow-once-marker" "concurrent"
run_multi_sandbox_approval
run_sandbox_approval \
  "sandbox_approval_deny" "deny" "SANDBOX_DENY_OK" \
  "${APPROVAL_WORKSPACE}/deny-marker"
run_sandbox_approval \
  "sandbox_approval_cancel" "cancel" "SANDBOX_CANCEL_UNUSED" \
  "${APPROVAL_WORKSPACE}/cancel-marker"
