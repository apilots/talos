#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../scripts/common.sh"

require_command curl

start_server
trap stop_server EXIT

health_json="$(curl -sS "${HEALTH_URL}")"
log_case_json "health_check" "{}" "${health_json}"
status="$(json_get "${health_json}" "status")"
if [[ "${status}" != "healthy" ]]; then
  echo "CASE health_check FAIL expected=healthy actual=${status}"
  exit 1
fi
echo "CASE health_check PASS status=${status}"

models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
log_case_json "models_list" "{}" "${models_json}"
models_data_json="$(api_data_json "${models_json}")"
model_count="$(python3 -c 'import json,sys; payload=json.load(sys.stdin); data=payload.get("models", []); print(len(data))' <<< "${models_data_json}")"
first_model_name="$(python3 -c 'import json,sys; payload=json.load(sys.stdin); data=payload.get("models", []); print((data[0].get("name") if data else ""))' <<< "${models_data_json}")"
first_model_context_window="$(python3 -c 'import json,sys; payload=json.load(sys.stdin); data=payload.get("models", []); print((data[0].get("context_window") if data else 0))' <<< "${models_data_json}")"
provider_count="$(python3 -c 'import json,sys; payload=json.load(sys.stdin); data=payload.get("models", []); print(len({item.get("provider") for item in data if item.get("provider")}))' <<< "${models_data_json}")"
ollama_models="$(python3 -c 'import json,sys; payload=json.load(sys.stdin); data=payload.get("models", []); print(",".join(item.get("id", "") for item in data if item.get("provider") == "ollama"))' <<< "${models_data_json}")"
if [[ "${model_count}" -lt 1 ]]; then
  echo "CASE models_list FAIL expected=>=1 actual=${model_count}"
  exit 1
fi
if [[ -z "${first_model_name}" || "${first_model_context_window}" -le 0 ]]; then
  echo "CASE models_list FAIL missing_model_metadata response=${models_json}"
  exit 1
fi
if [[ "${provider_count}" -lt 2 ]]; then
  echo "CASE models_list FAIL expected_multiple_providers actual=${provider_count} response=${models_json}"
  exit 1
fi
for expected_ollama_model in "qwen3:8b" "deepseek-r1:8b" "gpt-oss:20b"; do
  if [[ ",${ollama_models}," != *",${expected_ollama_model},"* ]]; then
    echo "CASE models_list FAIL missing_configured_ollama_model=${expected_ollama_model} response=${models_json}"
    exit 1
  fi
done
echo "CASE models_list PASS count=${model_count} providers=${provider_count} ollama=${ollama_models}"
