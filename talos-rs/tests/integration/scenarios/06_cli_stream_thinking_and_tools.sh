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

build_binaries
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
stream_input="You must call exec_command tool exactly once with cmd ls and return raw output inside <tool_output></tool_output>."

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
if ! echo "${stderr_text}" | grep -Eq "THINKING\\[|SUMMARY\\["; then
  echo "CASE cli_chat_stream_tool_exec FAIL missing_thinking_or_summary_event stderr=${stderr_text}"
  exit 1
fi
if ! echo "${stderr_text}" | grep -q "TOOL_CALL_STARTED name=exec_command"; then
  echo "CASE cli_chat_stream_tool_exec FAIL missing_tool_started stderr=${stderr_text}"
  exit 1
fi
if ! echo "${stderr_text}" | grep -q "TOOL_CALL_COMPLETED"; then
  echo "CASE cli_chat_stream_tool_exec FAIL missing_tool_completed stderr=${stderr_text}"
  exit 1
fi
exec_call_ids="$(STDERR_TEXT="${stderr_text}" python3 - <<'PY'
import os
import re

stderr = os.environ.get("STDERR_TEXT", "")
started = re.search(r"TOOL_CALL_STARTED name=exec_command call_id=([^ ]*)", stderr)
completed = re.search(r"TOOL_CALL_COMPLETED call_id=([^ ]+) tool=exec_command", stderr)
print(f"{started.group(1) if started else ''}\t{completed.group(1) if completed else ''}")
PY
)"
exec_started_call_id="${exec_call_ids%%$'\t'*}"
exec_completed_call_id="${exec_call_ids#*$'\t'}"
if [[ -z "${exec_started_call_id}" || -z "${exec_completed_call_id}" ]]; then
  echo "CASE cli_chat_stream_tool_exec FAIL missing_call_id started=${exec_started_call_id} completed=${exec_completed_call_id}"
  exit 1
fi
if [[ "${exec_started_call_id}" != "${exec_completed_call_id}" ]]; then
  echo "CASE cli_chat_stream_tool_exec FAIL call_id_mismatch started=${exec_started_call_id} completed=${exec_completed_call_id}"
  exit 1
fi

echo "CASE cli_chat_stream_tool_exec PASS"
echo "DETAIL cli_chat_stream_tool_exec_stdout=${stdout_text}"
echo "DETAIL cli_chat_stream_tool_exec_stderr=${stderr_text}"

memory_stream_stdout="${ROOT_DIR}/tests/integration/reports/cli_memory_stream_stdout.log"
memory_stream_stderr="${ROOT_DIR}/tests/integration/reports/cli_memory_stream_stderr.log"
memory_create_output="$("${cli}" --gateway-url "${GATEWAY_URL}" --auth-token "${AUTH_TOKEN}" session create --model "${primary_model}")"
memory_session_id="$(echo "${memory_create_output}" | sed -n 's/^Session created: \([^ ]*\).*/\1/p')"
if [[ -z "${memory_session_id}" ]]; then
  echo "CASE cli_stream_memory_session_create FAIL output=${memory_create_output}"
  exit 1
fi
echo "CASE cli_stream_memory_session_create PASS session_id=${memory_session_id}"

memory_stream_input="You must call memory_search tool exactly once with query talos项目 before any final answer. Do not answer from prior knowledge, prior messages, or workspace context. If no result is found, say so in one short sentence after the tool call."

"${cli}" --gateway-url "${GATEWAY_URL}" --auth-token "${AUTH_TOKEN}" \
  chat in "${memory_session_id}" "${memory_stream_input}" --stream \
  >"${memory_stream_stdout}" 2>"${memory_stream_stderr}"

memory_stdout_text="$(cat "${memory_stream_stdout}")"
memory_stderr_text="$(cat "${memory_stream_stderr}")"
memory_response_json="$(STDOUT_TEXT="${memory_stdout_text}" STDERR_TEXT="${memory_stderr_text}" python3 - <<'PY'
import json
import os
print(json.dumps({
    "stdout": os.environ.get("STDOUT_TEXT", ""),
    "stderr": os.environ.get("STDERR_TEXT", ""),
}, ensure_ascii=False))
PY
)"
log_case_json "cli_chat_stream_memory_search" "{\"session_id\":\"${memory_session_id}\",\"model\":\"${primary_model}\",\"input\":\"${memory_stream_input}\"}" "${memory_response_json}"

if [[ -z "${memory_stdout_text}" ]]; then
  echo "CASE cli_chat_stream_memory_search FAIL empty_stdout"
  exit 1
fi
if ! echo "${memory_stderr_text}" | grep -q "TOOL_CALL_STARTED name=memory_search"; then
  echo "CASE cli_chat_stream_memory_search FAIL missing_tool_started stderr=${memory_stderr_text}"
  exit 1
fi
if ! echo "${memory_stderr_text}" | grep -q "TOOL_CALL_COMPLETED call_id="; then
  echo "CASE cli_chat_stream_memory_search FAIL missing_tool_completed stderr=${memory_stderr_text}"
  exit 1
fi
if ! echo "${memory_stderr_text}" | grep -q "tool=memory_search"; then
  echo "CASE cli_chat_stream_memory_search FAIL missing_memory_tool_name stderr=${memory_stderr_text}"
  exit 1
fi
memory_call_ids="$(STDERR_TEXT="${memory_stderr_text}" python3 - <<'PY'
import os
import re

stderr = os.environ.get("STDERR_TEXT", "")
started = re.search(r"TOOL_CALL_STARTED name=memory_search call_id=([^ ]*)", stderr)
completed = re.search(r"TOOL_CALL_COMPLETED call_id=([^ ]+) tool=memory_search", stderr)
print(f"{started.group(1) if started else ''}\t{completed.group(1) if completed else ''}")
PY
)"
memory_started_call_id="${memory_call_ids%%$'\t'*}"
memory_completed_call_id="${memory_call_ids#*$'\t'}"
if [[ -z "${memory_started_call_id}" || -z "${memory_completed_call_id}" ]]; then
  echo "CASE cli_chat_stream_memory_search FAIL missing_call_id started=${memory_started_call_id} completed=${memory_completed_call_id}"
  exit 1
fi
if [[ "${memory_started_call_id}" != "${memory_completed_call_id}" ]]; then
  echo "CASE cli_chat_stream_memory_search FAIL call_id_mismatch started=${memory_started_call_id} completed=${memory_completed_call_id}"
  exit 1
fi

echo "CASE cli_chat_stream_memory_search PASS"
echo "DETAIL cli_chat_stream_memory_search_stdout=${memory_stdout_text}"
echo "DETAIL cli_chat_stream_memory_search_stderr=${memory_stderr_text}"
