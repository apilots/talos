#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

require_command curl
case_dir="$(make_e2e_temp_dir model-protocol)"
fixture="${TALOS_E2E_DIR}/scripts/model_protocol_fixture.py"
fixture_pid=""
evidence_dir="$(scenario_report_path "protocol-fixture-$(basename "${case_dir}")")"
mkdir -p "${evidence_dir}"
cleanup() {
  stop_server
  if [[ -n "${fixture_pid}" ]]; then
    terminate_pid "${fixture_pid}" TERM
    wait "${fixture_pid}" 2>/dev/null || true
  fi
  for artifact in "${case_dir}"/*.json "${case_dir}"/*.jsonl "${case_dir}"/fixture.log; do
    [[ ! -f "${artifact}" ]] || cp "${artifact}" "${evidence_dir}/"
  done
}
trap cleanup EXIT
python3 "${fixture}" serve "${case_dir}" >"${case_dir}/fixture.log" 2>&1 &
fixture_pid=$!
for ((attempt=0; attempt<100; attempt++)); do
  [[ -f "${case_dir}/port" ]] && break
  kill -0 "${fixture_pid}" 2>/dev/null || { cat "${case_dir}/fixture.log" >&2; exit 1; }
  sleep 0.1
done
[[ -f "${case_dir}/port" ]] || { echo "Protocol fixture startup timed out" >&2; exit 1; }
python3 "${fixture}" prepare "${TALOS_TEST_CONFIG_TEMPLATE:-${REPO_ROOT}/conf/talos-dev.yaml}" \
  "${case_dir}/talos.yaml" "$(cat "${case_dir}/port")" "${case_dir}/plan.tsv"
export TALOS_TEST_CONFIG_TEMPLATE="${case_dir}/talos.yaml"
export TALOS_TEST_INNER_MODEL_PROVIDER="chat-protocol-default"
export TALOS_TEST_INNER_MODEL_ID="opaque-7f82"
# This case never copies a developer's real provider credentials into its home.
: >"${case_dir}/empty.env"
export TALOS_TEST_ENV_TEMPLATE="${case_dir}/empty.env"
start_server
mkdir -p "${case_dir}/workspace"

last_provider=""
session_id=""
while IFS=$'\t' read -r provider mode; do
  if [[ "${provider}" != "${last_provider}" ]]; then
    session_id="$(new_session_id)"
    last_provider="${provider}"
  fi
  payload="$(python3 "${fixture}" payload "${provider}" "${mode}" "${case_dir}/workspace")"
  send_message_response "${session_id}" "${payload}" "${provider}-${mode}" >"${case_dir}/${provider}-${mode}.json"
done <"${case_dir}/plan.tsv"
python3 "${fixture}" verify "${GATEWAY_URL}" "${case_dir}"
printf 'DETAIL protocol_fixture_evidence=%s\n' "${evidence_dir}"
