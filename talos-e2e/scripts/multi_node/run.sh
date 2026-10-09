#!/usr/bin/env bash
set -euo pipefail
MULTI_NODE_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${MULTI_NODE_SCRIPT_DIR}/../common.sh"
for command in cargo curl openssl protoc python3; do
  require_command "${command}"
done
python3 -c 'import grpc, yaml; from google.protobuf import descriptor_pb2, descriptor_pool, json_format, message_factory' || {
  echo 'CASE multi_node_preflight FAIL install Python grpcio, protobuf and PyYAML'
  exit 1
}
if [[ "$1" != lease_expiry ]] && ! has_real_llm_key; then
  echo 'CASE multi_node_preflight FAIL real model provider key required in .env'
  exit 1
fi
build_server_binary
build_worker_binary
MULTI_NODE_CLOUD_ROOT="${TALOS_CLOUD_REPO:-${REPO_ROOT}/../talos-cloud}"
(cd "${MULTI_NODE_CLOUD_ROOT}/cloud-rs" && cargo build -q -p cloud-server)
python3 "${MULTI_NODE_SCRIPT_DIR}/scenarios.py" "$1"
