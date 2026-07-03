#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

load_env
require_command "${TALOS_RS_DIR}/target/debug/talos"

if [[ -z "${TALOS_LLM_BAILIAN_API_KEY:-}" && -z "${TALOS_LLM_ANTHROPIC_API_KEY:-}" && -z "${TALOS_LLM_OPENAI_API_KEY:-}" ]]; then
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

session_id="$(new_session_id)"
echo "CASE cli_stream_session_id PASS session_id=${session_id}"

stream_stdout="$(scenario_report_path cli_stream_stdout.log)"
stream_stderr="$(scenario_report_path cli_stream_stderr.log)"
stream_input="You must call tool 'Bash' exactly once before any final answer. Use command ls. Do not call any other tool in this turn. Do not answer from prior knowledge, prior messages, or workspace context. After the Bash tool result is returned, return the raw stdout inside <tool_output></tool_output>."

run_cli exec --session "${session_id}" --model "${primary_model}" "${stream_input}" \
  >"${stream_stdout}" 2>"${stream_stderr}"

stdout_text="$(cat "${stream_stdout}")"
stderr_text="$(cat "${stream_stderr}")"
events_text="${stdout_text}"$'\n'"${stderr_text}"
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
if ! echo "${events_text}" | grep -Eq "THINKING item=|SUMMARY "; then
  echo "CASE cli_chat_stream_tool_exec FAIL missing_thinking_or_summary_event events=${events_text}"
  exit 1
fi
if ! echo "${events_text}" | grep -q "TOOL_CALL_STARTED name=Bash internal=Bash"; then
  echo "CASE cli_chat_stream_tool_exec FAIL missing_tool_started events=${events_text}"
  exit 1
fi
if ! echo "${events_text}" | grep -q "TOOL_CALL_RESULT item="; then
  echo "CASE cli_chat_stream_tool_exec FAIL missing_tool_result events=${events_text}"
  exit 1
fi
exec_call_ids="$(EVENTS_TEXT="${events_text}" python3 - <<'PY'
import os
import re

events = os.environ.get("EVENTS_TEXT", "")
started = re.search(r"TOOL_CALL_STARTED name=Bash internal=Bash call_id=([^ ]*) item=([^\n ]+)", events)
result = re.search(r"TOOL_CALL_RESULT item=([^\n ]+) is_error=false", events)
print(f"{started.group(1) if started else ''}\t{started.group(2) if started else ''}\t{result.group(1) if result else ''}")
PY
)"
exec_started_call_id="${exec_call_ids%%$'\t'*}"
exec_remaining="${exec_call_ids#*$'\t'}"
exec_started_item_id="${exec_remaining%%$'\t'*}"
exec_result_item_id="${exec_remaining#*$'\t'}"
if [[ -z "${exec_started_call_id}" || -z "${exec_started_item_id}" || -z "${exec_result_item_id}" ]]; then
  echo "CASE cli_chat_stream_tool_exec FAIL missing_tool_identity call_id=${exec_started_call_id} started_item=${exec_started_item_id} result_item=${exec_result_item_id}"
  exit 1
fi
if [[ "${exec_started_item_id}" != "${exec_result_item_id}" ]]; then
  echo "CASE cli_chat_stream_tool_exec FAIL item_id_mismatch started=${exec_started_item_id} result=${exec_result_item_id}"
  exit 1
fi

echo "CASE cli_chat_stream_tool_exec PASS"
echo "DETAIL cli_chat_stream_tool_exec_stdout=${stdout_text}"
echo "DETAIL cli_chat_stream_tool_exec_stderr=${stderr_text}"
