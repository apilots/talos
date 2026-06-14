#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

load_env
require_command curl
require_command python3
require_command mktemp

TEST_TALOS_HOME="$(make_e2e_temp_dir "talos-memory-docs-home")"
cleanup() {
  stop_server
  rm -rf "${TEST_TALOS_HOME}"
}
export TALOS_HOME="${TEST_TALOS_HOME}"

start_server
trap cleanup EXIT

workspace_info_json="$(curl -sS "${GATEWAY_URL}/v1/workspace/info")"
WORKSPACE_DIR="$(json_get "${workspace_info_json}" "user_workspace_dir")"
if [[ -z "${WORKSPACE_DIR}" ]]; then
  echo "CASE workspace_info FAIL response=${workspace_info_json}"
  exit 1
fi
echo "CASE workspace_info PASS user_workspace_dir=${WORKSPACE_DIR}"
MEMORY_DATE="$(date '+%Y-%m-%d')"
MEMORY_FILE="${WORKSPACE_DIR}/memory/${MEMORY_DATE}.md"
MEMORY_INDEX_FILE="${WORKSPACE_DIR}/memory/${MEMORY_DATE}.index.json"
SOUL_FILE="${WORKSPACE_DIR}/SOUL.md"
MEMORY_MARKER="memory-api-marker-$(date '+%Y%m%d%H%M%S')-$$"
DOC_MARKER="workspace-doc-marker-$(date '+%Y%m%d%H%M%S')-$$"

request_json() {
  local method="$1"
  local url="$2"
  local body="${3:-}"
  local response_file
  response_file="$(mktemp "${TMPDIR:-/tmp}/talos-integration-response.XXXXXX")"
  local status
  if [[ -n "${body}" ]]; then
    status="$(curl -sS -o "${response_file}" -w "%{http_code}" \
      -X "${method}" "${url}" \
      -H "Content-Type: application/json" \
      -d "${body}")"
  else
    status="$(curl -sS -o "${response_file}" -w "%{http_code}" \
      -X "${method}" "${url}" \
     )"
  fi
  cat "${response_file}"
  rm -f "${response_file}"
  printf '\nHTTP_STATUS:%s' "${status}"
}

parse_response() {
  local raw="$1"
  RESPONSE_STATUS="${raw##*HTTP_STATUS:}"
  RESPONSE_BODY="${raw%HTTP_STATUS:*}"
}

parse_response "$(request_json GET "${GATEWAY_URL}/v1/workspace/docs")"
status="${RESPONSE_STATUS}"
body="${RESPONSE_BODY}"
log_case_json "list_workspace_docs" "{}" "${body}"
if [[ "${status}" != "200" ]]; then
  echo "CASE list_workspace_docs FAIL status=${status} body=${body}"
  exit 1
fi
workspace_doc_count="$(python3 -c 'import json,sys; print(len(json.load(sys.stdin).get("data", [])))' <<< "${body}")"
if [[ "${workspace_doc_count}" != "5" ]]; then
  echo "CASE list_workspace_docs FAIL expected=5 actual=${workspace_doc_count}"
  exit 1
fi
echo "CASE list_workspace_docs PASS count=${workspace_doc_count}"

parse_response "$(request_json GET "${GATEWAY_URL}/v1/workspace/docs/soul")"
status="${RESPONSE_STATUS}"
body="${RESPONSE_BODY}"
log_case_json "get_missing_workspace_doc" "{\"kind\":\"soul\"}" "${body}"
if [[ "${status}" != "200" ]]; then
  echo "CASE get_missing_workspace_doc FAIL status=${status} body=${body}"
  exit 1
fi
doc_exists="$(json_get "${body}" "exists")"
doc_exists="$(printf '%s' "${doc_exists}" | tr '[:upper:]' '[:lower:]')"
doc_content="$(json_get "${body}" "content")"
if [[ "${doc_exists}" != "false" || -n "${doc_content}" ]]; then
  echo "CASE get_missing_workspace_doc FAIL exists=${doc_exists} content=${doc_content}"
  exit 1
fi
echo "CASE get_missing_workspace_doc PASS exists=${doc_exists}"

