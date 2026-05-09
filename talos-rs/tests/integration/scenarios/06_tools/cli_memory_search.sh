#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command "${ROOT_DIR}/target/debug/talos-client"

if [[ -z "${TALOS_LLM_BAILIAN_API_KEY:-}" && -z "${TALOS_LLM_ANTHROPIC_API_KEY:-}" && -z "${TALOS_LLM_OPENAI_API_KEY:-}" ]]; then
  echo "CASE cli_stream_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE cli_stream_real_llm_precheck PASS key_detected=true"

build_binaries
prepare_talos_home

export TALOS_APP_SERVER_BIN="${ROOT_DIR}/target/debug/talos"
export TALOS_LOCAL_LAUNCH_TOKEN="${TALOS_LOCAL_LAUNCH_TOKEN:-talos-cli-memory-search-token}"

cli="${ROOT_DIR}/target/debug/talos-client"

run_cli() {
  "${cli}" "$@" 2>&1 | sed '/^Command executed successfully$/d'
}

models_json="$(run_cli models --format json)"
primary_model="$(python3 -c 'import json,sys; arr=json.load(sys.stdin); print((arr[0].get("id") if arr else ""))' <<< "${models_json}")"
if [[ -z "${primary_model}" ]]; then
  echo "CASE cli_stream_select_model FAIL models=${models_json}"
  exit 1
fi
echo "CASE cli_stream_select_model PASS model=${primary_model}"

memory_stream_stdout="$(scenario_report_path cli_memory_stream_stdout.log)"
memory_stream_stderr="$(scenario_report_path cli_memory_stream_stderr.log)"
memory_create_output="$(run_cli session create --model "${primary_model}")"
memory_session_id="$(echo "${memory_create_output}" | sed -n 's/^Session created: \([^ ]*\).*/\1/p')"
if [[ -z "${memory_session_id}" ]]; then
  echo "CASE cli_stream_memory_session_create FAIL output=${memory_create_output}"
  exit 1
fi
echo "CASE cli_stream_memory_session_create PASS session_id=${memory_session_id}"

memory_stream_input="You must call MemorySearch exactly once with query talos项目 before any final answer. Do not answer from prior knowledge, prior messages, or workspace context. If no result is found, say so in one short sentence after the tool call."

run_cli chat in "${memory_session_id}" "${memory_stream_input}" --stream \
  >"${memory_stream_stdout}" 2>"${memory_stream_stderr}"

memory_stdout_text="$(cat "${memory_stream_stdout}")"
memory_stderr_text="$(cat "${memory_stream_stderr}")"
memory_events_text="${memory_stdout_text}"$'\n'"${memory_stderr_text}"
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
if ! echo "${memory_events_text}" | grep -q "TOOL_CALL_STARTED name=MemorySearch internal=MemorySearch"; then
  echo "CASE cli_chat_stream_memory_search FAIL missing_tool_started events=${memory_events_text}"
  exit 1
fi
if ! echo "${memory_events_text}" | grep -q "TOOL_CALL_COMPLETED call_id="; then
  echo "CASE cli_chat_stream_memory_search FAIL missing_tool_completed events=${memory_events_text}"
  exit 1
fi
if ! echo "${memory_events_text}" | grep -q "tool=MemorySearch internal=MemorySearch"; then
  echo "CASE cli_chat_stream_memory_search FAIL missing_memory_tool_name events=${memory_events_text}"
  exit 1
fi
memory_call_ids="$(EVENTS_TEXT="${memory_events_text}" python3 - <<'PY'
import os
import re

events = os.environ.get("EVENTS_TEXT", "")
started = re.search(r"TOOL_CALL_STARTED name=MemorySearch internal=MemorySearch call_id=([^ ]*)", events)
completed = re.search(r"TOOL_CALL_COMPLETED call_id=([^ ]+) tool=MemorySearch internal=MemorySearch", events)
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
