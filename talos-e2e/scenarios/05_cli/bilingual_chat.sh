#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

load_env
require_command curl

if ! has_real_llm_key; then
  echo "CASE real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE real_llm_precheck PASS key_detected=true"

build_binaries
start_server
trap stop_server EXIT

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
primary_model="$(select_e2e_model_from_json "${models_json}")"
primary_provider="$(select_e2e_model_provider_from_json "${models_json}" "${primary_model}")"
if [[ -z "${primary_model}" || -z "${primary_provider}" ]]; then
  echo "CASE select_model FAIL models=${models_json}"
  exit 1
fi
export PRIMARY_MODEL="${primary_model}"
export PRIMARY_PROVIDER="${primary_provider}"
echo "CASE select_model PASS model=${primary_model}"

build_payload() {
  local input_text="$1"
  INPUT_TEXT="$input_text" python3 - <<'PY'
import json
import os

payload = {
    "input": [
        {
            "type": "text",
            "text": os.environ["INPUT_TEXT"]
        }
    ],
    "workspace_id": None,
}
print(json.dumps(payload, ensure_ascii=False))
PY
}

build_session_context() {
  local user_id="$1"
  USER_ID="$user_id" python3 - <<'PY'
import json
import os

payload = {
    "context": {
        "source_type": "http",
        "provider": "http",
        "channel_id": "bilingual-tools",
        "user_id": os.environ["USER_ID"],
        "thread_id": None,
        "message_id": "msg-bilingual-tools",
        "trace_id": "trace-bilingual-tools",
    },
    "modelSelection": {"modelRef": {"provider": os.environ["PRIMARY_PROVIDER"], "id": os.environ["PRIMARY_MODEL"]}, "reasoning": {"type": "modelDefault"}},
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

en_session_payload="$(build_session_context "lang-user-en")"
en_session_id="$(new_session_id)"
remember_session_context "${en_session_id}" "${en_session_payload}"
en_session_json="{\"id\":\"${en_session_id}\"}"
en_session_id="$(session_id_from_json "${en_session_json}")"
if [[ -z "${en_session_id}" ]]; then
  echo "CASE messages_english_session_id FAIL response=${en_session_json}"
  exit 1
fi
echo "CASE messages_english_session_id PASS session_id=${en_session_id}"

english_input="Please respond in English with one short sentence containing 'english-ok'."
english_payload="$(build_payload "${english_input}")"
english_json="$(send_message_response "${en_session_id}" "${english_payload}" "english")"
log_case_json "messages_english" "${english_payload}" "${english_json}"
english_output="$(extract_output_text "${english_json}")"
english_stop="$(json_get "${english_json}" "stop_reason")"
if [[ -z "${english_output}" || "${english_stop}" == "error" || "${english_output}" == Error:* ]]; then
  echo "CASE messages_english FAIL response=${english_json}"
  exit 1
fi
if ! echo "${english_output}" | grep -qi "english-ok"; then
  echo "CASE messages_english FAIL missing=english-ok output=${english_output}"
  exit 1
fi
echo "CASE messages_english PASS"
echo "DETAIL messages_english_output=${english_output}"

zh_session_payload="$(build_session_context "lang-user-zh")"
zh_session_id="$(new_session_id)"
remember_session_context "${zh_session_id}" "${zh_session_payload}"
zh_session_json="{\"id\":\"${zh_session_id}\"}"
zh_session_id="$(session_id_from_json "${zh_session_json}")"
if [[ -z "${zh_session_id}" ]]; then
  echo "CASE messages_chinese_session_id FAIL response=${zh_session_json}"
  exit 1
fi
echo "CASE messages_chinese_session_id PASS session_id=${zh_session_id}"

chinese_input="请用中文回复一句话，并包含“中文通过”四个字。"
chinese_payload="$(build_payload "${chinese_input}")"
chinese_json="$(send_message_response "${zh_session_id}" "${chinese_payload}" "chinese")"
log_case_json "messages_chinese" "${chinese_payload}" "${chinese_json}"
chinese_output="$(extract_output_text "${chinese_json}")"
chinese_stop="$(json_get "${chinese_json}" "stop_reason")"
if [[ -z "${chinese_output}" || "${chinese_stop}" == "error" || "${chinese_output}" == Error:* ]]; then
  echo "CASE messages_chinese FAIL response=${chinese_json}"
  exit 1
fi
if ! echo "${chinese_output}" | grep -q "中文通过"; then
  echo "CASE messages_chinese FAIL missing=中文通过 output=${chinese_output}"
  exit 1
fi
echo "CASE messages_chinese PASS"
echo "DETAIL messages_chinese_output=${chinese_output}"
