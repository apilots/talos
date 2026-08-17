#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/tui_install.sh"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "Linux Docker installation verification is supported only from a macOS host." >&2
  exit 1
fi

run_linux_docker_tui_install
