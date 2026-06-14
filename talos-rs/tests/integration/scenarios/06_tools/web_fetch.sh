#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command curl
require_command python3

if [[ -z "${TALOS_LLM_BAILIAN_API_KEY:-}" && -z "${TALOS_LLM_ANTHROPIC_API_KEY:-}" && -z "${TALOS_LLM_OPENAI_API_KEY:-}" ]]; then
  echo "CASE web_fetch_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE web_fetch_real_llm_precheck PASS key_detected=true"

build_binaries
start_server
trap stop_server EXIT

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
primary_model="$(select_e2e_model_from_json "${models_json}")"
if [[ -z "${primary_model}" ]]; then
  echo "CASE web_fetch_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
echo "CASE web_fetch_select_model PASS model=${primary_model}"

build_session_context() {
  python3 - <<'PY'
import json
import os

payload = {
    "context": {
        "source_type": "http",
        "provider": "http",
        "channel_id": "tool-web-fetch",
        "user_id": "tool-web-fetch-user",
        "thread_id": None,
        "message_id": "msg-tool-web-fetch",
        "trace_id": "trace-tool-web-fetch",
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
  echo "CASE web_fetch_session_id FAIL response=${session_json}"
  exit 1
fi
echo "CASE web_fetch_session_id PASS session_id=${session_id}"

fetch_input="You must call tool 'WebFetch' exactly once with url https://example.com and max_chars 1500. After the tool call, respond with exactly WEB_FETCH_OK:Example Domain"
fetch_payload="$(build_payload "${fetch_input}")"
fetch_json="$(send_message_response "${session_id}" "${fetch_payload}" "fetch")"
log_case_json "tool_web_fetch" "${fetch_payload}" "${fetch_json}"

fetch_output="$(extract_output_text "${fetch_json}")"
if [[ -z "${fetch_output}" || "${fetch_output}" == Error:* ]]; then
  echo "CASE tool_web_fetch FAIL response=${fetch_json}"
  exit 1
fi
if ! echo "${fetch_output}" | grep -Fq "WEB_FETCH_OK:Example Domain"; then
  echo "CASE tool_web_fetch FAIL missing_title output=${fetch_output}"
  exit 1
fi

WEB_FETCH_JSON="${fetch_json}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["WEB_FETCH_JSON"])
results = [item for item in data.get("output", []) if item.get("type") == "tool_result"]
assert results, "missing tool_result"
result = results[0]
assert result.get("tool_name") == "WebFetch", result.get("tool_name")
content_items = result.get("content") or []
assert content_items, "missing content payload"
content = ((content_items[0].get("value") or {}).get("value", content_items[0].get("value") or {}))
assert content.get("title") == "Example Domain", content
assert "example.com" in (content.get("final_url") or ""), content
assert content.get("content"), content
PY

echo "CASE tool_web_fetch PASS"
echo "DETAIL tool_web_fetch_output=${fetch_output}"
