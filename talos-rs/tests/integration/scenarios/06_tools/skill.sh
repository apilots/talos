#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command curl
require_command python3

if [[ -z "${BAILIAN_API_KEY:-}" && -z "${BAILIAN_CODINGPLAN_API_KEY:-}" && -z "${OPENAI_API_KEY:-}" && -z "${ANTHROPIC_API_KEY:-}" ]]; then
  echo "CASE skill_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE skill_real_llm_precheck PASS key_detected=true"

build_binaries
start_server
trap stop_server EXIT

models_json="$(curl -sS -H "Authorization: Bearer ${AUTH_TOKEN}" "${GATEWAY_URL}/v1/models")"
primary_model="$(python3 -c 'import json,sys; data=json.load(sys.stdin); arr=data.get("data",[]); print((arr[0].get("id") if arr else ""))' <<< "${models_json}")"
if [[ -z "${primary_model}" ]]; then
  echo "CASE skill_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
echo "CASE skill_select_model PASS model=${primary_model}"

create_session() {
  python3 - <<'PY'
import json
import os

payload = {
    "context": {
        "source_type": "http",
        "provider": "http",
        "channel_id": "tool-skill",
        "user_id": "tool-skill-user",
        "thread_id": None,
        "message_id": "msg-tool-skill",
        "trace_id": "trace-tool-skill",
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
    "stream": False,
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
  echo "CASE skill_create_session FAIL response=${session_json}"
  exit 1
fi
echo "CASE skill_create_session PASS session_id=${session_id}"

skill_input="You must call tool 'Skill' exactly once with query write. After the tool call, respond with exactly SKILL_TOOL_OK"
discover_input="You must call tool 'ToolSearch' exactly once with query Skill. After the tool call, respond with exactly SKILL_DISCOVERED"
discover_payload="$(build_payload "${discover_input}")"
discover_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions/${session_id}/messages" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${discover_payload}")"
log_case_json "tool_skill_discovery" "${discover_payload}" "${discover_json}"

discover_output="$(extract_output_text "${discover_json}")"
if [[ -z "${discover_output}" || "${discover_output}" == Error:* ]]; then
  echo "CASE tool_skill_discovery FAIL response=${discover_json}"
  exit 1
fi
if ! echo "${discover_output}" | grep -Fq "SKILL_DISCOVERED"; then
  echo "CASE tool_skill_discovery FAIL missing_ack output=${discover_output}"
  exit 1
fi

DISCOVER_JSON="${discover_json}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["DISCOVER_JSON"])
results = [item for item in data.get("output", []) if item.get("type") == "tool_result"]
assert results, "missing tool_result"
result = results[0]
assert result.get("tool_name") == "ToolSearch", result.get("tool_name")
content_items = result.get("content") or []
assert content_items, "missing content payload"
content = content_items[0].get("value") or {}
matches = content.get("matches") or []
assert any(match.get("public_name") == "Skill" for match in matches), matches
PY

skill_input="You must call tool 'Skill' exactly once with query write. After the tool call, respond with exactly SKILL_TOOL_OK"
skill_payload="$(build_payload "${skill_input}")"
skill_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions/${session_id}/messages" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -d "${skill_payload}")"
log_case_json "tool_skill" "${skill_payload}" "${skill_json}"

skill_output="$(extract_output_text "${skill_json}")"
if [[ -z "${skill_output}" || "${skill_output}" == Error:* ]]; then
  echo "CASE tool_skill FAIL response=${skill_json}"
  exit 1
fi
if ! echo "${skill_output}" | grep -Fq "SKILL_TOOL_OK"; then
  echo "CASE tool_skill FAIL missing_ack output=${skill_output}"
  exit 1
fi

SKILL_JSON="${skill_json}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["SKILL_JSON"])
results = [item for item in data.get("output", []) if item.get("type") == "tool_result"]
assert results, "missing tool_result"
result = results[0]
assert result.get("tool_name") == "Skill", result.get("tool_name")
content_items = result.get("content") or []
assert content_items, "missing content payload"
content = content_items[0].get("value") or {}
skills = content.get("skills")
assert isinstance(skills, list), content
for item in skills:
    assert item.get("public_name", "").startswith("skill_"), item
PY

echo "CASE tool_skill PASS"
echo "DETAIL tool_skill_output=${skill_output}"