create_doc_payload="$(python3 - <<PY
import json
print(json.dumps({
    "content": "# Soul\\n${DOC_MARKER}\\n",
    "base_version": {"mode": "must_not_exist"},
}))
PY
)"
parse_response "$(request_json PUT "${GATEWAY_URL}/v1/workspace/docs/soul" "${create_doc_payload}")"
status="${RESPONSE_STATUS}"
body="${RESPONSE_BODY}"
log_case_json "create_workspace_doc" "${create_doc_payload}" "${body}"
if [[ "${status}" != "200" ]]; then
  echo "CASE create_workspace_doc FAIL status=${status} body=${body}"
  exit 1
fi
doc_version_token="$(json_get "${body}" "version_token")"
doc_exists="$(json_get "${body}" "exists")"
doc_exists="$(printf '%s' "${doc_exists}" | tr '[:upper:]' '[:lower:]')"
if [[ "${doc_exists}" != "true" || -z "${doc_version_token}" || ! -f "${SOUL_FILE}" ]]; then
  echo "CASE create_workspace_doc FAIL exists=${doc_exists} version=${doc_version_token} path=${SOUL_FILE}"
  exit 1
fi
if ! grep -Fq "${DOC_MARKER}" "${SOUL_FILE}"; then
  echo "CASE create_workspace_doc FAIL marker_missing path=${SOUL_FILE}"
  exit 1
fi
echo "CASE create_workspace_doc PASS version=${doc_version_token}"

conflict_doc_payload="$(python3 - <<PY
import json
print(json.dumps({
    "content": "# Soul\\nshould-conflict\\n",
    "base_version": {"mode": "must_not_exist"},
}))
PY
)"
parse_response "$(request_json PUT "${GATEWAY_URL}/v1/workspace/docs/soul" "${conflict_doc_payload}")"
status="${RESPONSE_STATUS}"
body="${RESPONSE_BODY}"
log_case_json "reject_workspace_doc_blind_overwrite" "${conflict_doc_payload}" "${body}"
if [[ "${status}" != "409" ]]; then
  echo "CASE reject_workspace_doc_blind_overwrite FAIL status=${status} body=${body}"
  exit 1
fi
echo "CASE reject_workspace_doc_blind_overwrite PASS status=${status}"

parse_response "$(request_json GET "${GATEWAY_URL}/v1/memory/files?limit=5")"
status="${RESPONSE_STATUS}"
body="${RESPONSE_BODY}"
log_case_json "list_memory_files_before_write" "{\"limit\":5}" "${body}"
if [[ "${status}" != "200" ]]; then
  echo "CASE list_memory_files_before_write FAIL status=${status} body=${body}"
  exit 1
fi
memory_count_before="$(python3 -c 'import json,sys; print(len(json.load(sys.stdin).get("data", [])))' <<< "${body}")"
if [[ "${memory_count_before}" != "0" ]]; then
  echo "CASE list_memory_files_before_write FAIL expected=0 actual=${memory_count_before}"
  exit 1
fi
echo "CASE list_memory_files_before_write PASS count=${memory_count_before}"

create_memory_payload="$(python3 - <<PY
import json
print(json.dumps({
    "content": "# Memory\\n${MEMORY_MARKER}\\n",
    "base_version": {"mode": "must_not_exist"},
}))
PY
)"
parse_response "$(request_json PUT "${GATEWAY_URL}/v1/memory/files/${MEMORY_DATE}" "${create_memory_payload}")"
status="${RESPONSE_STATUS}"
body="${RESPONSE_BODY}"
log_case_json "create_memory_file" "${create_memory_payload}" "${body}"
if [[ "${status}" != "200" ]]; then
  echo "CASE create_memory_file FAIL status=${status} body=${body}"
  exit 1
fi
memory_version_token="$(json_get "${body}" "version_token")"
if [[ -z "${memory_version_token}" || ! -f "${MEMORY_FILE}" ]]; then
  echo "CASE create_memory_file FAIL version=${memory_version_token} path=${MEMORY_FILE}"
  exit 1
fi
if ! grep -Fq "${MEMORY_MARKER}" "${MEMORY_FILE}"; then
  echo "CASE create_memory_file FAIL marker_missing path=${MEMORY_FILE}"
  exit 1
fi
echo "CASE create_memory_file PASS version=${memory_version_token}"

