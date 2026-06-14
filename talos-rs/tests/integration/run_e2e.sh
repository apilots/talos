#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
build_binaries

timestamp="$(date '+%Y%m%d_%H%M%S')"
export TALOS_E2E_TMP_ROOT="${TALOS_E2E_TMP_ROOT:-${REPO_ROOT}/.tmp/e2e}"
report_file="${ROOT_DIR}/tests/integration/reports/e2e_report_${timestamp}.md"
latest_file="${ROOT_DIR}/tests/integration/reports/latest_report.md"
mkdir -p "${ROOT_DIR}/tests/integration/reports"
mkdir -p "${TALOS_E2E_TMP_ROOT}"

SCENARIO_ROOT="${ROOT_DIR}/tests/integration/scenarios"

usage() {
  cat <<'EOF'
Usage:
  tests/integration/run_e2e.sh
  tests/integration/run_e2e.sh --group 06_tools
  tests/integration/run_e2e.sh --case 05_cli/full_flow

Options:
  --group <name>   Run all cases under scenarios/<name>/
  --case <name>    Run one case by relative path or basename without .sh
EOF
}

find_case_path() {
  local case_name="$1"

  if [[ -f "${SCENARIO_ROOT}/${case_name}.sh" ]]; then
    printf '%s\n' "${SCENARIO_ROOT}/${case_name}.sh"
    return 0
  fi

  if [[ -f "${SCENARIO_ROOT}/${case_name}" ]]; then
    printf '%s\n' "${SCENARIO_ROOT}/${case_name}"
    return 0
  fi

  find "${SCENARIO_ROOT}" -type f -name "${case_name}.sh" | sort
}

collect_group_cases() {
  local group_name="$1"
  local group_dir="${SCENARIO_ROOT}/${group_name}"

  if [[ ! -d "${group_dir}" ]]; then
    echo "Unknown scenario group: ${group_name}" >&2
    exit 1
  fi

  find "${group_dir}" -type f -name '*.sh' | sort
}

append_lines_to_array() {
  local line
  while IFS= read -r line; do
    [[ -n "${line}" ]] || continue
    scenarios+=("${line}")
  done
}

declare -a scenarios=()
case_name=""
group_name=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --case)
      case_name="${2:-}"
      shift 2
      ;;
    --group)
      group_name="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ -n "${case_name}" && -n "${group_name}" ]]; then
  echo "--case and --group are mutually exclusive" >&2
  exit 1
fi

if [[ -n "${case_name}" ]]; then
  append_lines_to_array < <(find_case_path "${case_name}")
  if [[ "${#scenarios[@]}" -eq 0 ]]; then
    echo "Unknown scenario case: ${case_name}" >&2
    exit 1
  fi
  if [[ "${#scenarios[@]}" -gt 1 ]]; then
    echo "Ambiguous scenario case: ${case_name}" >&2
    printf 'Matched:\n' >&2
    printf '  %s\n' "${scenarios[@]}" >&2
    exit 1
  fi
elif [[ -n "${group_name}" ]]; then
  append_lines_to_array < <(collect_group_cases "${group_name}")
else
  scenarios=(
    "${SCENARIO_ROOT}/01_health_and_models.sh"
    "${SCENARIO_ROOT}/02_session_lifecycle.sh"
    "${SCENARIO_ROOT}/03_messages_http_sse.sh"
  )
  append_lines_to_array < <(collect_group_cases "05_cli")
  append_lines_to_array < <(collect_group_cases "06_tools")
  append_lines_to_array < <(collect_group_cases "07_memory")
  append_lines_to_array < <(collect_group_cases "08_tui")
  append_lines_to_array < <(collect_group_cases "09_app_server")
  append_lines_to_array < <(collect_group_cases "10_talos_app")
  append_lines_to_array < <(collect_group_cases "11_auth")
fi

pass_count=0
fail_count=0

{
  echo "# Talos 端到端集成测试报告"
  echo
  echo "- 执行时间: $(date '+%Y-%m-%d %H:%M:%S %z')"
  echo "- 工作目录: ${ROOT_DIR}"
  echo "- 服务端: target/debug/talos-server"
  echo "- 客户端 CLI: target/debug/talos-cli"
  echo
  echo "## 场景执行结果"
  echo
} > "${report_file}"

export TALOS_BINARIES_READY=1

for scenario in "${scenarios[@]}"; do
  script_path="${scenario}"
  scenario_name="${script_path#${SCENARIO_ROOT}/}"
  scenario_report_dir="${ROOT_DIR}/tests/integration/reports/${scenario_name%.sh}"
  log_file="${scenario_report_dir}/scenario.log"
  mkdir -p "${scenario_report_dir}"
  scenario_started_at="$(python3 - <<'PY'
import time
print(time.time())
PY
)"
  chmod +x "${script_path}"
  echo "Running ${scenario_name}..."
  if TALOS_SCENARIO_NAME="${scenario_name%.sh}" TALOS_SCENARIO_REPORT_DIR="${scenario_report_dir}" "${script_path}" >"${log_file}" 2>&1; then
    status="PASS"
    pass_count=$((pass_count + 1))
  else
    status="FAIL"
    fail_count=$((fail_count + 1))
  fi
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

echo "E2E passed. Report: ${report_file}"
