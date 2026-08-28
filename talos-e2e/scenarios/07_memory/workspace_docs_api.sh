#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

require_command curl
require_command python3
require_command mktemp

TEST_TALOS_HOME="$(make_e2e_temp_dir "talos-memory-docs-home")"
cleanup() {
  stop_server
  if [[ -n "${TALOS_MEMORY_E2E_ARTIFACT_DIR:-}" ]]; then
    mkdir -p "${TALOS_MEMORY_E2E_ARTIFACT_DIR}"
    cp -R "${TEST_TALOS_HOME}/." "${TALOS_MEMORY_E2E_ARTIFACT_DIR}/"
    rm -f "${TALOS_MEMORY_E2E_ARTIFACT_DIR}/.env"
  fi
  rm -rf "${TEST_TALOS_HOME}"
}
export TALOS_HOME="${TEST_TALOS_HOME}"

start_server
trap cleanup EXIT

workspace_info_json="$(curl -sS "${GATEWAY_URL}/v1/workspace/info")"
WORKSPACE_DIR="$(api_json_get "${workspace_info_json}" "user_workspace_dir")"
if [[ -z "${WORKSPACE_DIR}" ]]; then
  echo "CASE workspace_info FAIL response=${workspace_info_json}"
  exit 1
fi
echo "CASE workspace_info PASS user_workspace_dir=${WORKSPACE_DIR}"
MEMORY_DATE="$(date '+%Y-%m-%d')"
MEMORY_FILE="${WORKSPACE_DIR}/memory/${MEMORY_DATE}.md"
MEMORY_BUNDLE_INDEX="${WORKSPACE_DIR}/memory/index.md"
MEMORY_BUNDLE_LOG="${WORKSPACE_DIR}/memory/log.md"
MEMORY_INDEX="${TALOS_HOME}/data/memory-index.sqlite"
SOUL_FILE="${WORKSPACE_DIR}/SOUL.md"
MEMORY_DOC_FILE="${WORKSPACE_DIR}/MEMORY.md"
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
workspace_doc_count="$(python3 -c 'import json,sys; print(len(json.load(sys.stdin).get("data", [])))' <<< "$(api_data_json "${body}")")"
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
body_data="$(api_data_json "${body}")"
doc_exists="$(json_get "${body_data}" "doc.exists")"
doc_exists="$(printf '%s' "${doc_exists}" | tr '[:upper:]' '[:lower:]')"
doc_content="$(json_get "${body_data}" "doc.content")"
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
body_data="$(api_data_json "${body}")"
doc_version_token="$(json_get "${body_data}" "doc.version_token")"
doc_exists="$(json_get "${body_data}" "doc.exists")"
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

parse_response "$(request_json GET "${GATEWAY_URL}/v1/workspace/docs/memory")"
status="${RESPONSE_STATUS}"
body="${RESPONSE_BODY}"
body_data="$(api_data_json "${body}")"
memory_doc_exists="$(json_get "${body_data}" "doc.exists")"
memory_doc_exists="$(printf '%s' "${memory_doc_exists}" | tr '[:upper:]' '[:lower:]')"
if [[ "${status}" != "200" || "${memory_doc_exists}" != "false" ]]; then
  echo "CASE get_missing_user_workspace_memory FAIL status=${status} body=${body}"
  exit 1
fi
echo "CASE get_missing_user_workspace_memory PASS exists=${memory_doc_exists}"

create_memory_doc_payload="$(python3 - <<'PY'
import json
from datetime import datetime, timezone
generated_at = datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")
content = """---
type: UserWorkspaceMemory
title: User Workspace Memory
description: Durable cross-session memory scoped to one agent and one user.
tags:
  - talos
  - agent
  - user-memory
status: stable
generated:
  by: talos-agent/default
  at: "{generated_at}"
---

# User Workspace Memory

<!--
Keep only information that remains useful across Sessions.
Record conclusions instead of chronological activity.
Update or remove stale entries instead of accumulating history.
-->

<!-- talos:user-workspace-memory:start -->

## Interaction Guidance

<!-- Model-learned collaboration preferences and confirmed feedback. -->

## Key Knowledge

<!-- Durable facts, decisions, constraints, lessons, and references. -->

## Active Context

<!-- Cross-session goals, commitments, blockers, ownership, and next steps. -->

<!-- talos:user-workspace-memory:end -->
""".format(generated_at=generated_at)
print(json.dumps({"content": content, "base_version": {"mode": "must_not_exist"}}))
PY
)"
parse_response "$(request_json PUT "${GATEWAY_URL}/v1/workspace/docs/memory" "${create_memory_doc_payload}")"
status="${RESPONSE_STATUS}"
body="${RESPONSE_BODY}"
if [[ "${status}" != "200" || ! -f "${MEMORY_DOC_FILE}" ]]; then
  echo "CASE create_user_workspace_memory FAIL status=${status} body=${body}"
  exit 1
