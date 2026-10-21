#!/usr/bin/env bash
set -euo pipefail

TALOS_E2E_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${TALOS_E2E_DIR}/scripts/common.sh"

SCENARIO_ROOT="${TALOS_E2E_DIR}/scenarios"
RUNNER="${TALOS_E2E_DIR}/scripts/runner.py"

usage() {
  cat <<'EOF'
Usage:
  talos-e2e/run_e2e.sh [--list | --check]
  talos-e2e/run_e2e.sh --group 06_tools [--list | --check]
  talos-e2e/run_e2e.sh --case 05_cli/full_flow [--list | --check]

Options:
  --group <name>   Select one discovered numbered group
  --case <name>    Select one discovered case by relative path or unique basename
  --list          List selected paths only; no .env, preflight, builds or reports
  --check         Check selected environment prerequisites without running cases

Without a selector, discover every numbered root case and group. Missing
prerequisites block execution; scenarios are never silently omitted.
EOF
}

build_required_binaries() {
  local binaries binary
  binaries="$(python3 "${RUNNER}" binaries "${SCENARIO_ROOT}" "${selected_cases[@]}")"
  while IFS= read -r binary; do
    case "${binary}" in
      server) build_server_binary ;;
      cli) build_cli_binary ;;
      worker) build_worker_binary ;;
      app_server) prepare_binary TALOS_E2E_APP_SERVER_BIN app-server app-server ;;
      test_client) prepare_binary TALOS_E2E_APP_SERVER_TEST_CLIENT_BIN talos-app-server-test-client talos-app-server-test-client ;;
      sandbox) if [[ "$(uname -s)" == "Linux" ]]; then build_sandbox_binary; fi ;;
    esac
  done <<< "${binaries}"
  # Linux execution discovers the sandbox launcher beside runtime binaries.
  if [[ "$(uname -s)" == "Linux" && -n "${binaries}" ]]; then build_sandbox_binary; fi
  export TALOS_E2E_REQUIRED_BINARIES_READY=1
}

reset_case_directory() {
  local root_dir="$1"
  local case_dir="$2"

  case "${case_dir}" in
    "${root_dir}/"?*) ;;
    *)
      echo "Refusing to reset case directory outside ${root_dir}: ${case_dir}" >&2
      exit 1
      ;;
  esac
  rm -rf "${case_dir}"
}

declare -a scenarios=() selected_cases=()
case_name=""
group_name=""
list_only=0
check_only=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --case|--group)
      if [[ -z "${2:-}" || "${2}" == -* || -n "${case_name}${group_name}" ]]; then
        echo "Provide exactly one --case or --group selector with a value" >&2
        exit 2
      fi
      if [[ "$1" == --case ]]; then case_name="$2"; else group_name="$2"; fi
      shift 2
      ;;
    --list) list_only=1; shift ;;
    --check) check_only=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done
if [[ "${list_only}" == 1 && "${check_only}" == 1 ]]; then
  echo "--list and --check are mutually exclusive" >&2
  exit 2
fi
command -v python3 >/dev/null || { echo "Python 3.9+ is required for scenario discovery" >&2; exit 2; }
selection="$(python3 "${RUNNER}" select "${SCENARIO_ROOT}" "${group_name}" "${case_name}")"
while IFS= read -r scenario; do
  selected_cases+=("${scenario}")
  scenarios+=("${SCENARIO_ROOT}/${scenario}")
done <<< "${selection}"
if [[ "${list_only}" == 1 ]]; then
  printf '%s\n' "${selected_cases[@]}"
  exit 0
fi

load_env
# Preserve the existing standalone sandbox case's extra Linux acceptance.
export TALOS_E2E_SINGLE_CASE=""
if [[ -n "${case_name}" ]]; then export TALOS_E2E_SINGLE_CASE="${selected_cases[0]}"; fi
timestamp="$(date '+%Y%m%d_%H%M%S')"
export TALOS_E2E_RUN_ID="${timestamp}-$$"
report_file="${REPORT_ROOT}/e2e_report_${TALOS_E2E_RUN_ID}.md"
latest_file="${REPORT_ROOT}/latest_report.md"
mkdir -p "${REPORT_ROOT}"
preflight_log="${REPORT_ROOT}/preflight_${TALOS_E2E_RUN_ID}.log"
if ! python3 "${RUNNER}" check "${SCENARIO_ROOT}" "${selected_cases[@]}" >"${preflight_log}" 2>&1; then
  cat "${preflight_log}" >&2
  echo "E2E blocked before build/execution. Prerequisites: ${preflight_log}" >&2
  exit 1