if [[ ! -f "${MEMORY_INDEX_FILE}" ]]; then
  echo "CASE rebuild_memory_index FAIL missing_index=${MEMORY_INDEX_FILE}"
  exit 1
fi
index_source_hash="$(python3 -c 'import json,sys; print(json.load(sys.stdin).get("source_content_hash", ""))' < "${MEMORY_INDEX_FILE}")"
index_path="$(python3 -c 'import json,sys; print(json.load(sys.stdin).get("path", ""))' < "${MEMORY_INDEX_FILE}")"
if [[ -z "${index_source_hash}" || "${index_path}" != "memory/${MEMORY_DATE}.md" ]]; then
  echo "CASE rebuild_memory_index FAIL source_content_hash=${index_source_hash} path=${index_path}"
  exit 1
fi
echo "CASE rebuild_memory_index PASS path=${index_path}"

parse_response "$(request_json GET "${GATEWAY_URL}/v1/memory/files?limit=5&date_from=${MEMORY_DATE}&date_to=${MEMORY_DATE}")"
status="${RESPONSE_STATUS}"
body="${RESPONSE_BODY}"
log_case_json "list_memory_files_in_range" "{\"limit\":5,\"date_from\":\"${MEMORY_DATE}\",\"date_to\":\"${MEMORY_DATE}\"}" "${body}"
if [[ "${status}" != "200" ]]; then
  echo "CASE list_memory_files_in_range FAIL status=${status} body=${body}"
  exit 1
fi
memory_count_in_range="$(python3 -c 'import json,sys; print(len(json.load(sys.stdin).get("data", [])))' <<< "${body}")"
if [[ "${memory_count_in_range}" != "1" ]]; then
  echo "CASE list_memory_files_in_range FAIL expected=1 actual=${memory_count_in_range}"
  exit 1
fi
echo "CASE list_memory_files_in_range PASS count=${memory_count_in_range}"

parse_response "$(request_json GET "${GATEWAY_URL}/v1/memory/files?limit=5&before_date=${MEMORY_DATE}")"
status="${RESPONSE_STATUS}"
body="${RESPONSE_BODY}"
log_case_json "list_memory_files_before_date" "{\"limit\":5,\"before_date\":\"${MEMORY_DATE}\"}" "${body}"
if [[ "${status}" != "200" ]]; then
  echo "CASE list_memory_files_before_date FAIL status=${status} body=${body}"
  exit 1
fi
memory_count_before_date="$(python3 -c 'import json,sys; print(len(json.load(sys.stdin).get("data", [])))' <<< "${body}")"
if [[ "${memory_count_before_date}" != "0" ]]; then
  echo "CASE list_memory_files_before_date FAIL expected=0 actual=${memory_count_before_date}"
  exit 1
fi
echo "CASE list_memory_files_before_date PASS count=${memory_count_before_date}"

parse_response "$(request_json GET "${GATEWAY_URL}/v1/memory/search?q=${MEMORY_MARKER}&limit=5")"
status="${RESPONSE_STATUS}"
body="${RESPONSE_BODY}"
log_case_json "search_memory_files" "{\"q\":\"${MEMORY_MARKER}\",\"limit\":5}" "${body}"
if [[ "${status}" != "200" ]]; then
  echo "CASE search_memory_files FAIL status=${status} body=${body}"
  exit 1
fi
search_hit_count="$(python3 -c 'import json,sys; print(len(json.load(sys.stdin).get("data", [])))' <<< "${body}")"
echo "CASE search_memory_files PASS hits=${search_hit_count}"

stale_memory_payload="$(python3 - <<PY
import json
print(json.dumps({
    "content": "# Memory\\nstale-overwrite\\n",
    "base_version": {"mode": "match", "version_token": "stale-token"},
}))
PY
)"
parse_response "$(request_json PUT "${GATEWAY_URL}/v1/memory/files/${MEMORY_DATE}" "${stale_memory_payload}")"
status="${RESPONSE_STATUS}"
body="${RESPONSE_BODY}"
log_case_json "reject_memory_file_stale_token" "${stale_memory_payload}" "${body}"
if [[ "${status}" != "409" ]]; then
  echo "CASE reject_memory_file_stale_token FAIL status=${status} body=${body}"
  exit 1
fi
echo "CASE reject_memory_file_stale_token PASS status=${status}"
