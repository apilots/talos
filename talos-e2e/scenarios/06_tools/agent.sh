#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

require_command curl
require_command python3

if ! has_real_llm_key; then
  echo "CASE agent_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE agent_real_llm_precheck PASS key_detected=true"

build_binaries
start_server
trap stop_server EXIT

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
primary_model="$(select_e2e_model_from_json "${models_json}")"
primary_provider="$(select_e2e_model_provider_from_json "${models_json}" "${primary_model}")"
if [[ -z "${primary_model}" || -z "${primary_provider}" ]]; then
  echo "CASE agent_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
export PRIMARY_PROVIDER="${primary_provider}"
echo "CASE agent_select_model PASS model=${primary_model}"

build_session_start_defaults() {
  python3 - <<'PY'
import json
import os

payload = {
    "modelSelection": {"modelRef": {"provider": os.environ["PRIMARY_PROVIDER"], "id": os.environ["PRIMARY_MODEL"]}, "reasoningEffort": None},
}
print(json.dumps(payload, ensure_ascii=False))
PY
}

build_payload() {
  local input_text="$1"
  INPUT_TEXT="$input_text" python3 - <<'PY'
import json
import os

payload = {
    "input": {"content": [{"type": "text", "text": os.environ["INPUT_TEXT"]}]},
}
print(json.dumps(payload, ensure_ascii=False))
PY
}

extract_output_text() {
  local response_json="$1"
  RESPONSE_JSON="${response_json}" python3 - <<'PY'
import json
import os

raw = os.environ.get("RESPONSE_JSON", "")
try:
    data = json.loads(raw)
except Exception:
    print("")
    raise SystemExit(0)

for item in data.get("output", []):
    if item.get("type") == "output_text":
        print(item.get("text", ""))
        raise SystemExit(0)
print(data.get("output_text", ""))
PY
}

session_payload="$(build_session_start_defaults)"
session_id="$(new_session_id)"
remember_session_start_defaults "${session_id}" "${session_payload}"
session_json="{\"id\":\"${session_id}\"}"
session_id="$(session_id_from_json "${session_json}")"
if [[ -z "${session_id}" ]]; then
  echo "CASE agent_session_id FAIL response=${session_json}"
  exit 1
fi
echo "CASE agent_session_id PASS session_id=${session_id}"

agent_input="Spawn a subagent named child_echo with this exact task: first call Bash with command 'sleep 5'; after Bash finishes, follow the most recent instruction received; if there is no newer instruction, output ORIGINAL_ONLY. Immediately after spawn_agent returns, call send_message for child_echo with the message 'Output exactly AGENT_CHILD_OK'. Do not wait before sending that message. Then call wait_agent for child_echo and briefly report the final child status."
agent_payload="$(build_payload "${agent_input}")"
agent_json="$(send_message_response "${session_id}" "${agent_payload}" "agent")"
log_case_json "tool_agent" "${agent_payload}" "${agent_json}"

agent_output="$(extract_output_text "${agent_json}")"
if [[ -z "${agent_output}" || "${agent_output}" == Error:* ]]; then
  echo "CASE tool_agent FAIL response=${agent_json}"
  exit 1
fi

AGENT_JSON="${agent_json}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["AGENT_JSON"])
results = [item for item in data.get("output", []) if item.get("type") == "tool_result"]
by_name = {item.get("tool_name"): item for item in results}
assert set(by_name) >= {"spawn_agent", "send_message", "wait_agent"}, by_name.keys()

spawn_content = (by_name["spawn_agent"].get("content") or [None])[0]
assert spawn_content and spawn_content.get("type") == "subagent_result", spawn_content
spawn = spawn_content.get("result") or {}
assert spawn.get("operation") == "spawnAgent", spawn
spawn_result = spawn.get("result") or {}
assert spawn_result.get("agentId") == "default", spawn_result
assert (spawn_result.get("subagent") or {}).get("path") == "/root/child_echo", spawn_result
assert (spawn_result.get("subagent") or {}).get("sessionId"), spawn_result
assert (spawn_result.get("turn") or {}).get("turnId"), spawn_result

send_content = (by_name["send_message"].get("content") or [None])[0]
assert send_content and send_content.get("type") == "subagent_result", send_content
send = send_content.get("result") or {}
assert send.get("operation") == "sendMessage", send
send_result = send.get("result") or {}
assert send_result.get("agentId") == "default", send_result
assert (send_result.get("subagent") or {}).get("path") == "/root/child_echo", send_result
assert (send_result.get("turn") or {}).get("turnId") == (spawn_result.get("turn") or {}).get("turnId"), send_result
assert send_result.get("accepted") is True, send_result

wait_content = (by_name["wait_agent"].get("content") or [None])[0]
assert wait_content and wait_content.get("type") == "subagent_result", wait_content
wait = wait_content.get("result") or {}
assert wait.get("operation") == "waitAgent", wait
snapshot = wait.get("result") or {}
assert snapshot.get("agentId") == "default", snapshot
assert (snapshot.get("subagent") or {}).get("path") == "/root/child_echo", snapshot
outcome = snapshot.get("outcome") or {}
assert outcome.get("status") == "completed", outcome
assert "AGENT_CHILD_OK" in outcome.get("output", ""), outcome
PY

AGENT_EVENTS_LOG="$(scenario_report_path agent_events.log)"
AGENT_EVENTS_LOG="${AGENT_EVENTS_LOG}" python3 - <<'PY'
import json
import os
from pathlib import Path

path = Path(os.environ["AGENT_EVENTS_LOG"])
tool_call_ids = {}
tool_arguments = {}
terminal_result_ids = set()

for line in path.read_text(encoding="utf-8").splitlines():
    if not line.startswith("data: "):
        continue
    raw_record = json.loads(line[6:])
    event = raw_record.get("event") if isinstance(raw_record.get("event"), dict) else {}
    payload = event.get("payload") or {}
    if payload.get("type") != "messageBlockCompleted":
        continue
    block = (payload.get("data") or {}).get("block") or {}
    content = block.get("content") or {}
    content_type = content.get("type")
    data = content.get("data") or {}
    if content_type == "toolCall" and data.get("name") in {"spawn_agent", "send_message", "wait_agent"}:
        tool_name = data["name"]
        tool_call_ids[tool_name] = data.get("callId")
        arguments = data.get("arguments") or {}
        tool_arguments[tool_name] = arguments.get("value") or {}
    elif content_type == "toolResult" and data.get("toolName") in {"spawn_agent", "send_message", "wait_agent"}:
        assert data.get("isError") is False, data
        terminal_result_ids.add(data.get("callId"))

assert set(tool_call_ids) == {"spawn_agent", "send_message", "wait_agent"}, tool_call_ids
assert "agentId" not in tool_arguments["spawn_agent"], tool_arguments["spawn_agent"]
assert "agentId" not in tool_arguments["send_message"], tool_arguments["send_message"]
assert tool_arguments["send_message"].get("message") == "Output exactly AGENT_CHILD_OK", tool_arguments["send_message"]
assert set(tool_call_ids.values()).issubset(terminal_result_ids), terminal_result_ids
PY

echo "CASE tool_agent PASS"
echo "DETAIL tool_agent_output=${agent_output}"
