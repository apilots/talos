#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/tui.sh"
fixture="${TALOS_E2E_DIR}/scripts/model_protocol_fixture.py"
case_dir="$(make_e2e_temp_dir tui-controlled)"
fixture_pid=""
TUI_SESSION_NAME="talos-controlled-$$"
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
[[ -f "${case_dir}/port" ]] || { echo "Controlled upstream did not start" >&2; exit 1; }
python3 "${fixture}" prepare "${REPO_ROOT}/conf/talos-dev.yaml" \
  "${case_dir}/talos.yaml" "$(cat "${case_dir}/port")" "${case_dir}/plan.tsv"
export TALOS_HOME="${case_dir}/home" TALOS_TEST_CONFIG_TEMPLATE="${case_dir}/talos.yaml"
: >"${case_dir}/empty.env"
export TALOS_TEST_ENV_TEMPLATE="${case_dir}/empty.env"
export TALOS_TEST_INNER_MODEL_PROVIDER=chat-protocol-default TALOS_TEST_INNER_MODEL_ID=opaque-7f82
prepare_talos_home
mkdir -p "${case_dir}/workspace"
tmux new-session -d -s "${TUI_SESSION_NAME}" -x 110 -y 38 \
  "cd '${case_dir}/workspace' && '${TALOS_E2E_CLI_BIN}' 2>'${SCENARIO_REPORT_DIR}/stderr.txt'"
tui_wait_for_text "${TUI_SESSION_NAME}" 'Type a message' 120 0.25
# The existing controlled provider exercises the actual Agent/Bash streaming loop.
tui_send_input "${TUI_SESSION_NAME}" 'PROTOCOL_CASE=chat-protocol-default:on. Run the controlled Bash tool and return its result.'
tui_wait_for_text "${TUI_SESSION_NAME}" 'PROTOCOL_OK:call_chat-protocol-default_on' 240 0.25 "${SCENARIO_REPORT_DIR}/terminal.txt"
# Completion text can precede TurnCompleted; wait for the actual idle UI state.
tui_wait_for_text "${TUI_SESSION_NAME}" 'last ·' 240 0.1 "${SCENARIO_REPORT_DIR}/terminal.txt"
tmux resize-window -t "${TUI_SESSION_NAME}" -x 80 -y 38
tui_wait_for_text "${TUI_SESSION_NAME}" 'PROTOCOL_OK:call_chat-protocol-default_on' 40 0.1
python3 - "${case_dir}" "${TALOS_HOME}/data/talos.db" <<'PY'
import json,sqlite3,sys
from pathlib import Path
root=Path(sys.argv[1])
assert not (root/'errors.jsonl').exists(), 'Upstream contract failed'
requests=[json.loads(line) for line in (root/'requests.jsonl').read_text().splitlines()]
assert len(requests)==2 and [r['followup'] for r in requests]==[False,True], requests
with sqlite3.connect(sys.argv[2]) as db:
    payloads=[json.loads(row[0]) for row in db.execute('SELECT payload_json FROM agent_events WHERE sequence IS NOT NULL')]
    events=json.dumps(payloads)
results=[block['content']['data'] for event in payloads if event['type']=='messageCompleted'
         for block in event['data']['blocks'] if block['content']['type']=='toolResult']
assert results, 'Tool result not persisted'
assert all(r['metrics']['finishedAtMs'] is not None and r['metrics']['executionDurationMs'] is not None for r in results), results
assert any(event['type']=='turnCompleted' for event in payloads), 'Turn not completed'
assert 'PROTOCOL_OK:call_chat-protocol-default_on' in events, 'Completed answer not persisted'
assert 'TOOL_chat-protocol-default_on' in events, 'Bash result not persisted'
PY
tui_send_command "${TUI_SESSION_NAME}" '/quit'
tui_wait_for_exit "${TUI_SESSION_NAME}"
if grep -Fq '[tracing-subscriber] Unable to write an event' "${SCENARIO_REPORT_DIR}/stderr.txt"; then
  cat "${SCENARIO_REPORT_DIR}/stderr.txt" >&2
  exit 1
fi
echo 'CASE controlled_tui_chat PASS streaming=real tool=Bash persistence=verified resize=verified'
