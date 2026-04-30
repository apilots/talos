#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command curl
require_command python3

if [[ -z "${BAILIAN_API_KEY:-}" && -z "${BAILIAN_CODINGPLAN_API_KEY:-}" && -z "${OPENAI_API_KEY:-}" && -z "${ANTHROPIC_API_KEY:-}" ]]; then
  echo "CASE agent_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE agent_real_llm_precheck PASS key_detected=true"

build_binaries
start_server
trap stop_server EXIT

models_json="$(curl -sS -H "Authorization: Bearer ${AUTH_TOKEN}" "${GATEWAY_URL}/v1/models")"
primary_model="$(python3 -c 'import json,sys; data=json.load(sys.stdin); arr=data.get("data",[]); print((arr[0].get("id") if arr else ""))' <<< "${models_json}")"
if [[ -z "${primary_model}" ]]; then
  echo "CASE agent_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
echo "CASE agent_select_model PASS model=${primary_model}"

create_session() {
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

session_payload="$(create_session)"
session_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${session_payload}")"
session_id="$(session_id_from_json "${session_json}")"
if [[ -z "${session_id}" ]]; then
  echo "CASE agent_create_session FAIL response=${session_json}"
  exit 1
fi
echo "CASE agent_create_session PASS session_id=${session_id}"

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
assert results, "missing tool_result"
result = results[0]
assert result.get("tool_name") == "Agent", result.get("tool_name")
content_items = result.get("content") or []
assert content_items, "missing content payload"
content = ((content_items[0].get("value") or {}).get("value", content_items[0].get("value") or {}))
envelope = content.get("envelope") or {}
pending = envelope.get("pending") or {}
if envelope.get("status") == "pending":
    assert result.get("is_error") is True, result
    assert pending.get("pending_kind") == "approval", pending
    assert "approval" in (pending.get("prompt") or "").lower(), pending
    assert "tool_use_progress" in (content.get("semantic_events") or []), content
else:
    assert content.get("session_id"), content
    assert content.get("agent_kind") in {"general_purpose", "review", "implementation", "research"}, content
    assert content.get("execution_mode") == "background", content
    assert content.get("status") in {"running", "completed"}, content
PY

echo "CASE tool_agent PASS"
echo "DETAIL tool_agent_output=${agent_output}"
