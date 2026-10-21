#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../common.sh"
for command in cargo docker git openssl protoc python3 pnpm; do require_command "${command}"; done
[[ "$(uname -s)" == Darwin ]] || { echo 'CASE sync_native_mac_preflight FAIL requires a real macOS host' >&2; exit 1; }
python3 -c 'import grpc, yaml; from google.protobuf import descriptor_pool'
docker info >/dev/null
worker_image="${TALOS_WORKER_E2E_IMAGE:-talos-worker:sync-e2e}"
# v0.1.3 release-manifest.json; matches the locked public SDK/types versions.
sync_image="${TALOS_SYNC_E2E_IMAGE:-ghcr.io/apilots/talos-sync:v0.1.3@sha256:3d022fa009e4b1c3d144906d4aa8e969b238c045c75970914ca158560b882318}"
[[ "${sync_image}" =~ @sha256:[a-f0-9]{64}$ ]] || { echo 'CASE sync_image_preflight FAIL requires an immutable image digest' >&2; exit 1; }
image_marker="${E2E_TMP_ROOT}/computer-sync-images-${TALOS_E2E_RUN_ID:?}.ids"
image_context=""
trap 'if [[ -n "${image_context}" ]]; then rm -rf "${image_context}"; fi' EXIT
# Both product Dockerfiles expect talos/ inside their context. Select only
# current-checkout image inputs, regardless of the worktree directory name.
if [[ ! -f "${image_marker}" || "${1}" == nat ]]; then
  image_context="$(make_e2e_temp_dir computer-sync-image-context)"
  python3 "${SCRIPT_DIR}/image_context.py" "${REPO_ROOT}" "${image_context}/talos"
fi
if [[ "${1}" == nat ]]; then
  docker build -f "${REPO_ROOT}/build/server/Dockerfile" -t "${TALOS_SERVER_E2E_IMAGE:-talos-server:sync-e2e}" "${image_context}"
  docker build -f "${SCRIPT_DIR}/firewall.Dockerfile" -t talos-e2e-firewall:local "${SCRIPT_DIR}"
fi
(cd "${TALOS_RS_DIR}" && cargo build -q -p talos-server -p talos-worker-bin -p app-server)
export TALOS_CLOUD_SERVER_BIN="$(resolve_cloud_server_binary)"
# Build the candidate once per runner invocation, then verify its exact image IDs.
# Rebuilding every case adds registry requests without testing different source.
if [[ ! -f "${image_marker}" ]]; then
  docker pull "${sync_image}"
  docker build -f "${REPO_ROOT}/build/worker/Dockerfile" -t "${worker_image}" "${image_context}"
  docker image inspect "${worker_image}" "${sync_image}" --format '{{.Id}}' > "${image_marker}"
fi
expected_images="$(cat "${image_marker}")"
current_images="$(docker image inspect "${worker_image}" "${sync_image}" --format '{{.Id}}')"
[[ "${current_images}" == "${expected_images}" ]] || { echo 'E2E candidate image changed during this run' >&2; exit 1; }
export TALOS_SCENARIO_REPORT_DIR="${SCENARIO_REPORT_DIR}"
export TALOS_WORKER_E2E_IMAGE="${worker_image}" TALOS_SYNC_E2E_IMAGE="${sync_image}"
pnpm --dir "${TALOS_APP_DIR}/desktop" exec vite build --config "${SCRIPT_DIR}/../computer_workspace/vite.config.ts"
python3 "${SCRIPT_DIR}/scenarios.py" "${1}"
