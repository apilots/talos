#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
for command in cargo curl openssl protoc python3 pnpm; do
  require_command "${command}"
done
export TALOS_E2E_CADDY_BIN="${TALOS_E2E_CADDY_BIN:-caddy}"
require_command "${TALOS_E2E_CADDY_BIN}"
python3 -c 'import grpc, yaml; from google.protobuf import descriptor_pool' || {
  echo 'CASE computer_preflight FAIL install grpcio, protobuf and PyYAML' >&2
  exit 1
}
# This slice exercises registration only; no model credential is required.
build_server_binary
(cd "${TALOS_RS_DIR}" && cargo build -q -p app-server)
export TALOS_CLOUD_SERVER_BIN="$(resolve_cloud_server_binary)"
export TALOS_SCENARIO_REPORT_DIR="${SCENARIO_REPORT_DIR}"
pnpm --dir "${TALOS_APP_DIR}/desktop" exec vite build --config "${SCRIPT_DIR}/../../scripts/computer_workspace/vite.config.ts"
python3 "${SCRIPT_DIR}/../../scripts/computer_workspace/registration.py"
