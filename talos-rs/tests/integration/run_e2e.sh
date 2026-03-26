#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
"${ROOT_DIR}/tests/architecture/check_layering.sh"
build_binaries

timestamp="$(date '+%Y%m%d_%H%M%S')"
report_file="${ROOT_DIR}/tests/integration/reports/e2e_report_${timestamp}.md"
latest_file="${ROOT_DIR}/tests/integration/reports/latest_report.md"
mkdir -p "${ROOT_DIR}/tests/integration/reports"

scenarios=(
  "01_health_and_models.sh"
  "02_session_lifecycle.sh"
  "03_messages_http_sse.sh"
  "04_cli_full_flow.sh"
  "05_chat_bilingual_and_tools.sh"
  "06_cli_stream_thinking_and_tools.sh"
  "07_session_close_flushes_memory.sh"
)

pass_count=0
fail_count=0

{
  echo "# Talos 端到端集成测试报告"
  echo
  echo "- 执行时间: $(date '+%Y-%m-%d %H:%M:%S %z')"
  echo "- 工作目录: ${ROOT_DIR}"
  echo "- 服务端: target/debug/talos"
  echo "- 客户端 CLI: target/debug/talos-client"
  echo
  echo "## 场景执行结果"
  echo
} > "${report_file}"

for scenario in "${scenarios[@]}"; do
  script_path="${ROOT_DIR}/tests/integration/scenarios/${scenario}"
  log_file="${ROOT_DIR}/tests/integration/reports/${scenario%.sh}.log"
  chmod +x "${script_path}"
  echo "Running ${scenario}..."
  if "${script_path}" >"${log_file}" 2>&1; then
    status="PASS"
    pass_count=$((pass_count + 1))
  else
    status="FAIL"
    fail_count=$((fail_count + 1))
  fi

  {
    echo "### 场景 ${scenario}"
    echo
    echo "- 状态: ${status}"
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
