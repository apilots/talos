#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../common.sh"
for command in cargo docker openssl protoc python3 pnpm; do require_command "${command}"; done
[[ "$(uname -s)" == Darwin ]] || { echo 'CASE sync_native_mac_preflight FAIL requires a real macOS host' >&2; exit 1; }
python3 -c 'import grpc, yaml; from google.protobuf import descriptor_pool'
docker info >/dev/null
sync_source="${TALOS_SYNC_REPO:-${REPO_ROOT}/../talos-sync}"
worker_image="${TALOS_WORKER_E2E_IMAGE:-talos-worker:sync-e2e}"
sync_image="${TALOS_SYNC_E2E_IMAGE:-talos-sync:sync-e2e}"
if [[ "${1}" == nat ]]; then
  (cd "${REPO_ROOT}" && just docker-server-build "${TALOS_SERVER_E2E_IMAGE:-talos-server:sync-e2e}")
  docker build -f "${SCRIPT_DIR}/firewall.Dockerfile" -t talos-e2e-firewall:local "${SCRIPT_DIR}"
fi
(cd "${TALOS_RS_DIR}" && cargo build -q -p talos-server -p talos-worker-bin -p app-server)
(cd "${TALOS_CLOUD_REPO:-${REPO_ROOT}/../talos-cloud}/cloud-rs" && cargo build -q -p talos-cloud-server)
# Build the candidate once per runner invocation, then verify its exact image IDs.
# Rebuilding every case adds registry requests without testing different source.
image_marker="${E2E_TMP_ROOT}/computer-sync-images-${TALOS_E2E_RUN_ID:?}.ids"
if [[ ! -f "${image_marker}" ]]; then
  (cd "${REPO_ROOT}" && just docker-worker-build "${worker_image}")
  docker build -f "${sync_source}/build/server/Dockerfile" -t "${sync_image}" "${sync_source}"
  docker image inspect "${worker_image}" "${sync_image}" --format '{{.Id}}' > "${image_marker}"
fi
expected_images="$(cat "${image_marker}")"
current_images="$(docker image inspect "${worker_image}" "${sync_image}" --format '{{.Id}}')"
[[ "${current_images}" == "${expected_images}" ]] || { echo 'E2E candidate image changed during this run' >&2; exit 1; }
export TALOS_SCENARIO_REPORT_DIR="${SCENARIO_REPORT_DIR}"
export TALOS_WORKER_E2E_IMAGE="${worker_image}" TALOS_SYNC_E2E_IMAGE="${sync_image}"
pnpm --dir "${TALOS_APP_DIR}/desktop" exec vite build --config "${SCRIPT_DIR}/../computer_workspace/vite.config.ts"
python3 "${SCRIPT_DIR}/scenarios.py" "${1}"
