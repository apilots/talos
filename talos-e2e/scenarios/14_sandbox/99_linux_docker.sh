#!/usr/bin/env bash
set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/scripts/common.sh"
source "${TALOS_E2E_DIR}/scripts/sandbox.sh"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "Linux Docker sandbox group is supported only from a macOS host." >&2
  exit 1
fi
run_linux_docker_sandbox_group
