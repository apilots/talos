#!/usr/bin/env bash
set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/scripts/common.sh"
source "${TALOS_E2E_DIR}/scripts/sandbox.sh"

run_linux_docker_sandbox_group
