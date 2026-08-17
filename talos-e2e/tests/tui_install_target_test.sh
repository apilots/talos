#!/usr/bin/env bash
set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TALOS_E2E_DIR="$(cd "${TEST_DIR}/.." && pwd)"
# shellcheck disable=SC1091
source "${TALOS_E2E_DIR}/scripts/common.sh"
# shellcheck disable=SC1091
source "${TALOS_E2E_DIR}/scripts/tui_install.sh"

assert_equal() {
  local expected="$1"
  local actual="$2"
  local label="$3"

  if [[ "${actual}" != "${expected}" ]]; then
    echo "Expected ${label} to be ${expected}, got ${actual}" >&2
    exit 1
  fi
}

assert_equal "linux" "$(normalize_tui_install_platform Linux)" "Linux platform"
assert_equal "macos" "$(normalize_tui_install_platform Darwin)" "macOS platform"
assert_equal "amd64" "$(normalize_tui_install_arch x86_64)" "x86_64 architecture"
assert_equal "amd64" "$(normalize_tui_install_arch amd64)" "amd64 architecture"
assert_equal "arm64" "$(normalize_tui_install_arch arm64)" "arm64 architecture"
assert_equal "arm64" "$(normalize_tui_install_arch aarch64)" "aarch64 architecture"

if normalize_tui_install_platform Windows >/dev/null 2>&1; then
  echo "Unsupported platforms must fail normalization" >&2
  exit 1
fi
if normalize_tui_install_arch riscv64 >/dev/null 2>&1; then
  echo "Unsupported architectures must fail normalization" >&2
  exit 1
fi

TUI_INSTALL_EXPECTED_PLATFORM=linux \
  TUI_INSTALL_EXPECTED_ARCH=amd64 \
  verify_tui_install_runner linux amd64 >/dev/null

if TUI_INSTALL_EXPECTED_PLATFORM=linux \
  verify_tui_install_runner linux amd64 >/dev/null 2>&1; then
  echo "An incomplete expected runner contract must fail" >&2
  exit 1
fi

if TUI_INSTALL_EXPECTED_PLATFORM=macos \
  TUI_INSTALL_EXPECTED_ARCH=arm64 \
  verify_tui_install_runner linux arm64 >/dev/null 2>&1; then
  echo "A platform mismatch must fail" >&2
  exit 1
fi

if TUI_INSTALL_EXPECTED_PLATFORM=linux \
  TUI_INSTALL_EXPECTED_ARCH=amd64 \
  verify_tui_install_runner linux arm64 >/dev/null 2>&1; then
  echo "An architecture mismatch must fail" >&2
  exit 1
fi

echo "TUI installation target tests passed."
