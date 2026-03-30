#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command "${ROOT_DIR}/target/debug/talos-client"

if [[ -z "${BAILIAN_API_KEY:-}" && -z "${OPENAI_API_KEY:-}" && -z "${ANTHROPIC_API_KEY:-}" ]]; then
  echo "CASE cli_stream_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE cli_stream_real_llm_precheck PASS key_detected=true"

start_server
trap stop_server EXIT

cli="${ROOT_DIR}/target/debug/talos-client"
models_json="$(curl -sS -H "Authorization: Bearer ${AUTH_TOKEN}" "${GATEWAY_URL}/v1/models")"
primary_model="$(python3 -c 'import json,sys; data=json.load(sys.stdin); arr=data.get("data",[]); print((arr[0].get("id") if arr else ""))' <<< "${models_json}")"
if [[ -z "${primary_model}" ]]; then
  echo "CASE cli_stream_select_model FAIL models=${models_json}"
  exit 1
fi
echo "CASE cli_stream_select_model PASS model=${primary_model}"

create_output="$("${cli}" --gateway-url "${GATEWAY_URL}" --auth-token "${AUTH_TOKEN}" session create --model "${primary_model}")"
session_id="$(echo "${create_output}" | sed -n 's/^Session created: \([^ ]*\).*/\1/p')"
if [[ -z "${session_id}" ]]; then
  echo "CASE cli_stream_session_create FAIL output=${create_output}"
  exit 1
fi
echo "CASE cli_stream_session_create PASS session_id=${session_id}"

stream_stdout="${ROOT_DIR}/tests/integration/reports/cli_stream_stdout.log"
stream_stderr="${ROOT_DIR}/tests/integration/reports/cli_stream_stderr.log"
stream_input="You must call exec tool exactly once with command ls and return raw output inside <tool_output></tool_output>."

"${cli}" --gateway-url "${GATEWAY_URL}" --auth-token "${AUTH_TOKEN}" \
  chat in "${session_id}" "${stream_input}" --stream \
  >"${stream_stdout}" 2>"${stream_stderr}"

stdout_text="$(cat "${stream_stdout}")"
stderr_text="$(cat "${stream_stderr}")"
response_json="$(STDOUT_TEXT="${stdout_text}" STDERR_TEXT="${stderr_text}" python3 - <<'PY'
import json
import os
print(json.dumps({
    "stdout": os.environ.get("STDOUT_TEXT", ""),
    "stderr": os.environ.get("STDERR_TEXT", ""),
}, ensure_ascii=False))
PY
)"
log_case_json "cli_chat_stream_tool_exec" "{\"session_id\":\"${session_id}\",\"model\":\"${primary_model}\",\"input\":\"${stream_input}\"}" "${response_json}"

if [[ -z "${stdout_text}" ]]; then
  echo "CASE cli_chat_stream_tool_exec FAIL empty_stdout"
  exit 1
fi
if ! echo "${stdout_text}" | grep -q "Cargo.toml"; then
  echo "CASE cli_chat_stream_tool_exec FAIL missing=Cargo.toml stdout=${stdout_text}"
  exit 1
fi
if ! echo "${stderr_text}" | grep -Eq "THINKING\\[|SUMMARY\\["; then
  echo "CASE cli_chat_stream_tool_exec FAIL missing_thinking_or_summary_event stderr=${stderr_text}"
  exit 1
fi
if ! echo "${stderr_text}" | grep -q "TOOL_CALL_STARTED name=exec"; then
  echo "CASE cli_chat_stream_tool_exec FAIL missing_tool_started stderr=${stderr_text}"
  exit 1
fi
if ! echo "${stderr_text}" | grep -q "TOOL_CALL_COMPLETED"; then
  echo "CASE cli_chat_stream_tool_exec FAIL missing_tool_completed stderr=${stderr_text}"
  exit 1
fi

echo "CASE cli_chat_stream_tool_exec PASS"
echo "DETAIL cli_chat_stream_tool_exec_stdout=${stdout_text}"
echo "DETAIL cli_chat_stream_tool_exec_stderr=${stderr_text}"
