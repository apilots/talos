#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../common.sh"
for command in cargo curl openssl protoc python3 lsof; do require_command "${command}"; done
python3 -c 'import grpc, yaml; from google.protobuf import descriptor_pool'
if ! has_real_llm_key; then
  echo 'CASE outbound_preflight FAIL real model provider key required in .env' >&2
  exit 1
fi
if [[ "$1" == "oauth" ]]; then
  export TALOS_E2E_CADDY_BIN="${TALOS_E2E_CADDY_BIN:-caddy}"
  require_command "${TALOS_E2E_CADDY_BIN}"
fi
build_server_binary
build_worker_binary
(cd "${TALOS_CLOUD_REPO:-${REPO_ROOT}/../talos-cloud}/cloud-rs" && cargo build -q -p cloud-server)
python3 "${SCRIPT_DIR}/execution.py" "$1"
