#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export TALOS_RESOURCE_DIR="${TALOS_RESOURCE_DIR:-${SCRIPT_DIR}}"
exec "${SCRIPT_DIR}/bin/talos-server" "$@"
