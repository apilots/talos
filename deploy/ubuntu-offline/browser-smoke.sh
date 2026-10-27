#!/usr/bin/env bash
set -euo pipefail
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(git -C "${script_dir}" rev-parse --show-toplevel)"
export TALOS_E2E_SERVER_IMAGE="${1:?Usage: browser-smoke.sh image:tag}"
exec bash "${repo_root}/talos-e2e/run_e2e.sh" --case 20_browser_workspace/06_docker_server
