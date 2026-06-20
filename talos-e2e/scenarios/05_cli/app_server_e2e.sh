#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

load_env
build_binaries
prepare_talos_home

configure_cli_skills_fixture() {
  local skill_root="${TALOS_HOME}/agents-skills/cli-review"
  mkdir -p "${skill_root}"
  cat >"${skill_root}/SKILL.md" <<'EOF'
---
name: cli-review
description: Review from CLI smoke.
---

CLI smoke skill.
EOF
}

configure_cli_skills_fixture

export TALOS_APP_SERVER_BIN="${TALOS_RS_DIR}/target/debug/app-server"

cli="${TALOS_RS_DIR}/target/debug/talos-cli"

run_cli() {
  "${cli}" "$@" 2>&1 | sed '/^Command executed successfully$/d'
}

capture_cli() {
  local __out_var="$1"
  shift
  local output
  set +e
  output="$(run_cli "$@")"
  local status=$?
  set -e
  printf -v "${__out_var}" '%s' "${output}"
  return "${status}"
}

models_output="$(run_cli models)"
log_case_json "cli_app_server_models" "{\"command\":\"models\"}" "${models_output}"
if ! echo "${models_output}" | grep -q "Available models"; then
  echo "CASE cli_app_server_models FAIL output=${models_output}"
  exit 1
fi
echo "CASE cli_app_server_models PASS"

status_output="$(run_cli status)"
log_case_json "cli_app_server_status" "{\"command\":\"status\"}" "${status_output}"
if ! echo "${status_output}" | grep -q "App-server accessible"; then
  echo "CASE cli_app_server_status FAIL output=${status_output}"
  exit 1
fi
echo "CASE cli_app_server_status PASS"

session_id="$(new_session_id)"
echo "CASE cli_app_server_session_id PASS session_id=${session_id}"

if ! capture_cli chat_output chat in "${session_id}" "Answer exactly: cli-app-server-ok"; then
  log_case_json "cli_app_server_chat_in" "{\"command\":\"chat in\",\"session_id\":\"${session_id}\"}" "${chat_output}"
  echo "CASE cli_app_server_chat_in FAIL output=${chat_output}"
  exit 1
fi
log_case_json "cli_app_server_chat_in" "{\"command\":\"chat in\",\"session_id\":\"${session_id}\"}" "${chat_output}"
if [[ -z "${chat_output}" || "${chat_output}" == Error:* ]]; then
  echo "CASE cli_app_server_chat_in FAIL output=${chat_output}"
  exit 1
fi
echo "CASE cli_app_server_chat_in PASS"
echo "DETAIL cli_app_server_chat_output=${chat_output}"

info_output="$(run_cli session info "${session_id}")"
log_case_json "cli_app_server_session_info" "{\"command\":\"session info\",\"session_id\":\"${session_id}\"}" "${info_output}"
if ! echo "${info_output}" | grep -q "${session_id}"; then
  echo "CASE cli_app_server_session_info FAIL output=${info_output}"
  exit 1
fi
echo "CASE cli_app_server_session_info PASS"

list_output="$(run_cli session list --limit 5)"
log_case_json "cli_app_server_session_list" "{\"command\":\"session list\",\"limit\":5}" "${list_output}"
if ! echo "${list_output}" | grep -q "${session_id}"; then
  echo "CASE cli_app_server_session_list FAIL output=${list_output}"
  exit 1
fi
echo "CASE cli_app_server_session_list PASS"

smoke_output="$(run_cli app-server-smoke --scenario smoke)"
log_case_json "cli_app_server_smoke" "{\"command\":\"app-server-smoke\",\"scenario\":\"smoke\"}" "${smoke_output}"
if ! echo "${smoke_output}" | grep -q "App-server scenario passed"; then
  echo "CASE cli_app_server_smoke FAIL output=${smoke_output}"
  exit 1
fi
echo "CASE cli_app_server_smoke PASS"

skills_list_output="$(run_cli skills list)"
log_case_json "cli_skills_list" "{\"command\":\"skills list\"}" "${skills_list_output}"
if ! echo "${skills_list_output}" | grep -Fq '$cli-review'; then
  echo "CASE cli_skills_list FAIL output=${skills_list_output}"
  exit 1
fi
echo "CASE cli_skills_list PASS"

skills_refresh_output="$(run_cli skills refresh)"
log_case_json "cli_skills_refresh" "{\"command\":\"skills refresh\"}" "${skills_refresh_output}"
if ! echo "${skills_refresh_output}" | grep -Fq '$cli-review'; then
  echo "CASE cli_skills_refresh FAIL output=${skills_refresh_output}"
  exit 1
fi
echo "CASE cli_skills_refresh PASS"

skills_suggest_output="$(run_cli skills suggest cli)"
log_case_json "cli_skills_suggest" "{\"command\":\"skills suggest\",\"query\":\"cli\"}" "${skills_suggest_output}"
if ! echo "${skills_suggest_output}" | grep -Fq '$cli-review'; then
  echo "CASE cli_skills_suggest FAIL output=${skills_suggest_output}"
  exit 1
fi
echo "CASE cli_skills_suggest PASS"

skills_read_output="$(run_cli skills read --name cli-review)"
log_case_json "cli_skills_read" "{\"command\":\"skills read\",\"name\":\"cli-review\"}" "${skills_read_output}"
if ! echo "${skills_read_output}" | grep -q "CLI smoke skill"; then
  echo "CASE cli_skills_read FAIL output=${skills_read_output}"
  exit 1
fi
echo "CASE cli_skills_read PASS"
