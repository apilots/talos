#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
build_app_server_binaries
build_cli_binary
case_dir="$(make_e2e_temp_dir automation)"
fixture="${TALOS_E2E_DIR}/scripts/automation_model_fixture.py"
fixture_pid=""
evidence="$(scenario_report_path automation-business)"
mkdir -p "${evidence}"
cleanup() {
  if [[ -n "${fixture_pid}" ]]; then
    terminate_pid "${fixture_pid}" TERM
    wait "${fixture_pid}" 2>/dev/null || true
  fi
  cp "${case_dir}"/*.json "${case_dir}"/*.jsonl "${evidence}/" 2>/dev/null || true
}
trap cleanup EXIT
python3 "${fixture}" serve "${case_dir}" >"${evidence}/model.log" 2>&1 &
fixture_pid=$!
for ((attempt=0; attempt<100; attempt++)); do
  [[ ! -f "${case_dir}/port" ]] || break
  kill -0 "${fixture_pid}" 2>/dev/null || { cat "${evidence}/model.log" >&2; exit 1; }
  sleep 0.1
done
[[ -f "${case_dir}/port" ]] || { echo 'Controlled model failed to start' >&2; exit 1; }
python3 "${fixture}" prepare "${REPO_ROOT}/conf/talos-dev.yaml" "${case_dir}/talos.yaml" "$(cat "${case_dir}/port")"
: >"${case_dir}/empty.env"
export TALOS_TEST_CONFIG_TEMPLATE="${case_dir}/talos.yaml" TALOS_TEST_ENV_TEMPLATE="${case_dir}/empty.env"
export TALOS_TEST_INNER_MODEL_PROVIDER=automation-fixture TALOS_TEST_INNER_MODEL_ID=automation-controlled
export TALOS_APP_SERVER_TEST_PROVIDER=automation-fixture TALOS_APP_SERVER_TEST_MODEL=automation-controlled
export TALOS_AUTOMATION_FIXTURE_DIR="${case_dir}"
export TALOS_AUTOMATION_FIXTURE_CONFIG="${case_dir}/talos.json"
export TALOS_APP_SERVER_BIN="${TALOS_E2E_APP_SERVER_BIN}"
python3 "${TALOS_E2E_DIR}/scripts/automation_cli_acceptance.py" \
  --talos-bin "${TALOS_E2E_CLI_BIN}" --fixture-dir "${case_dir}" \
  --home "${case_dir}/cli-home" --config-template "${case_dir}/talos.yaml" \
  --report-dir "${evidence}/cli" >"${evidence}/cli.log" 2>&1 || {
  cat "${evidence}/cli.log" >&2
  exit 1
}
cat "${evidence}/cli.log"
for transport in app-server embedded; do
  export TALOS_HOME="${case_dir}/${transport}-home" TALOS_CONFIG="${case_dir}/${transport}-home/conf/talos.yaml"
  export TALOS_AUTOMATION_TRANSPORT="${transport}"
  prepare_talos_home
  scenario=automation-e2e
  [[ "${transport}" != embedded ]] || scenario=automation-embedded-e2e
  if ! timeout 360 "${TALOS_E2E_APP_SERVER_TEST_CLIENT_BIN}" "${scenario}" >"${evidence}/${transport}.log" 2>&1; then
    cat "${evidence}/${transport}.log" >&2
    exit 1
  fi
  cat "${evidence}/${transport}.log"
done
export TALOS_HOME="${case_dir}/restart-home" TALOS_CONFIG="${case_dir}/restart-home/conf/talos.yaml"
prepare_talos_home
timeout 90 "${TALOS_E2E_APP_SERVER_TEST_CLIENT_BIN}" automation-restart-e2e >"${evidence}/restart.log" 2>&1 || {
  cat "${evidence}/restart.log" >&2
  exit 1
}
cat "${evidence}/restart.log"
(cd "${REPO_ROOT}" && just test --locked -p talos-base --run-ignored only -E 'test(automation_worker_business)') >"${evidence}/remote-workers.log" 2>&1 || {
  cat "${evidence}/remote-workers.log" >&2
  exit 1
}
cat "${evidence}/remote-workers.log"
[[ ! -f "${case_dir}/errors.jsonl" ]] || { cat "${case_dir}/errors.jsonl" >&2; exit 1; }
echo 'CASE session_automation PASS model=controlled tool=real transports=app-server,embedded,direct,outbound cadence=real-minute'
