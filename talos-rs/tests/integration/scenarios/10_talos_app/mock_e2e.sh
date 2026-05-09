#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
APP_DIR="${ROOT_DIR}/../talos-app"

if ! command -v pnpm >/dev/null 2>&1; then
  echo "Missing required command: pnpm" >&2
  exit 1
fi

if [[ ! -d "${APP_DIR}" ]]; then
  echo "Talos app directory not found: ${APP_DIR}" >&2
  exit 1
fi

(
  cd "${APP_DIR}"
  pnpm test
)