fi
cat "${preflight_log}"
if [[ "${check_only}" == 1 ]]; then exit 0; fi
mkdir -p "${TALOS_E2E_TMP_ROOT}"
export npm_config_cache="${TALOS_E2E_NPM_CACHE:-${TALOS_E2E_TMP_ROOT}/npm-cache}"
mkdir -p "${npm_config_cache}"
export TALOS_E2E_RESOURCE_ROOT="${TALOS_E2E_TMP_ROOT}/runs/${TALOS_E2E_RUN_ID}"
export PYTHONPATH="${TALOS_E2E_DIR}/scripts${PYTHONPATH:+:${PYTHONPATH}}"
mkdir -p "${TALOS_E2E_RESOURCE_ROOT}/shared"
active_case=""
finish_run() {
  local status=$?
  trap - EXIT
  trap '' INT TERM
  if [[ -n "${active_case}" ]]; then
    kill -TERM "${active_case}" 2>/dev/null || true
    wait "${active_case}" 2>/dev/null || true
  fi
  if python3 "${TALOS_E2E_DIR}/scripts/lifecycle.py" cleanup; then
    rm -rf "${TALOS_E2E_RESOURCE_ROOT}"
  else
    echo "Run cleanup failed; ownership records retained: ${TALOS_E2E_RESOURCE_ROOT}" >&2
    if [[ -f "${report_file}" ]]; then
      printf '\n## Run cleanup\n\nFAIL: owned resources remain in `%s`.\n' "${TALOS_E2E_RESOURCE_ROOT}" >> "${report_file}"
    fi
    [[ "${status}" != 0 ]] || status=1
  fi
  if [[ -f "${report_file}" ]]; then cp "${report_file}" "${latest_file}"; fi
  if [[ "${status}" == 0 ]]; then echo "E2E passed. Report: ${report_file}"; fi
  exit "${status}"
}
trap finish_run EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
build_required_binaries

pass_count=0
fail_count=0

{
  echo "# Talos 端到端集成测试报告"
  echo
  echo "- 执行时间: $(date '+%Y-%m-%d %H:%M:%S %z')"
  echo "- 工作目录: ${REPO_ROOT}"
  echo "- E2E 目录: ${TALOS_E2E_DIR}"
  echo "- Rust 目录: ${TALOS_RS_DIR}"
  echo "- 服务端: ${TALOS_E2E_SERVER_BIN:-not required}"
  echo "- 终端客户端: ${TALOS_E2E_CLI_BIN:-not required}"
  echo
  echo "## 场景执行结果"
  echo
} > "${report_file}"

for scenario in "${scenarios[@]}"; do
  script_path="${scenario}"
  scenario_name="${script_path#${SCENARIO_ROOT}/}"
  scenario_report_dir="${REPORT_ROOT}/${scenario_name%.sh}"
  log_file="${scenario_report_dir}/scenario.log"
  reset_case_directory "${REPORT_ROOT}" "${scenario_report_dir}"
  mkdir -p "${scenario_report_dir}"
  scenario_started_at="$(python3 - <<'PY'
import time
print(time.time())
PY
)"
  chmod +x "${script_path}"
  echo "Running ${scenario_name}..."
  # Full runs use the same group semantics as --group, without repeating
  # each native sandbox case's standalone Docker expansion before case99.
  effective_group="${group_name}"
  if [[ -z "${case_name}" && "${scenario_name}" == */* ]]; then effective_group="${scenario_name%%/*}"; fi
  TALOS_E2E_CASE_ID="${scenario_name%.sh}" TALOS_E2E_GROUP_NAME="${effective_group}" TALOS_SCENARIO_NAME="${scenario_name%.sh}" TALOS_SCENARIO_REPORT_DIR="${scenario_report_dir}" python3 "${TALOS_E2E_DIR}/scripts/case_run.py" "${script_path}" >"${log_file}" 2>&1 &
  active_case=$!
  if wait "${active_case}"; then
    status="PASS"
    pass_count=$((pass_count + 1))
  else
    status="FAIL"
    fail_count=$((fail_count + 1))
  fi
  active_case=""
  scenario_elapsed_secs="$(SCENARIO_STARTED_AT="${scenario_started_at}" python3 - <<'PY'
import os
import time
started = float(os.environ["SCENARIO_STARTED_AT"])
print(f"{time.time() - started:.2f}")
PY
)"
  echo "Finished ${scenario_name} (${status}) in ${scenario_elapsed_secs}s"

  {
    echo "### 场景 ${scenario_name}"
    echo
    echo "- 状态: ${status}"
    echo "- 耗时(秒): ${scenario_elapsed_secs}"
    echo "- 日志: \`${log_file}\`"
    echo
    echo '```text'
    cat "${log_file}"
    echo '```'
    echo
  } >> "${report_file}"
done

{
  echo "## 汇总"
  echo
  echo "- 场景总数: ${#scenarios[@]}"
  echo "- 通过: ${pass_count}"
  echo "- 失败: ${fail_count}"
} >> "${report_file}"

cp "${report_file}" "${latest_file}"

if [[ "${fail_count}" -gt 0 ]]; then
  echo "E2E failed. Report: ${report_file}" >&2
  exit 1
fi