fi
for expected in \
  "type: UserWorkspaceMemory" \
  "status: stable" \
  "generated:" \
  "by: talos-agent/default" \
  "<!-- talos:user-workspace-memory:start -->" \
  "## Interaction Guidance" \
  "## Key Knowledge" \
  "## Active Context" \
  "<!-- talos:user-workspace-memory:end -->"; do
  if ! grep -Fq "${expected}" "${MEMORY_DOC_FILE}"; then
    echo "CASE create_user_workspace_memory FAIL missing=${expected} path=${MEMORY_DOC_FILE}"
    exit 1
  fi
done
echo "CASE create_user_workspace_memory PASS path=${MEMORY_DOC_FILE}"

parse_response "$(request_json PUT "${GATEWAY_URL}/v1/workspace/docs/memory" "${create_memory_doc_payload}")"
status="${RESPONSE_STATUS}"
if [[ "${status}" != "409" ]]; then
  echo "CASE reject_user_workspace_memory_blind_overwrite FAIL status=${status} body=${RESPONSE_BODY}"
  exit 1
fi
echo "CASE reject_user_workspace_memory_blind_overwrite PASS status=${status}"

parse_response "$(request_json GET "${GATEWAY_URL}/v1/memory/files?limit=5")"
status="${RESPONSE_STATUS}"
body="${RESPONSE_BODY}"
log_case_json "list_memory_files_before_write" "{\"limit\":5}" "${body}"
if [[ "${status}" != "200" ]]; then
  echo "CASE list_memory_files_before_write FAIL status=${status} body=${body}"
  exit 1
fi
memory_count_before="$(python3 -c 'import json,sys; print(len(json.load(sys.stdin).get("data", [])))' <<< "$(api_data_json "${body}")")"
if [[ "${memory_count_before}" != "0" ]]; then
  echo "CASE list_memory_files_before_write FAIL expected=0 actual=${memory_count_before}"
  exit 1
fi
echo "CASE list_memory_files_before_write PASS count=${memory_count_before}"

create_memory_payload="$(python3 - <<PY
import json
from datetime import datetime, timezone
from zoneinfo import ZoneInfo

