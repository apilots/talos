#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

require_command pnpm
require_command cargo

if ! has_real_llm_key; then
  echo "CASE desktop_user_action_real_llm_precheck FAIL missing provider key in .env"
  exit 1
fi
echo "CASE desktop_user_action_real_llm_precheck PASS key_detected=true"

(
  cd "${TALOS_APP_DIR}"
  TALOS_DESKTOP_SMOKE_INTERACTION=1 pnpm --filter talos-desktop smoke
)
