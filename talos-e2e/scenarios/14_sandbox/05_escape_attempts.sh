#!/usr/bin/env bash
set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/scripts/common.sh"
source "${TALOS_E2E_DIR}/scripts/sandbox.sh"
run_sandbox_case escape_attempts
