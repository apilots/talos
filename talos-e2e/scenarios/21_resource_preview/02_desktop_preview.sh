#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
export TALOS_SCENARIO_REPORT_DIR="${SCENARIO_REPORT_DIR}"
export TALOS_E2E_CADDY_BIN="${TALOS_E2E_CADDY_BIN:-caddy}"
for command in cargo node pnpm openssl protoc python3 "${TALOS_E2E_CADDY_BIN}"; do require_command "${command}"; done
python3 -c 'import grpc, yaml, playwright; from google.protobuf import descriptor_pool'
build_server_binary
prepare_binary TALOS_E2E_APP_SERVER_BIN app-server app-server
pnpm --dir "${TALOS_APP_DIR}/desktop" build:main
python3 "${TALOS_E2E_DIR}/scripts/resource_preview/desktop.py"
