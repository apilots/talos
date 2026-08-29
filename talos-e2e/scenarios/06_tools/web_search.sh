#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

require_command curl
require_command python3

if ! has_real_llm_key; then
  echo "CASE web_search_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE web_search_real_llm_precheck PASS key_detected=true"

for key_name in \
  TALOS_WEB_SEARCH_FIRECRAWL_API_KEY \
  TALOS_WEB_SEARCH_TAVILY_API_KEY; do
  if [[ -z "${!key_name:-}" ]]; then
    echo "CASE web_search_provider_precheck FAIL missing ${key_name} in .env"
    exit 1
  fi
  echo "CASE web_search_provider_precheck PASS provider_key=${key_name}"
done

build_binaries
trap stop_server EXIT

build_session_context() {
  python3 - <<'PY'
import json
import os

payload = {
    "context": {
        "source_type": "http",
        "provider": "http",
        "channel_id": "tool-web-search",
        "user_id": "tool-web-search-user",
        "thread_id": None,
        "message_id": "msg-tool-web-search",
        "trace_id": "trace-tool-web-search",
    },
    "modelSelection": {"modelRef": {"provider": os.environ["PRIMARY_PROVIDER"], "id": os.environ["PRIMARY_MODEL"]}, "reasoningEffort": None},
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

create_provider_config() {
  local provider="$1"
  local config_path
  config_path="$(scenario_report_path "talos-${provider}.yaml")"
  PROVIDER="${provider}" CONFIG_SOURCE="${REPO_ROOT}/conf/talos-dev.yaml" CONFIG_PATH="${config_path}" python3 - <<'PY'
import os
from pathlib import Path

source = Path(os.environ["CONFIG_SOURCE"])
target = Path(os.environ["CONFIG_PATH"])
provider = os.environ["PROVIDER"]
lines = source.read_text().splitlines(keepends=True)
in_web_search = False
for index, line in enumerate(lines):
    stripped = line.strip()
    indent = len(line) - len(line.lstrip())
    if stripped == "web_search:":
        in_web_search = True
        continue
    if in_web_search and stripped and indent <= 6:
        break
    if in_web_search and stripped.startswith("provider:"):
        newline = "\n" if line.endswith("\n") else ""
        lines[index] = f'{line[:indent]}provider: "{provider}"{newline}'
        target.write_text("".join(lines))
        break
else:
    raise SystemExit("missing agent.kernel.tools.web_search.provider")
PY
  printf '%s\n' "${config_path}"
}

run_provider() {
  local provider="$1"
  local query="$2"
  local expected_domain="$3"

  stop_server
  unset TALOS_HOME
  export TALOS_TEST_CONFIG_TEMPLATE
  TALOS_TEST_CONFIG_TEMPLATE="$(create_provider_config "${provider}")"
  start_server

  local models_json
  local primary_model
  local primary_provider
  models_json="$(curl -sS "${GATEWAY_URL}/v1/models")"
  primary_model="$(select_e2e_model_from_json "${models_json}")"
  primary_provider="$(select_e2e_model_provider_from_json "${models_json}" "${primary_model}")"
  if [[ "${primary_model}" != "deepseek-v4-flash" || "${primary_provider}" != "deepseek" ]]; then
    echo "CASE web_search_select_model FAIL expected=deepseek/deepseek-v4-flash actual=${primary_provider}/${primary_model}"
    exit 1
  fi
  export PRIMARY_MODEL="${primary_model}"
  export PRIMARY_PROVIDER="${primary_provider}"
  echo "CASE web_search_select_model PASS provider=${provider} model=${primary_model}"

  local session_payload
  local session_id
  local session_json
  session_payload="$(build_session_context)"
  session_id="$(new_session_id)"
  remember_session_context "${session_id}" "${session_payload}"
  session_json="{\"id\":\"${session_id}\"}"
  session_id="$(session_id_from_json "${session_json}")"
  if [[ -z "${session_id}" ]]; then
    echo "CASE web_search_session_id FAIL provider=${provider} response=${session_json}"
    exit 1
  fi
  echo "CASE web_search_session_id PASS provider=${provider} session_id=${session_id}"

  local activate_input
  local activate_payload
  local activate_json
  activate_input="You must call tool 'ToolSearch' exactly once with query WebSearch to activate it. Do not call WebSearch in this turn. After the ToolSearch tool result is returned, respond with exactly WEB_SEARCH_ACTIVATED"
  activate_payload="$(build_payload "${activate_input}")"
  activate_json="$(send_message_response "${session_id}" "${activate_payload}" "activate-${provider}")"
  log_case_json "tool_web_search_activate_${provider}" "${activate_payload}" "${activate_json}"

  ACTIVATE_JSON="${activate_json}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["ACTIVATE_JSON"])
results = [item for item in data.get("output", []) if item.get("type") == "tool_result"]
assert results, "missing tool_result"
tool_names = [item.get("tool_name") for item in results]
assert "ToolSearch" in tool_names, tool_names
PY

  local search_input
  local search_payload
  local search_json
  local search_output
  search_input="You must call tool 'WebSearch' exactly once with query ${query}, allowed_domains ${expected_domain}, and limit 3. Do not call ToolSearch again. After the WebSearch tool result is returned, respond with exactly WEB_SEARCH_OK"
  search_payload="$(build_payload "${search_input}")"
  search_json="$(send_message_response "${session_id}" "${search_payload}" "search-${provider}")"
  log_case_json "tool_web_search_${provider}" "${search_payload}" "${search_json}"

  search_output="$(extract_output_text "${search_json}")"
  if [[ -z "${search_output}" || "${search_output}" == Error:* ]]; then
    echo "CASE tool_web_search FAIL provider=${provider} response=${search_json}"
    exit 1
  fi

  WEB_SEARCH_JSON="${search_json}" EXPECTED_DOMAIN="${expected_domain}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["WEB_SEARCH_JSON"])
results = [item for item in data.get("output", []) if item.get("type") == "tool_result"]
assert results, "missing tool_result"
tool_names = [item.get("tool_name") for item in results]
assert "WebSearch" in tool_names, tool_names
result = next(item for item in results if item.get("tool_name") == "WebSearch")
content_items = result.get("content") or []
assert content_items, "missing content payload"
references = [item for item in content_items if item.get("type") == "web_reference"]
assert references, content_items
expected_domain = os.environ["EXPECTED_DOMAIN"]
assert all(item.get("url") for item in references), references
assert all(item.get("title") for item in references), references
assert all(item.get("citation_id") for item in references), references
assert all(item.get("snippet") for item in references), references
assert all(item.get("source_domain", "").endswith(expected_domain) for item in references), references
assert [item.get("provider_rank") for item in references] == list(range(1, len(references) + 1)), references
PY

  echo "CASE tool_web_search PASS provider=${provider}"
  echo "DETAIL tool_web_search_${provider}_output=${search_output}"
}

run_provider "firecrawl" "site:firecrawl.dev Firecrawl Search API" "firecrawl.dev"
run_provider "tavily" "site:tavily.com Tavily Search API" "tavily.com"

if [[ -n "${TALOS_WEB_SEARCH_BRAVE_API_KEY:-}" ]]; then
  run_provider "brave" "site:brave.com Brave Search API" "brave.com"
else
  echo "CASE tool_web_search SKIP provider=brave reason=missing_api_key"
fi
