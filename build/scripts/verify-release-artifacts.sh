#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
PROFILE="${PROFILE:-dev}"
PRODUCT_DIR="${PRODUCT_DIR:-${REPO_ROOT}/dist/products/${PROFILE}}"

require_file() {
  local pattern="$1"
  if ! compgen -G "${pattern}" >/dev/null; then
    echo "Missing required artifact: ${pattern}" >&2
    exit 1
  fi
}

require_file "${PRODUCT_DIR}/server/Talos-Server-*.tar.gz"
require_file "${PRODUCT_DIR}/tui/Talos-TUI-*.tar.gz"
require_file "${PRODUCT_DIR}/desktop-mac/Talos-*.dmg"
require_file "${PRODUCT_DIR}/desktop-linux/Talos-*.AppImage"
require_file "${PRODUCT_DIR}/desktop-linux/SHA256SUMS"

verify_server_config_template() {
  local archive_path
  archive_path="$(find "${PRODUCT_DIR}/server" -maxdepth 1 -name "Talos-Server-*.tar.gz" | head -n 1)"
  if ! tar -tzf "${archive_path}" | grep -Eq '^(\./)?bin/talos-server$'; then
    echo "Missing server binary in ${archive_path}" >&2
    exit 1
  fi
  if ! tar -tzf "${archive_path}" | grep -Eq '^(\./)?conf/talos.yaml$'; then
    echo "Missing server config template in ${archive_path}" >&2
    exit 1
  fi
}

verify_tui() {
  local archive_path
  archive_path="$(find "${PRODUCT_DIR}/tui" -maxdepth 1 -name "Talos-TUI-*.tar.gz" | head -n 1)"
  if ! tar -tzf "${archive_path}" | grep -Eq '^bin/talos$'; then
    echo "Missing TUI binary in ${archive_path}" >&2
    exit 1
  fi
  if ! tar -tzf "${archive_path}" | grep -Eq '^conf/talos.yaml$'; then
    echo "Missing TUI config template in ${archive_path}" >&2
    exit 1
  fi
}

verify_macos_app_server() {
  if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "Skipping macOS DMG app-server inspection on non-Darwin host."
    return
  fi

  local dmg_path
  dmg_path="$(find "${PRODUCT_DIR}/desktop-mac" -maxdepth 1 -name "Talos-*.dmg" | head -n 1)"
  local mount_dir
  mount_dir="$(mktemp -d "${TMPDIR:-/tmp}/talos-dmg.XXXXXX")"
  hdiutil attach -nobrowse -readonly -mountpoint "${mount_dir}" "${dmg_path}" >/dev/null
  trap 'hdiutil detach "${mount_dir}" >/dev/null 2>&1 || true; rm -rf "${mount_dir}"' RETURN

  local app_server="${mount_dir}/Talos.app/Contents/Resources/app-server/app-server"
  if [[ ! -x "${app_server}" ]]; then
    echo "Missing packaged macOS app-server binary: ${app_server}" >&2
    exit 1
  fi
  local config_template="${mount_dir}/Talos.app/Contents/Resources/conf/talos.yaml"
  if [[ ! -f "${config_template}" ]]; then
    echo "Missing packaged macOS config template: ${config_template}" >&2
    exit 1
  fi

  hdiutil detach "${mount_dir}" >/dev/null
  rm -rf "${mount_dir}"
  trap - RETURN
}

verify_linux_app_server() {
  echo "Skipping Linux AppImage app-server inspection."
}

(
  cd "${PRODUCT_DIR}/desktop-linux"
  shasum -a 256 -c SHA256SUMS
)

verify_server_config_template
verify_tui
verify_macos_app_server
verify_linux_app_server

if [[ "${TALOS_VERIFY_MAC_SIGNATURE:-0}" == "1" ]]; then
  dmg_path="$(find "${PRODUCT_DIR}/desktop-mac" -maxdepth 1 -name "Talos-*.dmg" | head -n 1)"
  spctl -a -t open --context context:primary-signature -v "${dmg_path}"
  xcrun stapler validate "${dmg_path}"
fi

echo "Release artifacts verified: ${PRODUCT_DIR}"
