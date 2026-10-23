#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
export TALOS_SCENARIO_REPORT_DIR="${SCENARIO_REPORT_DIR}"
: "${TALOS_E2E_WORKER_IMAGE:?Build the current standard Worker image and set TALOS_E2E_WORKER_IMAGE}"
docker image inspect "${TALOS_E2E_WORKER_IMAGE}" --format '{{.Id}}' > "${SCENARIO_REPORT_DIR}/worker-image.txt"
prepare_binary TALOS_E2E_SERVER_BIN talos-server talos-server
python3 "${TALOS_E2E_DIR}/scripts/output_logs/docker_acceptance.py"
