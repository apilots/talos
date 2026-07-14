#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

require_command "${TALOS_RS_DIR}/target/debug/talos"

if ! has_real_llm_key; then
  echo "CASE cli_stream_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE cli_stream_real_llm_precheck PASS key_detected=true"

build_binaries
prepare_talos_home

cli="${TALOS_RS_DIR}/target/debug/talos"

run_cli() {
  "${cli}" "$@" 2>&1
}

models_json="$(run_cli models --format json)"
primary_model="$(select_e2e_model_from_json "${models_json}")"
primary_provider="$(select_e2e_model_provider_from_json "${models_json}" "${primary_model}")"
if [[ -z "${primary_model}" || -z "${primary_provider}" ]]; then
  echo "CASE cli_stream_select_model FAIL models=${models_json}"
  exit 1
fi
echo "CASE cli_stream_select_model PASS model=${primary_model}"

memory_stream_stdout="$(scenario_report_path cli_memory_stream_stdout.log)"
memory_stream_stderr="$(scenario_report_path cli_memory_stream_stderr.log)"
memory_session_id="$(new_session_id)"
echo "CASE cli_stream_memory_session_id PASS session_id=${memory_session_id}"

memory_stream_input="You must call MemorySearch exactly once with query talos项目 before any final answer. Do not answer from prior knowledge, prior messages, or workspace context. If no result is found, say so in one short sentence after the tool call."

run_cli exec --session "${memory_session_id}" --model "${primary_model}" "${memory_stream_input}" \
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
if ! echo "${memory_events_text}" | grep -q "TOOL_CALL_RESULT item="; then
  echo "CASE cli_chat_stream_memory_search FAIL missing_tool_result events=${memory_events_text}"
  exit 1
fi
if ! echo "${memory_events_text}" | grep -q "TOOL_CALL_STARTED name=MemorySearch internal=MemorySearch"; then
  echo "CASE cli_chat_stream_memory_search FAIL missing_memory_tool_name events=${memory_events_text}"
  exit 1
fi
memory_call_ids="$(EVENTS_TEXT="${memory_events_text}" python3 - <<'PY'
import os
import re

events = os.environ.get("EVENTS_TEXT", "")
started = re.search(r"TOOL_CALL_STARTED name=MemorySearch internal=MemorySearch call_id=([^ ]*) item=([^\n ]+)", events)
started_item = started.group(2) if started else ""
result_item = ""
if started_item and re.search(rf"TOOL_CALL_RESULT item={re.escape(started_item)} is_error=false", events):
    result_item = started_item
print(f"{started.group(1) if started else ''}\t{started_item}\t{result_item}")
PY
)"
memory_started_call_id="${memory_call_ids%%$'\t'*}"
memory_remaining="${memory_call_ids#*$'\t'}"
memory_started_item_id="${memory_remaining%%$'\t'*}"
memory_result_item_id="${memory_remaining#*$'\t'}"
if [[ -z "${memory_started_call_id}" || -z "${memory_started_item_id}" || -z "${memory_result_item_id}" ]]; then
  echo "CASE cli_chat_stream_memory_search FAIL missing_tool_identity call_id=${memory_started_call_id} started_item=${memory_started_item_id} result_item=${memory_result_item_id}"
  exit 1
fi
if [[ "${memory_started_item_id}" != "${memory_result_item_id}" ]]; then
  echo "CASE cli_chat_stream_memory_search FAIL item_id_mismatch started=${memory_started_item_id} result=${memory_result_item_id}"
  exit 1
fi

echo "CASE cli_chat_stream_memory_search PASS"
echo "DETAIL cli_chat_stream_memory_search_stdout=${memory_stdout_text}"
echo "DETAIL cli_chat_stream_memory_search_stderr=${memory_stderr_text}"
