#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
require_command curl

export TALOS_TEST_ENABLE_JWT_AUTH=1
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

# Register admin and user
admin_payload='{"username":"admin-llm","password":"password123","email":"admin-llm@example.com"}'
request_json POST "${GATEWAY_URL}/v1/auth/register" "${admin_payload}"
admin_token="$(python3 -c 'import json,sys; print(json.loads(sys.stdin.read())["access_token"])' <<< "${HTTP_BODY}")"

user_payload='{"username":"user-llm","password":"password123","email":"user-llm@example.com"}'
request_json POST "${GATEWAY_URL}/v1/auth/register" "${user_payload}"
user_token="$(python3 -c 'import json,sys; print(json.loads(sys.stdin.read())["access_token"])' <<< "${HTTP_BODY}")"

# CASE: non-admin cannot access admin/llm
request_json GET "${GATEWAY_URL}/v1/admin/llm" "" "${user_token}"
log_case_json "admin_llm_requires_admin_get" "{}" "${HTTP_BODY}"
if [[ "${HTTP_STATUS}" != "403" ]]; then
  echo "CASE admin_llm_requires_admin_get FAIL expected=403 actual=${HTTP_STATUS}"
  exit 1
fi
echo "CASE admin_llm_requires_admin_get PASS status=${HTTP_STATUS}"

# CASE: admin can GET /v1/admin/llm
request_json GET "${GATEWAY_URL}/v1/admin/llm" "" "${admin_token}"
log_case_json "admin_get_llm_config" "{}" "${HTTP_BODY}"
if [[ "${HTTP_STATUS}" != "200" ]]; then
  echo "CASE admin_get_llm_config FAIL expected=200 actual=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi
llm_provider="$(python3 -c 'import json,sys; print(json.loads(sys.stdin.read()).get("provider",""))' <<< "${HTTP_BODY}")"
if [[ -z "${llm_provider}" ]]; then
  echo "CASE admin_get_llm_config FAIL missing provider field body=${HTTP_BODY}"
  exit 1
fi
echo "CASE admin_get_llm_config PASS provider=${llm_provider}"

# CASE: PUT adds a model and GET /v1/models reflects it
current_llm="${HTTP_BODY}"

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

request_json PUT "${GATEWAY_URL}/v1/admin/llm" "${new_llm}" "${admin_token}"
log_case_json "admin_put_llm_adds_model" "${new_llm}" "${HTTP_BODY}"
if [[ "${HTTP_STATUS}" != "200" ]]; then
  echo "CASE admin_put_llm_adds_model FAIL expected=200 actual=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi
echo "CASE admin_put_llm_adds_model PASS"

# Verify /v1/models now includes the test model
request_json GET "${GATEWAY_URL}/v1/models" "" "${admin_token}"
log_case_json "admin_llm_model_catalog_updated" "{}" "${HTTP_BODY}"
found="$(python3 -c 'import json,sys; data=json.loads(sys.stdin.read()).get("data",[]); print(any(m.get("id")=="test-model-e2e" for m in data))' <<< "${HTTP_BODY}")"
if [[ "${found}" != "True" ]]; then
  echo "CASE admin_llm_model_catalog_updated FAIL test-model-e2e not found body=${HTTP_BODY}"
  exit 1
fi
echo "CASE admin_llm_model_catalog_updated PASS"

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

request_json PUT "${GATEWAY_URL}/v1/admin/llm" "${current_llm}" "${admin_token}"
log_case_json "admin_put_llm_removes_model" "${current_llm}" "${HTTP_BODY}"
if [[ "${HTTP_STATUS}" != "200" ]]; then
  echo "CASE admin_put_llm_removes_model FAIL expected=200 actual=${HTTP_STATUS} body=${HTTP_BODY}"
  exit 1
fi

request_json GET "${GATEWAY_URL}/v1/models" "" "${admin_token}"
found="$(python3 -c 'import json,sys; data=json.loads(sys.stdin.read()).get("data",[]); print(any(m.get("id")=="test-model-e2e" for m in data))' <<< "${HTTP_BODY}")"
if [[ "${found}" != "False" ]]; then
  echo "CASE admin_put_llm_removes_model FAIL test-model-e2e still present"
  exit 1
fi
echo "CASE admin_put_llm_removes_model PASS"
