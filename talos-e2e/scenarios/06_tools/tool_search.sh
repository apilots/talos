#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

load_env
require_command curl
require_command python3

if [[ -z "${TALOS_LLM_BAILIAN_API_KEY:-}" && -z "${TALOS_LLM_ANTHROPIC_API_KEY:-}" && -z "${TALOS_LLM_OPENAI_API_KEY:-}" ]]; then
  echo "CASE tool_search_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE tool_search_real_llm_precheck PASS key_detected=true"

build_binaries
start_server
trap stop_server EXIT

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
primary_model="$(select_e2e_model_from_json "${models_json}")"
if [[ -z "${primary_model}" ]]; then
  echo "CASE tool_search_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
echo "CASE tool_search_select_model PASS model=${primary_model}"

build_session_context() {
  python3 - <<'PY'
import json
import os

payload = {
    "context": {
        "source_type": "http",
        "provider": "http",
        "channel_id": "tool-tool-search",
        "user_id": "tool-tool-search-user",
        "thread_id": None,
        "message_id": "msg-tool-tool-search",
        "trace_id": "trace-tool-tool-search",
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
  echo "CASE tool_search_session_id FAIL response=${session_json}"
  exit 1
fi
echo "CASE tool_search_session_id PASS session_id=${session_id}"

search_input="You must call tool 'ToolSearch' exactly once with query WebSearch. After the tool call, respond with exactly TOOL_SEARCH_OK"
search_payload="$(build_payload "${search_input}")"
search_json="$(send_message_response "${session_id}" "${search_payload}" "search")"
log_case_json "tool_search" "${search_payload}" "${search_json}"

search_output="$(extract_output_text "${search_json}")"
if [[ -z "${search_output}" || "${search_output}" == Error:* ]]; then
  echo "CASE tool_search FAIL response=${search_json}"
  exit 1
fi
if ! echo "${search_output}" | grep -Fq "TOOL_SEARCH_OK"; then
  echo "CASE tool_search FAIL missing_ack output=${search_output}"
  exit 1
fi

TOOL_SEARCH_JSON="${search_json}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["TOOL_SEARCH_JSON"])
results = [item for item in data.get("output", []) if item.get("type") == "tool_result"]
assert results, "missing tool_result"
result = results[0]
assert result.get("tool_name") == "ToolSearch", result.get("tool_name")
content_items = result.get("content") or []
assert content_items, "missing content payload"
item = content_items[0]
assert item.get("type") == "json", item
content = (item.get("value") or {}).get("value") or {}
matches = content.get("matches") or []
assert matches, "missing matches"
assert any(item.get("public_name") == "WebSearch" for item in matches), matches
PY

echo "CASE tool_search PASS"
echo "DETAIL tool_search_output=${search_output}"
