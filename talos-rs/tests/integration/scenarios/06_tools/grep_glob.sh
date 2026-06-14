#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command curl
require_command python3
require_command mktemp

if [[ -z "${TALOS_LLM_BAILIAN_API_KEY:-}" && -z "${TALOS_LLM_ANTHROPIC_API_KEY:-}" && -z "${TALOS_LLM_OPENAI_API_KEY:-}" ]]; then
  echo "CASE grep_glob_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE grep_glob_real_llm_precheck PASS key_detected=true"

TEST_TALOS_HOME="$(mktemp -d "${TMPDIR:-/tmp}/talos-tool-grep-glob-home.XXXXXX")"
cleanup() {
  stop_server
  rm -rf "${TEST_TALOS_HOME}"
}
export TALOS_HOME="${TEST_TALOS_HOME}"

WORKSPACE_DIR="${TALOS_HOME}/workspace-talos"
DOC_DIR="${WORKSPACE_DIR}/docs"
SRC_DIR="${WORKSPACE_DIR}/src"
mkdir -p "${DOC_DIR}" "${SRC_DIR}"
GLOB_FILE="docs/alpha.md"
GREP_FILE="src/search_target.txt"
GREP_MARKER="grep-marker-$(date '+%Y%m%d%H%M%S')-$$"

cat > "${WORKSPACE_DIR}/${GLOB_FILE}" <<'EOF'
# Alpha
glob fixture
EOF
cat > "${WORKSPACE_DIR}/${GREP_FILE}" <<EOF
search fixture
${GREP_MARKER}
EOF

build_binaries
start_server
trap cleanup EXIT

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
primary_model="$(select_e2e_model_from_json "${models_json}")"
if [[ -z "${primary_model}" ]]; then
  echo "CASE grep_glob_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
echo "CASE grep_glob_select_model PASS model=${primary_model}"

create_session() {
  local channel="$1"
  local user="$2"
  CHANNEL_ID="${channel}" USER_ID="${user}" python3 - <<'PY'
import json
import os

payload = {
    "context": {
        "source_type": "http",
        "provider": "http",
        "channel_id": os.environ["CHANNEL_ID"],
        "user_id": os.environ["USER_ID"],
        "thread_id": None,
        "message_id": "msg-tool-grep-glob",
        "trace_id": "trace-tool-grep-glob",
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

glob_session_payload="$(create_session "tool-glob" "tool-glob-user")"
glob_session_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions" \
  -H "Content-Type: application/json" \
  -d "${glob_session_payload}")"
glob_session_id="$(session_id_from_json "${glob_session_json}")"
if [[ -z "${glob_session_id}" ]]; then
  echo "CASE tool_glob_create_session FAIL response=${glob_session_json}"
  exit 1
fi
echo "CASE tool_glob_create_session PASS session_id=${glob_session_id}"

glob_input="You must call tool 'Glob' exactly once with pattern docs/*.md. After the tool call, respond with exactly GLOB_OK:${GLOB_FILE}"
glob_payload="$(build_payload "${glob_input}")"
glob_json="$(send_message_response "${glob_session_id}" "${glob_payload}" "glob")"
log_case_json "tool_glob" "${glob_payload}" "${glob_json}"
glob_output="$(extract_output_text "${glob_json}")"
if [[ -z "${glob_output}" || "${glob_output}" == Error:* ]]; then
  echo "CASE tool_glob FAIL response=${glob_json}"
  exit 1
fi
if ! echo "${glob_output}" | grep -Fq "GLOB_OK:${GLOB_FILE}"; then
  echo "CASE tool_glob FAIL missing_path output=${glob_output}"
  exit 1
fi
glob_tool_result="$(python3 -c 'import json,sys; d=json.load(sys.stdin); results=[i for i in d.get("output", []) if i.get("type")=="tool_result"]; print(results[0].get("tool_name","") if results else "")' <<< "${glob_json}")"
if [[ "${glob_tool_result}" != "Glob" ]]; then
  echo "CASE tool_glob FAIL unexpected_tool result=${glob_tool_result}"
  exit 1
fi
echo "CASE tool_glob PASS"
echo "DETAIL tool_glob_output=${glob_output}"

grep_session_payload="$(create_session "tool-grep" "tool-grep-user")"
grep_session_json="$(curl -sS -X POST "${GATEWAY_URL}/v1/sessions" \
  -H "Content-Type: application/json" \
  -d "${grep_session_payload}")"
grep_session_id="$(session_id_from_json "${grep_session_json}")"
if [[ -z "${grep_session_id}" ]]; then
  echo "CASE tool_grep_create_session FAIL response=${grep_session_json}"
  exit 1
fi
echo "CASE tool_grep_create_session PASS session_id=${grep_session_id}"

grep_input="You must call tool 'Grep' exactly once with pattern ${GREP_MARKER} and path src. After the tool call, respond with exactly GREP_OK:${GREP_FILE}"
grep_payload="$(build_payload "${grep_input}")"
grep_json="$(send_message_response "${grep_session_id}" "${grep_payload}" "grep")"
log_case_json "tool_grep" "${grep_payload}" "${grep_json}"
grep_output="$(extract_output_text "${grep_json}")"
if [[ -z "${grep_output}" || "${grep_output}" == Error:* ]]; then
  echo "CASE tool_grep FAIL response=${grep_json}"
  exit 1
fi
if ! echo "${grep_output}" | grep -Fq "GREP_OK:${GREP_FILE}"; then
  echo "CASE tool_grep FAIL missing_path output=${grep_output}"
  exit 1
fi
grep_tool_result="$(python3 -c 'import json,sys; d=json.load(sys.stdin); results=[i for i in d.get("output", []) if i.get("type")=="tool_result"]; print(results[0].get("tool_name","") if results else "")' <<< "${grep_json}")"
if [[ "${grep_tool_result}" != "Grep" ]]; then
  echo "CASE tool_grep FAIL unexpected_tool result=${grep_tool_result}"
  exit 1
fi
echo "CASE tool_grep PASS"
echo "DETAIL tool_grep_output=${grep_output}"
