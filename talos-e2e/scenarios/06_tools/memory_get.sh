#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

require_command curl
require_command python3
require_command mktemp

if ! has_real_llm_key; then
  echo "CASE memory_get_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE memory_get_real_llm_precheck PASS key_detected=true"

TEST_TALOS_HOME="$(make_e2e_temp_dir "talos-tool-memory-get-home")"
cleanup() {
  stop_server
  rm -rf "${TEST_TALOS_HOME}"
}
export TALOS_HOME="${TEST_TALOS_HOME}"

MEMORY_DATE="$(date '+%Y-%m-%d')"
MEMORY_MARKER="memory-get-marker-$(date '+%Y%m%d%H%M%S')-$$"

build_binaries
start_server
trap cleanup EXIT

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
primary_model="$(select_e2e_model_from_json "${models_json}")"
primary_provider="$(select_e2e_model_provider_from_json "${models_json}" "${primary_model}")"
if [[ -z "${primary_model}" || -z "${primary_provider}" ]]; then
  echo "CASE memory_get_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
export PRIMARY_PROVIDER="${primary_provider}"
echo "CASE memory_get_select_model PASS model=${primary_model}"

request_json() {
  local method="$1"
  local url="$2"
  local body="${3:-}"
  local response_file
  response_file="$(mktemp "${TMPDIR:-/tmp}/talos-integration-response.XXXXXX")"
  local status
  if [[ -n "${body}" ]]; then
    status="$(curl -sS -o "${response_file}" -w "%{http_code}" \
      -X "${method}" "${url}" \
      -H "Content-Type: application/json" \
      -d "${body}")"
  else
    status="$(curl -sS -o "${response_file}" -w "%{http_code}" \
      -X "${method}" "${url}" \
     )"
  fi
  cat "${response_file}"
  rm -f "${response_file}"
  printf '\nHTTP_STATUS:%s' "${status}"
}

parse_response() {
  local raw="$1"
  RESPONSE_STATUS="${raw##*HTTP_STATUS:}"
  RESPONSE_BODY="${raw%HTTP_STATUS:*}"
}

