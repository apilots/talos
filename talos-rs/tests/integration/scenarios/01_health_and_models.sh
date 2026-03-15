#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
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

models_json="$(curl -sS -H "Authorization: Bearer ${AUTH_TOKEN}" "${GATEWAY_URL}/v1/models")"
log_case_json "models_list" "{}" "${models_json}"
model_count="$(python3 -c 'import json,sys; print(len(json.load(sys.stdin).get("data", [])))' <<< "${models_json}")"
if [[ "${model_count}" -lt 1 ]]; then
  echo "CASE models_list FAIL expected=>=1 actual=${model_count}"
  exit 1
fi
echo "CASE models_list PASS count=${model_count}"
