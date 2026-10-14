#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
check_stage() {
  echo "Release source checks: $1"
  case "$1" in
    contracts)
      just test-commit-messages
      just test-release-notes
      bash scripts/check-release-version-tests.sh
      python3 scripts/check_error_contract_tests.py
      ;;
    rust-format) just fmt-check ;;
    rust-check) just check ;;
    rust-tests) just test ;;
    frontend) pnpm --dir talos-app ci:frontend-app ;;
    installer)
      deploy/install/tests/install_test.sh
      deploy/install/tests/release_contract_test.sh
      talos-e2e/tests/tui_install_target_test.sh
      ;;
    *) echo "Unknown release source stage: $1" >&2; exit 2 ;;
  esac
}

if [[ "${1:-all}" == "all" ]]; then
  for stage in contracts rust-format rust-check rust-tests frontend installer; do
    check_stage "${stage}"
  done
else
  check_stage "$1"
fi