build_session_context() {
  python3 - <<'PY'
import json
import os

payload = {
    "context": {
        "source_type": "http",
        "provider": "http",
        "channel_id": "tool-memory-get",
        "user_id": "tool-memory-get-user",
        "thread_id": None,
        "message_id": "msg-tool-memory-get",
        "trace_id": "trace-tool-memory-get",
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

create_memory_payload="$(python3 - <<PY
import json
from datetime import datetime, timezone
from zoneinfo import ZoneInfo

occurred_at = int(datetime.strptime("${MEMORY_DATE} 09:30", "%Y-%m-%d %H:%M").replace(tzinfo=ZoneInfo("Asia/Shanghai")).timestamp())
generated_at = datetime.fromtimestamp(occurred_at, timezone.utc).isoformat().replace("+00:00", "Z")
content = f"""---
type: DreamMemory
title: Dream Memory ${MEMORY_DATE}
description: Durable memories formed for ${MEMORY_DATE}.
tags: [talos, dream-memory]
status: stable
generated:
  by: talos-agent/default
  at: "{generated_at}"
---

# Dream Memory ${MEMORY_DATE}

<!-- talos:dream-memory:start -->

## Memories

<!-- talos:dream-memory:entry key="mem_0123456789abcdef0123456789abcdef" -->
### Memory get fixture

- **Occurred At:** {occurred_at}
- **Kind:** fact
- **Importance:** normal

${MEMORY_MARKER}

<!-- talos:dream-memory:end -->
"""
print(json.dumps({
    "date": "${MEMORY_DATE}",
    "content": content,
}))
PY
)"
parse_response "$(request_json PUT "${GATEWAY_URL}/v1/memory/files/${MEMORY_DATE}" "${create_memory_payload}")"
status="${RESPONSE_STATUS}"
body="${RESPONSE_BODY}"
log_case_json "seed_memory_file_for_memory_get" "${create_memory_payload}" "${body}"
if [[ "${status}" != "200" ]]; then
  echo "CASE seed_memory_file_for_memory_get FAIL status=${status} body=${body}"
  exit 1
fi
echo "CASE seed_memory_file_for_memory_get PASS status=${status}"

memory_search_json="$(curl -sS "${GATEWAY_URL}/v1/memory/search?query=${MEMORY_MARKER}&limit=1")"
memory_id="$(MEMORY_SEARCH_JSON="${memory_search_json}" python3 - <<'PY'
import json
import os

raw = json.loads(os.environ["MEMORY_SEARCH_JSON"])
if not (isinstance(raw, dict) and raw.get("status") == "success" and isinstance(raw.get("data"), dict)):
    raise SystemExit("memory search response must be ApiResponse success envelope")
raw = raw["data"]
data = raw.get("data", [])
print(data[0].get("memoryId", "") if data else "")
PY
)"
if [[ -z "${memory_id}" ]]; then
  echo "CASE seed_memory_search_for_memory_get FAIL response=${memory_search_json}"
  exit 1
fi
echo "CASE seed_memory_search_for_memory_get PASS memory_id=${memory_id}"

session_payload="$(build_session_context)"
session_id="$(new_session_id)"
remember_session_context "${session_id}" "${session_payload}"
session_json="{\"id\":\"${session_id}\"}"
session_id="$(session_id_from_json "${session_json}")"
if [[ -z "${session_id}" ]]; then
  echo "CASE memory_get_session_id FAIL response=${session_json}"
  exit 1
fi
echo "CASE memory_get_session_id PASS session_id=${session_id}"

memory_get_input="You must call tool 'MemoryGet' exactly once with memoryId ${memory_id}. After the tool call, respond with exactly MEMORY_GET_OK:${MEMORY_MARKER}"
memory_get_payload="$(build_payload "${memory_get_input}")"
memory_get_json="$(send_message_response "${session_id}" "${memory_get_payload}" "memory_get")"
log_case_json "tool_memory_get" "${memory_get_payload}" "${memory_get_json}"

memory_get_output="$(extract_output_text "${memory_get_json}")"
if [[ -z "${memory_get_output}" || "${memory_get_output}" == Error:* ]]; then
  echo "CASE tool_memory_get FAIL response=${memory_get_json}"
  exit 1
fi
if ! echo "${memory_get_output}" | grep -Fq "MEMORY_GET_OK:${MEMORY_MARKER}"; then
  echo "CASE tool_memory_get FAIL missing_marker output=${memory_get_output}"
  exit 1
fi

tool_result_check="$(MEMORY_GET_JSON="${memory_get_json}" MEMORY_ID="${memory_id}" MEMORY_MARKER="${MEMORY_MARKER}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["MEMORY_GET_JSON"])
calls = [item for item in data.get("output", []) if item.get("type") == "tool_call"]
results = [item for item in data.get("output", []) if item.get("type") == "tool_result"]
call = calls[0] if calls else {}
result = results[0] if results else {}
content = json.dumps(result.get("content", []), ensure_ascii=False)
valid = (
    len(calls) == 1
    and len(results) == 1
    and call.get("name") == "MemoryGet"
    and call.get("arguments", {}).get("memoryId") == os.environ["MEMORY_ID"]
    and result.get("tool_name") == "MemoryGet"
    and result.get("is_error") is False
    and os.environ["MEMORY_MARKER"] in content
)
print("ok" if valid else "invalid")
PY
)"
if [[ "${tool_result_check}" != "ok" ]]; then
  echo "CASE tool_memory_get FAIL invalid_tool_result response=${memory_get_json}"
  exit 1
fi

echo "CASE tool_memory_get PASS"
echo "DETAIL tool_memory_get_output=${memory_get_output}"
