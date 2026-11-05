#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/tui.sh"
build_cli_binary
fixture="${TALOS_E2E_DIR}/scripts/automation_model_fixture.py"
driver="${TALOS_E2E_DIR}/scripts/automation_tui.py"
case_dir="$(make_e2e_temp_dir automation-tui)"
fixture_pid=""
TUI_SESSION_NAME="talos-automation-$$"
cleanup() {
  if tmux has-session -t "${TUI_SESSION_NAME}" 2>/dev/null; then
    tui_capture "${TUI_SESSION_NAME}" >"${SCENARIO_REPORT_DIR}/terminal.txt"
  fi
  tui_stop "${TUI_SESSION_NAME}"
  if [[ -n "${fixture_pid}" ]]; then
    terminate_pid "${fixture_pid}" TERM
    wait "${fixture_pid}" 2>/dev/null || true
  fi
  cp "${case_dir}"/*.jsonl "${SCENARIO_REPORT_DIR}/" 2>/dev/null || true
}
trap cleanup EXIT
python3 "${fixture}" serve "${case_dir}" >"${SCENARIO_REPORT_DIR}/fixture.log" 2>&1 &
fixture_pid=$!
for ((attempt=0; attempt<100; attempt++)); do
  [[ ! -f "${case_dir}/port" ]] || break
  kill -0 "${fixture_pid}" 2>/dev/null || { cat "${SCENARIO_REPORT_DIR}/fixture.log" >&2; exit 1; }
  sleep 0.1
done
[[ -f "${case_dir}/port" ]] || { echo 'Controlled automation upstream did not start' >&2; exit 1; }
python3 "${fixture}" prepare "${REPO_ROOT}/conf/talos-dev.yaml" \
  "${case_dir}/talos.yaml" "$(cat "${case_dir}/port")"
export TALOS_HOME="${case_dir}/home" TALOS_TEST_CONFIG_TEMPLATE="${case_dir}/talos.yaml"
: >"${case_dir}/empty.env"
export TALOS_TEST_ENV_TEMPLATE="${case_dir}/empty.env"
export TALOS_TEST_INNER_MODEL_PROVIDER=automation-fixture TALOS_TEST_INNER_MODEL_ID=automation-controlled
prepare_talos_home
export TALOS_CONFIG="${TALOS_HOME}/conf/talos.yaml"
database="${TALOS_HOME}/data/talos.db"
mkdir -p "${case_dir}/workspace"
tmux new-session -d -s "${TUI_SESSION_NAME}" -x 140 -y 40 \
  "cd '${case_dir}/workspace' && '${TALOS_E2E_CLI_BIN}' 2>'${SCENARIO_REPORT_DIR}/stderr.txt'"
tui_wait_for_text "${TUI_SESSION_NAME}" 'Type a message' 120 0.25

wait_for_gate() {
  local gate="$1"
  for ((attempt=0; attempt<600; attempt++)); do
    [[ ! -f "${case_dir}/${gate}" ]] || return 0
    [[ ! -f "${case_dir}/errors.jsonl" ]] || { cat "${case_dir}/errors.jsonl" >&2; return 1; }
    sleep 0.1
  done
  echo "Automation gate did not arrive: ${gate}" >&2
  return 1
}

wait_for_empty_panel() {
  local snapshot="$1"
  for ((attempt=0; attempt<200; attempt++)); do
    tui_capture "${TUI_SESSION_NAME}" >"${snapshot}"
    if ! grep -Eq 'Tasks [0-9]+ · Waiting' "${snapshot}"; then
      return 0
    fi
    sleep 0.1
  done
  echo 'Ended automation still occupies the TUI task panel' >&2
  return 1
}

goal="goal_$(python3 -c 'import uuid; print(uuid.uuid4().hex)')"
loop="dynamicstop_$(python3 -c 'import uuid; print(uuid.uuid4().hex)')"
goal_prompt="AUTO_CASE=${goal} controlled automation work"
loop_prompt="AUTO_CASE=${loop} controlled automation work; check progress; model decides the next check time"

# Both entries use the original bracketed-paste and slash/SkillSelection path.
tui_send_input "${TUI_SESSION_NAME}" "/goal ${goal_prompt}"
wait_for_gate "${goal}.work-1"
tmux send-keys -t "${TUI_SESSION_NAME}" C-g
tui_wait_for_text "${TUI_SESSION_NAME}" "◎ Goal · AUTO_CASE=${goal}" 120 0.1 \
  "${SCENARIO_REPORT_DIR}/goal-running.txt"
touch "${case_dir}/${goal}.release-work-1"
tui_wait_for_text "${TUI_SESSION_NAME}" 'Goal completion declared; settle this Turn normally' 240 0.1
python3 "${driver}" await-terminal "${database}" goal "${goal}"
wait_for_empty_panel "${SCENARIO_REPORT_DIR}/goal-complete.txt"

tui_send_input "${TUI_SESSION_NAME}" "/loop ${loop_prompt}"
wait_for_gate "${loop}.work-1"
tui_wait_for_text "${TUI_SESSION_NAME}" "↻ Loop · AUTO_CASE=${loop}" 120 0.1 \
  "${SCENARIO_REPORT_DIR}/loop-running.txt"
touch "${case_dir}/${loop}.release-work-1"
wait_for_gate "${loop}.declared"
touch "${case_dir}/${loop}.release-declared"
tui_wait_for_text "${TUI_SESSION_NAME}" 'Model Stop declaration settles with the current Loop Turn' 240 0.1
python3 "${driver}" await-terminal "${database}" loop "${loop}"
wait_for_empty_panel "${SCENARIO_REPORT_DIR}/loop-stopped.txt"
python3 "${driver}" verify "${database}" "${goal}" "${loop}" \
  "${SCENARIO_REPORT_DIR}/result.json"
[[ ! -f "${case_dir}/errors.jsonl" ]] || { cat "${case_dir}/errors.jsonl" >&2; exit 1; }
tui_send_command "${TUI_SESSION_NAME}" '/quit'
tui_wait_for_exit "${TUI_SESSION_NAME}"
echo 'CASE automation_tui_entry PASS entry=slash selection=original stream=automatic timing=real10s terminal=removed'
