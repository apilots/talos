#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command curl
require_command python3

if [[ -z "${TALOS_LLM_BAILIAN_API_KEY:-}" && -z "${TALOS_LLM_ANTHROPIC_API_KEY:-}" && -z "${TALOS_LLM_OPENAI_API_KEY:-}" ]]; then
  echo "CASE task_stop_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE task_stop_real_llm_precheck PASS key_detected=true"

build_binaries
start_server
trap stop_server EXIT

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
primary_model="$(select_e2e_model_from_json "${models_json}")"
if [[ -z "${primary_model}" ]]; then
  echo "CASE task_stop_select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
echo "CASE task_stop_select_model PASS model=${primary_model}"

build_session_context() {
  python3 - <<'PY'
import json
import os

payload = {
    "context": {
        "source_type": "http",
        "provider": "http",
        "channel_id": "tool-task-stop",
        "user_id": "tool-task-stop-user",
        "thread_id": None,
        "message_id": "msg-tool-task-stop",
        "trace_id": "trace-tool-task-stop",
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
  echo "CASE task_stop_session_id FAIL response=${session_json}"
  exit 1
fi
echo "CASE task_stop_session_id PASS session_id=${session_id}"

task_input="You must first call tool 'Bash' exactly once with command python3 -c \"import time; time.sleep(10)\" and run_in_background true. Then call tool 'TaskStop' exactly once using the returned task id. After both tool calls, respond with exactly TASK_STOP_OK"
task_payload="$(build_payload "${task_input}")"
task_json="$(send_message_response "${session_id}" "${task_payload}" "task")"
log_case_json "tool_task_stop" "${task_payload}" "${task_json}"

task_output="$(extract_output_text "${task_json}")"
if [[ -z "${task_output}" || "${task_output}" == Error:* ]]; then
  echo "CASE tool_task_stop FAIL response=${task_json}"
  exit 1
fi
if ! echo "${task_output}" | grep -Fq "TASK_STOP_OK"; then
  echo "CASE tool_task_stop FAIL missing_ack output=${task_output}"
  exit 1
fi

TASK_STOP_JSON="${task_json}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["TASK_STOP_JSON"])
results = [item for item in data.get("output", []) if item.get("type") == "tool_result"]
assert len(results) >= 2, results
bash_result = next(item for item in results if item.get("tool_name") == "Bash")
task_result = next(item for item in results if item.get("tool_name") == "TaskStop")

bash_content = (((bash_result.get("content") or [{}])[0].get("value") or {}).get("value", (bash_result.get("content") or [{}])[0].get("value") or {}))
assert bash_content.get("session_id") is not None, bash_content
assert bash_content.get("status") in {"running", "exited"}, bash_content

task_content = (((task_result.get("content") or [{}])[0].get("value") or {}).get("value", (task_result.get("content") or [{}])[0].get("value") or {}))
assert task_content.get("stopped") is True, task_content
assert task_content.get("task_id") is not None, task_content
PY

echo "CASE tool_task_stop PASS"
echo "DETAIL tool_task_stop_output=${task_output}"
