#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command curl

export TALOS_HOME="$(scenario_report_path talos-home)"
rm -rf "${TALOS_HOME}"

request_json() {
  local method="$1"
  local url="$2"
  local body="${3:-}"
  local token="${4:-}"
  local response_file
  response_file="$(mktemp)"

  local -a args=(-sS -X "${method}" -o "${response_file}" -w "%{http_code}")
  if [[ -n "${body}" ]]; then
    args+=(-H "Content-Type: application/json" -d "${body}")
  fi
  if [[ -n "${token}" ]]; then
    args+=(-H "Authorization: Bearer ${token}")
  fi
  args+=("${url}")

  HTTP_STATUS="$(curl "${args[@]}")"
  HTTP_BODY="$(cat "${response_file}")"
  rm -f "${response_file}"
}

start_server
trap stop_server EXIT

# CASE: anonymous auth-disabled mode can access llm config in local integration tests
request_json GET "${GATEWAY_URL}/v1/llm"
log_case_json "anonymous_get_llm_config" "{}" "${HTTP_BODY}"
if [[ "${HTTP_STATUS}" != "200" ]]; then
  echo "CASE anonymous_get_llm_config FAIL expected=200 actual=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi
echo "CASE anonymous_get_llm_config PASS status=${HTTP_STATUS}"

llm_provider="$(python3 -c 'import json,sys; print(json.loads(sys.stdin.read()).get("config",{}).get("provider",""))' <<< "${HTTP_BODY}")"
if [[ -z "${llm_provider}" ]]; then
  echo "CASE anonymous_get_llm_config FAIL missing provider field body=${HTTP_BODY}"
  exit 1
fi
echo "CASE anonymous_get_llm_config_provider PASS provider=${llm_provider}"

# CASE: PUT adds a model and GET /v1/models reflects it
current_llm="$(python3 -c 'import json,sys; print(json.dumps(json.loads(sys.stdin.read()).get("config",{})))' <<< "${HTTP_BODY}")"

# Use python to inject a test model into the current config
new_llm="$(CURRENT_LLM="${current_llm}" python3 - <<'PYEOF'
import json, os
llm = json.loads(os.environ["CURRENT_LLM"])
provider = llm.get("provider", "anthropic")
test_model = {
    "id": "test-model-e2e",
    "name": "Test E2E",
    "context_window": 4096,
    "reasoning": False,
    "input": ["text"],
    "cost": {"input": 0, "output": 0, "cache_read": 0, "cache_write": 0},
    "compat": {}
}
if provider in llm and isinstance(llm[provider], dict):
    llm[provider].setdefault("models", [])
    llm[provider]["models"] = [m for m in llm[provider]["models"] if m.get("id") != "test-model-e2e"]
    llm[provider]["models"].append(test_model)
print(json.dumps(llm))
PYEOF
)"

request_json PUT "${GATEWAY_URL}/v1/llm" "${new_llm}"
log_case_json "anonymous_put_llm_adds_model" "${new_llm}" "${HTTP_BODY}"
if [[ "${HTTP_STATUS}" != "200" ]]; then
  echo "CASE anonymous_put_llm_adds_model FAIL expected=200 actual=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi
echo "CASE anonymous_put_llm_adds_model PASS"

# Verify /v1/models now includes the test model
request_json GET "${GATEWAY_URL}/v1/models"
log_case_json "anonymous_llm_model_catalog_updated" "{}" "${HTTP_BODY}"
found="$(python3 -c 'import json,sys; data=json.loads(sys.stdin.read()).get("data",[]); print(any(m.get("id")=="test-model-e2e" for m in data))' <<< "${HTTP_BODY}")"
if [[ "${found}" != "True" ]]; then
  echo "CASE anonymous_llm_model_catalog_updated FAIL test-model-e2e not found body=${HTTP_BODY}"
  exit 1
fi
echo "CASE anonymous_llm_model_catalog_updated PASS"

# CASE: PUT removes the test model
current_llm="$(NEW_LLM="${new_llm}" python3 - <<'PYEOF'
import json, os
llm = json.loads(os.environ["NEW_LLM"])
provider = llm.get("provider", "anthropic")
if provider in llm and isinstance(llm[provider], dict):
    llm[provider]["models"] = [m for m in llm[provider].get("models", []) if m.get("id") != "test-model-e2e"]
print(json.dumps(llm))
PYEOF
)"

request_json PUT "${GATEWAY_URL}/v1/llm" "${current_llm}"
log_case_json "anonymous_put_llm_removes_model" "${current_llm}" "${HTTP_BODY}"
if [[ "${HTTP_STATUS}" != "200" ]]; then
  echo "CASE anonymous_put_llm_removes_model FAIL expected=200 actual=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi

request_json GET "${GATEWAY_URL}/v1/models"
found="$(python3 -c 'import json,sys; data=json.loads(sys.stdin.read()).get("data",[]); print(any(m.get("id")=="test-model-e2e" for m in data))' <<< "${HTTP_BODY}")"
if [[ "${found}" != "False" ]]; then
  echo "CASE anonymous_put_llm_removes_model FAIL test-model-e2e still present"
  exit 1
fi
echo "CASE anonymous_put_llm_removes_model PASS"
