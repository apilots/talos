#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

load_env
require_command curl
require_command python3

if [[ -z "${TALOS_LLM_BAILIAN_API_KEY:-}" && -z "${TALOS_LLM_ANTHROPIC_API_KEY:-}" && -z "${TALOS_LLM_OPENAI_API_KEY:-}" ]]; then
  echo "CASE agent_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE agent_real_llm_precheck PASS key_detected=true"

build_binaries
start_server
trap stop_server EXIT

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
primary_model="$(select_e2e_model_from_json "${models_json}")"
if [[ -z "${primary_model}" ]]; then
  echo "CASE agent_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
echo "CASE agent_select_model PASS model=${primary_model}"

build_session_context() {
  python3 - <<'PY'
import json
import os

payload = {
    "context": {
        "source_type": "http",
        "provider": "http",
        "channel_id": "tool-agent",
        "user_id": "tool-agent-user",
        "thread_id": None,
        "message_id": "msg-tool-agent",
        "trace_id": "trace-tool-agent",
    },
    "model": os.environ["PRIMARY_MODEL"],
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
    "input": [{"type": "text", "text": os.environ["INPUT_TEXT"]}],
    "workspace_id": None,
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

session_payload="$(build_session_context)"
session_id="$(new_session_id)"
remember_session_context "${session_id}" "${session_payload}"
session_json="{\"id\":\"${session_id}\"}"
session_id="$(session_id_from_json "${session_json}")"
if [[ -z "${session_id}" ]]; then
  echo "CASE agent_session_id FAIL response=${session_json}"
  exit 1
fi
echo "CASE agent_session_id PASS session_id=${session_id}"

agent_input="You must call tool 'Agent' exactly once with description child echo task, prompt Reply with AGENT_CHILD_OK, subagent_type review, and run_in_background true. After the tool call, briefly report the resulting status."
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
if results:
    result = results[0]
    assert result.get("tool_name") == "Agent", result.get("tool_name")
    content_items = result.get("content") or []
    assert content_items, "missing content payload"
    item = content_items[0]
    assert item.get("type") == "json", item
    content = (item.get("value") or {}).get("value") or {}
    assert content.get("session_id"), content
    assert content.get("agent_kind") in {"general_purpose", "review", "implementation", "research"}, content
    assert content.get("execution_mode") == "background", content
    assert content.get("status") in {"running", "completed"}, content
PY

AGENT_EVENTS_LOG="$(scenario_report_path agent_events.log)"
AGENT_EVENTS_LOG="${AGENT_EVENTS_LOG}" python3 - <<'PY'
import json
import os
from pathlib import Path

path = Path(os.environ["AGENT_EVENTS_LOG"])
tool_item_id = None
saw_terminal_result = False

for line in path.read_text(encoding="utf-8").splitlines():
    if not line.startswith("data: "):
        continue
    raw_record = json.loads(line[6:])
    record = raw_record.get("event") if isinstance(raw_record.get("event"), dict) else raw_record
    payload = record.get("payload") or {}
    if record.get("event_type") == "item_started":
        item = payload.get("item") or {}
        content = item.get("content") or {}
        if item.get("kind") == "tool_call" and content.get("type") == "tool_call" and content.get("name") == "Agent":
            tool_item_id = item.get("item_id")
    elif record.get("event_type") == "item_delta":
        delta = payload.get("delta") or {}
        if payload.get("item_id") != tool_item_id:
            continue
        if delta.get("type") == "tool_result":
            saw_terminal_result = True

assert tool_item_id, "missing Agent tool item"
assert saw_terminal_result, "Agent tool should emit terminal tool_result"
PY

echo "CASE tool_agent PASS"
echo "DETAIL tool_agent_output=${agent_output}"
