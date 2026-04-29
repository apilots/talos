#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command curl
require_command python3

if [[ -z "${BAILIAN_API_KEY:-}" && -z "${BAILIAN_CODINGPLAN_API_KEY:-}" && -z "${OPENAI_API_KEY:-}" && -z "${ANTHROPIC_API_KEY:-}" ]]; then
  echo "CASE web_search_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE web_search_real_llm_precheck PASS key_detected=true"

build_binaries
start_server
trap stop_server EXIT

models_json="$(curl -sS -H "Authorization: Bearer ${AUTH_TOKEN}" "${GATEWAY_URL}/v1/models")"
primary_model="$(python3 -c 'import json,sys; data=json.load(sys.stdin); arr=data.get("data",[]); print((arr[0].get("id") if arr else ""))' <<< "${models_json}")"
if [[ -z "${primary_model}" ]]; then
  echo "CASE web_search_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
echo "CASE web_search_select_model PASS model=${primary_model}"

create_session() {
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
  echo "CASE web_search_create_session FAIL response=${session_json}"
  exit 1
fi
echo "CASE web_search_create_session PASS session_id=${session_id}"

search_input="Within this single turn, first call tool 'ToolSearch' with query WebSearch to activate it, then call tool 'WebSearch' exactly once with query site:example.com Example Domain, allowed_domains example.com, and limit 3. After both tool calls, respond with exactly WEB_SEARCH_OK"
search_payload="$(build_payload "${search_input}")"
search_json="$(send_message_response "${session_id}" "${search_payload}" "search")"
log_case_json "tool_web_search" "${search_payload}" "${search_json}"

search_output="$(extract_output_text "${search_json}")"
if [[ -z "${search_output}" || "${search_output}" == Error:* ]]; then
  echo "CASE tool_web_search FAIL response=${search_json}"
  exit 1
fi
if ! echo "${search_output}" | grep -Fq "WEB_SEARCH_OK"; then
  echo "CASE tool_web_search FAIL missing_ack output=${search_output}"
  exit 1
fi

WEB_SEARCH_JSON="${search_json}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["WEB_SEARCH_JSON"])
results = [item for item in data.get("output", []) if item.get("type") == "tool_result"]
assert results, "missing tool_result"
tool_names = [item.get("tool_name") for item in results]
assert "ToolSearch" in tool_names, tool_names
assert "WebSearch" in tool_names, tool_names
result = next(item for item in results if item.get("tool_name") == "WebSearch")
content_items = result.get("content") or []
assert content_items, "missing content payload"
content = content_items[0].get("value") or {}
hits = content.get("hits")
assert isinstance(hits, list), content
assert content.get("query") == "site:example.com Example Domain", content
assert content.get("summary"), content
PY

echo "CASE tool_web_search PASS"
echo "DETAIL tool_web_search_output=${search_output}"
