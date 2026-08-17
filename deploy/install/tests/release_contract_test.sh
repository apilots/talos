#!/usr/bin/env bash
set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${TEST_DIR}/../../.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/talos-release-contract.XXXXXX")"

cleanup() {
  rm -rf "${TEST_ROOT}"
}
trap cleanup EXIT

if PRODUCT_DIR="${TEST_ROOT}/products" \
  TUI_PACKAGE_VERSION="0.0.0-version-mismatch" \
  "${REPO_ROOT}/build.sh" tui >"${TEST_ROOT}/build.log" 2>&1; then
  echo "Mismatched TUI package and binary versions should fail" >&2
  exit 1
fi

grep -Fq "does not match Talos binary version" "${TEST_ROOT}/build.log" || {
  cat "${TEST_ROOT}/build.log" >&2
  echo "Version mismatch did not return the expected error" >&2
  exit 1
}

echo "Talos release contract tests passed."
