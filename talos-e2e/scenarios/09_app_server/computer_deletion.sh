#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
for command in cargo curl openssl protoc python3 pnpm; do require_command "${command}"; done
python3 -c 'import grpc, yaml, playwright; from google.protobuf import descriptor_pool'
build_server_binary
export TALOS_SCENARIO_REPORT_DIR="${SCENARIO_REPORT_DIR}"
# Public PRs use a deterministic Auth boundary; no private Cloud checkout or secrets.
# Local release acceptance also runs deletion.py against a real disposable Cloud fixture.
python3 "${SCRIPT_DIR}/../../scripts/computer_workspace/deletion.py" --contract-auth
