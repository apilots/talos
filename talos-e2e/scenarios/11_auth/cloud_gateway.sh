#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
for command in cargo curl openssl protoc python3 pnpm; do require_command "${command}"; done
python3 -c 'import grpc, yaml; from google.protobuf import descriptor_pool'
build_binaries
build_app_server_binaries
export TALOS_CLOUD_SERVER_BIN="$(resolve_cloud_server_binary)"
export TALOS_SCENARIO_REPORT_DIR="${SCENARIO_REPORT_DIR}"
pnpm --dir "${TALOS_APP_DIR}/desktop" exec vite build --config "${SCRIPT_DIR}/../../scripts/computer_workspace/vite.config.ts"
python3 "${SCRIPT_DIR}/../../scripts/computer_workspace/authentication.py"