occurred_at = int(datetime.strptime("${MEMORY_DATE} 09:30", "%Y-%m-%d %H:%M").replace(tzinfo=ZoneInfo("Asia/Shanghai")).timestamp())
generated_at = datetime.fromtimestamp(occurred_at, timezone.utc).isoformat().replace("+00:00", "Z")
content = f"""---
type: DreamMemory
title: Dream Memory ${MEMORY_DATE}
description: Durable memories formed for ${MEMORY_DATE}.
tags: [talos, dream-memory]
status: stable
generated:
  by: talos-agent/default
  at: "{generated_at}"
---

# Dream Memory ${MEMORY_DATE}

<!-- talos:dream-memory:start -->

## Memories

<!-- talos:dream-memory:entry key="mem_0123456789abcdef0123456789abcdef" -->
### API memory

- **Occurred At:** {occurred_at}
- **Kind:** fact
- **Importance:** normal

${MEMORY_MARKER}

<!-- talos:dream-memory:end -->
"""
print(json.dumps({
    "date": "${MEMORY_DATE}",
    "content": content,
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
memory_version_token="$(api_json_get "${body}" "file.version_token")"
if [[ -z "${memory_version_token}" || ! -f "${MEMORY_FILE}" ]]; then
  echo "CASE create_memory_file FAIL version=${memory_version_token} path=${MEMORY_FILE}"
  exit 1
fi
if [[ ! -f "${MEMORY_BUNDLE_INDEX}" || ! -f "${MEMORY_BUNDLE_LOG}" ]]; then
  echo "CASE create_memory_file FAIL bundle_missing index=${MEMORY_BUNDLE_INDEX} log=${MEMORY_BUNDLE_LOG}"
  exit 1
fi
if ! grep -Fq "${MEMORY_MARKER}" "${MEMORY_FILE}"; then
  echo "CASE create_memory_file FAIL marker_missing path=${MEMORY_FILE}"
  exit 1
fi
for expected in \
  "type: DreamMemory" \
  "generated:" \
  "by: talos-agent/default" \
  "<!-- talos:dream-memory:start -->" \
  "<!-- talos:dream-memory:entry key=\"mem_0123456789abcdef0123456789abcdef\" -->" \
  "- **Occurred At:**" \
  "- **Kind:** fact" \
  "- **Importance:** normal" \
  "<!-- talos:dream-memory:end -->"; do
  if ! grep -Fq -- "${expected}" "${MEMORY_FILE}"; then
    echo "CASE create_memory_file FAIL canonical_field_missing=${expected} path=${MEMORY_FILE}"
    exit 1
  fi
done
for expected in \
  'okf_version: "0.2"' \
  "[Dream Memory ${MEMORY_DATE}](${MEMORY_DATE}.md)" \
  "Contains 1 durable memory: API memory."; do
  if ! grep -Fq -- "${expected}" "${MEMORY_BUNDLE_INDEX}"; then
    echo "CASE create_memory_file FAIL index_field_missing=${expected} path=${MEMORY_BUNDLE_INDEX}"
    exit 1
  fi
done
for expected in \
  "# Dream Memory Log" \
  "**Creation**" \
  "[Dream Memory ${MEMORY_DATE}](${MEMORY_DATE}.md)" \
  ' revision `' \
  "Contains 1 durable memory: API memory."; do
  if ! grep -Fq -- "${expected}" "${MEMORY_BUNDLE_LOG}"; then
    echo "CASE create_memory_file FAIL log_field_missing=${expected} path=${MEMORY_BUNDLE_LOG}"
    exit 1
  fi
done
echo "CASE create_memory_file PASS version=${memory_version_token}"

parse_response "$(request_json GET "${GATEWAY_URL}/v1/memory/files?limit=5&date_from=${MEMORY_DATE}&date_to=${MEMORY_DATE}")"
status="${RESPONSE_STATUS}"
body="${RESPONSE_BODY}"
log_case_json "list_memory_files_in_range" "{\"limit\":5,\"date_from\":\"${MEMORY_DATE}\",\"date_to\":\"${MEMORY_DATE}\"}" "${body}"
if [[ "${status}" != "200" ]]; then
  echo "CASE list_memory_files_in_range FAIL status=${status} body=${body}"
  exit 1
fi
memory_count_in_range="$(python3 -c 'import json,sys; print(len(json.load(sys.stdin).get("data", [])))' <<< "$(api_data_json "${body}")")"
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
memory_count_before_date="$(python3 -c 'import json,sys; print(len(json.load(sys.stdin).get("data", [])))' <<< "$(api_data_json "${body}")")"
if [[ "${memory_count_before_date}" != "0" ]]; then
  echo "CASE list_memory_files_before_date FAIL expected=0 actual=${memory_count_before_date}"
  exit 1
fi
echo "CASE list_memory_files_before_date PASS count=${memory_count_before_date}"

parse_response "$(request_json GET "${GATEWAY_URL}/v1/memory/search?query=${MEMORY_MARKER}&limit=5")"
status="${RESPONSE_STATUS}"
body="${RESPONSE_BODY}"
log_case_json "search_memory_files" "{\"query\":\"${MEMORY_MARKER}\",\"limit\":5}" "${body}"
if [[ "${status}" != "200" ]]; then
  echo "CASE search_memory_files FAIL status=${status} body=${body}"
  exit 1
fi
search_hit_count="$(python3 -c 'import json,sys; print(len(json.load(sys.stdin).get("data", [])))' <<< "$(api_data_json "${body}")")"
if [[ "${search_hit_count}" -lt "1" ]]; then
  echo "CASE search_memory_files FAIL expected_at_least=1 actual=${search_hit_count}"
  exit 1
fi
echo "CASE search_memory_files PASS hits=${search_hit_count}"
memory_id="$(python3 -c 'import json,sys; data=json.load(sys.stdin).get("data", []); print(data[0].get("memoryId", "") if data else "")' <<< "$(api_data_json "${body}")")"
if [[ -z "${memory_id}" ]]; then
  echo "CASE search_memory_files FAIL missing_memory_id body=${body}"
  exit 1
fi

stop_server
rm -f "${MEMORY_INDEX}" "${MEMORY_INDEX}-wal" "${MEMORY_INDEX}-shm"
start_server

parse_response "$(request_json GET "${GATEWAY_URL}/v1/memory/search?query=${MEMORY_MARKER}&limit=5")"
status="${RESPONSE_STATUS}"
body="${RESPONSE_BODY}"
rebuilt_hit_count="$(python3 -c 'import json,sys; print(len(json.load(sys.stdin).get("data", [])))' <<< "$(api_data_json "${body}")")"
if [[ "${status}" != "200" || "${rebuilt_hit_count}" -lt "1" ]]; then
  echo "CASE rebuild_memory_index_from_okf FAIL status=${status} body=${body}"
  exit 1
fi
parse_response "$(request_json GET "${GATEWAY_URL}/v1/memory/entries/${memory_id}")"
status="${RESPONSE_STATUS}"
body="${RESPONSE_BODY}"
rebuilt_body="$(api_json_get "${body}" "memory.body")"
if [[ "${status}" != "200" || "${rebuilt_body}" != *"${MEMORY_MARKER}"* ]]; then
  echo "CASE rebuild_memory_index_from_okf FAIL get_status=${status} body=${body}"
  exit 1
fi
echo "CASE rebuild_memory_index_from_okf PASS memory_id=${memory_id}"

stale_memory_payload="$(python3 - <<PY
import json
print(json.dumps({
    "date": "${MEMORY_DATE}",
    "content": "# Memory\\nstale-overwrite\\n",
    "expected_version": "stale-token",
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
