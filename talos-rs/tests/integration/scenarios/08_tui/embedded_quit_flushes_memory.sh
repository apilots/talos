#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
# shellcheck disable=SC1091
source "${ROOT_DIR}/tests/integration/scripts/common.sh"

load_env
build_binaries

cd "${ROOT_DIR}"
cargo test -p talos-ui-tui e2e_tests::embedded_quit_flush_tests::test_embedded_quit_flushes_memory_after_runtime_shutdown -- --ignored --nocapture
