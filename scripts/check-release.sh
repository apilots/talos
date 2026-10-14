#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
just test-commit-messages
just test-release-notes
bash scripts/check-release-version-tests.sh
python3 scripts/check_error_contract_tests.py
just fmt-check
just check
just test
pnpm --dir talos-app ci:frontend-app
for product in server tui; do
  product_dir="${PWD}/output/release-check-products/${product}"
  PROFILE=dev PRODUCT_DIR="${product_dir}" ./build.sh "${product}"
  PROFILE=dev PRODUCT_DIR="${product_dir}" \
    bash build/scripts/verify-release-artifacts.sh "${product}"
done
deploy/install/tests/install_test.sh
deploy/install/tests/release_contract_test.sh
talos-e2e/tests/tui_install_target_test.sh
