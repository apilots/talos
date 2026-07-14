#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

require_command curl
require_command python3

if ! has_real_llm_key; then
  echo "CASE web_search_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE web_search_real_llm_precheck PASS key_detected=true"

build_binaries
start_server
trap stop_server EXIT

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
primary_model="$(select_e2e_model_from_json "${models_json}")"
primary_provider="$(select_e2e_model_provider_from_json "${models_json}" "${primary_model}")"
if [[ -z "${primary_model}" || -z "${primary_provider}" ]]; then
  echo "CASE web_search_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
export PRIMARY_PROVIDER="${primary_provider}"
echo "CASE web_search_select_model PASS model=${primary_model}"

build_session_context() {
  python3 - <<'PY'
import json
import os

payload = {
    "context": {
        "source_type": "http",
        "provider": "http",
        "channel_id": "tool-web-search",
        "user_id": "tool-web-search-user",
        "thread_id": None,
        "message_id": "msg-tool-web-search",
        "trace_id": "trace-tool-web-search",
    },
    "modelSelection": {"modelRef": {"provider": os.environ["PRIMARY_PROVIDER"], "id": os.environ["PRIMARY_MODEL"]}, "reasoning": {"type": "modelDefault"}},
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
  echo "CASE web_search_session_id FAIL response=${session_json}"
  exit 1
fi
echo "CASE web_search_session_id PASS session_id=${session_id}"

activate_input="You must call tool 'ToolSearch' exactly once with query WebSearch to activate it. Do not call WebSearch in this turn. After the ToolSearch tool result is returned, respond with exactly WEB_SEARCH_ACTIVATED"
activate_payload="$(build_payload "${activate_input}")"
activate_json="$(send_message_response "${session_id}" "${activate_payload}" "activate")"
log_case_json "tool_web_search_activate" "${activate_payload}" "${activate_json}"

ACTIVATE_JSON="${activate_json}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["ACTIVATE_JSON"])
results = [item for item in data.get("output", []) if item.get("type") == "tool_result"]
assert results, "missing tool_result"
tool_names = [item.get("tool_name") for item in results]
assert "ToolSearch" in tool_names, tool_names
PY

search_input="You must call tool 'WebSearch' exactly once with query site:example.com Example Domain, allowed_domains example.com, and limit 3. Do not call ToolSearch again. After the WebSearch tool result is returned, respond with exactly WEB_SEARCH_OK"
search_payload="$(build_payload "${search_input}")"
search_json="$(send_message_response "${session_id}" "${search_payload}" "search")"
log_case_json "tool_web_search" "${search_payload}" "${search_json}"

search_output="$(extract_output_text "${search_json}")"
if [[ -z "${search_output}" || "${search_output}" == Error:* ]]; then
  echo "CASE tool_web_search FAIL response=${search_json}"
  exit 1
fi

WEB_SEARCH_JSON="${search_json}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["WEB_SEARCH_JSON"])
results = [item for item in data.get("output", []) if item.get("type") == "tool_result"]
assert results, "missing tool_result"
tool_names = [item.get("tool_name") for item in results]
assert "WebSearch" in tool_names, tool_names
result = next(item for item in results if item.get("tool_name") == "WebSearch")
content_items = result.get("content") or []
assert content_items, "missing content payload"
item = content_items[0]
item_type = item.get("type")
assert item_type in {"web_reference", "text"}, item
if item_type == "web_reference":
    assert item.get("url"), item
    assert item.get("citation_id"), item
else:
    assert "No search results" in item.get("text", ""), item
PY

echo "CASE tool_web_search PASS"
echo "DETAIL tool_web_search_output=${search_output}"
