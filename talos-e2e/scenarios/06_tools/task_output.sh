#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

load_env
require_command curl
require_command python3

if [[ -z "${TALOS_LLM_BAILIAN_API_KEY:-}" && -z "${TALOS_LLM_ANTHROPIC_API_KEY:-}" && -z "${TALOS_LLM_OPENAI_API_KEY:-}" ]]; then
  echo "CASE task_output_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE task_output_real_llm_precheck PASS key_detected=true"

TASK_MARKER="task-output-marker-$(date '+%Y%m%d%H%M%S')-$$"

build_binaries
start_server
trap stop_server EXIT

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
primary_model="$(select_e2e_model_from_json "${models_json}")"
if [[ -z "${primary_model}" ]]; then
  echo "CASE task_output_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
echo "CASE task_output_select_model PASS model=${primary_model}"

build_session_context() {
  python3 - <<'PY'
import json
import os

payload = {
    "context": {
        "source_type": "http",
        "provider": "http",
        "channel_id": "tool-task-output",
        "user_id": "tool-task-output-user",
        "thread_id": None,
        "message_id": "msg-tool-task-output",
        "trace_id": "trace-tool-task-output",
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
  echo "CASE task_output_session_id FAIL response=${session_json}"
  exit 1
fi
echo "CASE task_output_session_id PASS session_id=${session_id}"

bash_input="Use the Bash tool exactly once. Set command to python3 -c \"import time; print('${TASK_MARKER}', flush=True); time.sleep(20)\". Set run_in_background to true and timeout to 0. After the tool call, respond with exactly TASK_STARTED"
bash_payload="$(build_payload "${bash_input}")"
bash_json="$(send_message_response "${session_id}" "${bash_payload}" "bash")"
log_case_json "tool_task_output_bash" "${bash_payload}" "${bash_json}"

bash_output="$(extract_output_text "${bash_json}")"
if [[ -z "${bash_output}" || "${bash_output}" == Error:* ]]; then
  echo "CASE tool_task_output_bash FAIL response=${bash_json}"
  exit 1
fi
if ! echo "${bash_output}" | grep -Fq "TASK_STARTED"; then
  echo "CASE tool_task_output_bash FAIL missing_ack output=${bash_output}"
  exit 1
fi

task_id="$(BASH_JSON="${bash_json}" TASK_MARKER="${TASK_MARKER}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["BASH_JSON"])
results = [item for item in data.get("output", []) if item.get("type") == "tool_result"]
for bash_result in reversed(results):
    if bash_result.get("tool_name") != "Bash":
        continue
    bash_content = (((bash_result.get("content") or [{}])[0].get("value") or {}).get("value", (bash_result.get("content") or [{}])[0].get("value") or {}))
    if bash_content.get("status") == "running" and bash_content.get("session_id") is not None:
        print(bash_content["session_id"])
        raise SystemExit(0)
raise AssertionError(results)
PY
)"

task_input="Use the TaskOutput tool exactly once with task_id ${task_id} and offset 0. After the tool call, respond with exactly TASK_OUTPUT_OK:${TASK_MARKER}"
task_payload="$(build_payload "${task_input}")"
task_json="$(send_message_response "${session_id}" "${task_payload}" "task")"
log_case_json "tool_task_output" "${task_payload}" "${task_json}"

task_output="$(extract_output_text "${task_json}")"
if [[ -z "${task_output}" || "${task_output}" == Error:* ]]; then
  echo "CASE tool_task_output FAIL response=${task_json}"
  exit 1
fi
if ! echo "${task_output}" | grep -Fq "TASK_OUTPUT_OK:${TASK_MARKER}"; then
  echo "CASE tool_task_output FAIL missing_marker output=${task_output}"
  exit 1
fi

TASK_OUTPUT_JSON="${task_json}" TASK_MARKER="${TASK_MARKER}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["TASK_OUTPUT_JSON"])
results = [item for item in data.get("output", []) if item.get("type") == "tool_result"]
assert results, results
task_result = next(item for item in results if item.get("tool_name") == "TaskOutput")
task_content = (((task_result.get("content") or [{}])[0].get("value") or {}).get("value", (task_result.get("content") or [{}])[0].get("value") or {}))
assert task_content.get("status") in {"running", "exited"}, task_content
assert os.environ["TASK_MARKER"] in (task_content.get("output") or ""), task_content
PY

echo "CASE tool_task_output PASS"
echo "DETAIL tool_task_output_output=${task_output}"
