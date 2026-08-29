#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

require_command curl
require_command python3

if ! has_real_llm_key; then
  echo "CASE todo_write_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE todo_write_real_llm_precheck PASS key_detected=true"

TODO_ID="todo-case-$(date '+%Y%m%d%H%M%S')-$$"

build_binaries
start_server
trap stop_server EXIT

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
primary_model="$(select_e2e_model_from_json "${models_json}")"
primary_provider="$(select_e2e_model_provider_from_json "${models_json}" "${primary_model}")"
if [[ -z "${primary_model}" || -z "${primary_provider}" ]]; then
  echo "CASE todo_write_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
export PRIMARY_PROVIDER="${primary_provider}"
echo "CASE todo_write_select_model PASS model=${primary_model}"

build_session_context() {
  python3 - <<'PY'
import json
import os

payload = {
    "context": {
        "source_type": "http",
        "provider": "http",
        "channel_id": "tool-todo-write",
        "user_id": "tool-todo-write-user",
        "thread_id": None,
        "message_id": "msg-tool-todo-write",
        "trace_id": "trace-tool-todo-write",
    },
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
    "input": [{"type": "text", "text": os.environ["INPUT_TEXT"]}],
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
  echo "CASE todo_write_session_id FAIL response=${session_json}"
  exit 1
fi
echo "CASE todo_write_session_id PASS session_id=${session_id}"

todo_input="You must call tool 'TodoWrite' exactly once with a todo item id ${TODO_ID}, content placeholder test todo only, status in_progress. After the tool call, respond with exactly TODO_WRITE_OK:${TODO_ID}"
todo_payload="$(build_payload "${todo_input}")"
todo_json="$(send_message_response "${session_id}" "${todo_payload}" "todo")"
log_case_json "tool_todo_write" "${todo_payload}" "${todo_json}"

todo_output="$(extract_output_text "${todo_json}")"
if [[ -z "${todo_output}" || "${todo_output}" == Error:* ]]; then
  echo "CASE tool_todo_write FAIL response=${todo_json}"
  exit 1
fi
TODO_JSON="${todo_json}" TODO_ID="${TODO_ID}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["TODO_JSON"])
todo_id = os.environ["TODO_ID"]
tool_calls = [item for item in data.get("output", []) if item.get("type") == "tool_call"]
assert tool_calls, "missing tool_call"
call = tool_calls[0]
assert call.get("name") == "TodoWrite", call.get("name")
todos = (call.get("arguments") or {}).get("todos") or []
assert todos, "missing tool_call todos"
assert todos[0].get("id") == todo_id, todos[0]
assert todos[0].get("status") == "in_progress", todos[0]
results = [item for item in data.get("output", []) if item.get("type") == "tool_result"]
assert results, "missing tool_result"
result = results[0]
assert result.get("tool_name") == "TodoWrite", result.get("tool_name")
content_items = result.get("content") or []
assert content_items, "missing content payload"
item = content_items[0]
assert item.get("type") == "json", item
value = (item.get("value") or {}).get("value") or {}
items = value.get("items") or []
assert items, "missing todo items"
first = items[0]
assert first.get("status") == "in_progress", first
assert first.get("id") == todo_id, first
PY

echo "CASE tool_todo_write PASS"
echo "DETAIL tool_todo_write_output=${todo_output}"
