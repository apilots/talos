#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
for command in cargo curl openssl protoc python3 pnpm; do require_command "${command}"; done
python3 -c 'import grpc, yaml, playwright; from google.protobuf import descriptor_pool'
build_server_binary
build_worker_binary
export TALOS_SCENARIO_REPORT_DIR="${SCENARIO_REPORT_DIR}"
python3 "${TALOS_E2E_DIR}/scripts/resource_preview/web.py"
