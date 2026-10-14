#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
PROFILE="${PROFILE:-dev}"
PRODUCT_DIR="${PRODUCT_DIR:-${REPO_ROOT}/dist/products/${PROFILE}}"
VERIFY_TARGET="${1:-all}"

require_file() {
  local pattern="$1"
  if ! compgen -G "${pattern}" >/dev/null; then
    echo "Missing required artifact: ${pattern}" >&2
    exit 1
  fi
}

verify_archive_licenses() {
  local archive_path="$1"
  for name in LICENSE NOTICE THIRD_PARTY_NOTICES; do
    if ! tar -tzf "${archive_path}" | grep -E "^(\\./)?${name}$" >/dev/null; then
      echo "Missing ${name} in ${archive_path}" >&2
      exit 1
    fi
  done
}

verify_server_config_template() {
  local archive_path
  archive_path="$(find "${PRODUCT_DIR}/server" -maxdepth 1 -name "Talos-Server-*.tar.gz" | head -n 1)"
  verify_archive_licenses "${archive_path}"
  if ! tar -tzf "${archive_path}" | grep -E '^(\./)?bin/talos-server$' >/dev/null; then
    echo "Missing server binary in ${archive_path}" >&2
    exit 1
  fi
  if ! tar -tzf "${archive_path}" | grep -E '^(\./)?bin/talos-worker$' >/dev/null; then
    echo "Missing worker binary in ${archive_path}" >&2
    exit 1
  fi
  if [[ "$(basename "${archive_path}")" == *linux* ]] \
    && ! tar -tzf "${archive_path}" | grep -E '^(\./)?bin/talos-linux-sandbox-launcher$' >/dev/null; then
    echo "Missing Linux sandbox launcher in ${archive_path}" >&2
    exit 1
  fi
  if ! tar -tzf "${archive_path}" | grep -E '^(\./)?conf/talos.yaml$' >/dev/null; then
    echo "Missing server config template in ${archive_path}" >&2
    exit 1
  fi
  if ! tar -tzf "${archive_path}" | grep -E '^(\./)?conf/migrations/sqlite/.+\.sql$' >/dev/null; then
    echo "Missing server SQLite migrations in ${archive_path}" >&2
    exit 1
  fi
  if ! tar -tzf "${archive_path}" | grep -E '^(\./)?conf/migrations/postgres/README.md$' >/dev/null; then
    echo "Missing server PostgreSQL migration placeholder in ${archive_path}" >&2
    exit 1
  fi
}

verify_tui_checksums() {
  (
    cd "${PRODUCT_DIR}/tui"
    shasum -a 256 -c SHA256SUMS
  )
}

verify_tui() {
  local archive_path
  archive_path="$(find "${PRODUCT_DIR}/tui" -maxdepth 1 -name "Talos-TUI-*.tar.gz" | head -n 1)"
  verify_archive_licenses "${archive_path}"
  if ! tar -tzf "${archive_path}" | grep -E '^bin/talos$' >/dev/null; then
    echo "Missing TUI binary in ${archive_path}" >&2
    exit 1
  fi
  if ! tar -tzf "${archive_path}" | grep -E '^conf/talos.yaml$' >/dev/null; then
    echo "Missing TUI config template in ${archive_path}" >&2
    exit 1
  fi
  if ! tar -tzf "${archive_path}" | grep -E '^conf/migrations/sqlite/.+\.sql$' >/dev/null; then
    echo "Missing TUI SQLite migrations in ${archive_path}" >&2
    exit 1
  fi
  if ! tar -tzf "${archive_path}" | grep -E '^conf/migrations/postgres/README.md$' >/dev/null; then
    echo "Missing TUI PostgreSQL migration placeholder in ${archive_path}" >&2
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
  for license_file in LICENSE NOTICE THIRD_PARTY_NOTICES; do
    if [[ ! -f "${mount_dir}/Talos.app/Contents/Resources/licenses/${license_file}" ]]; then
      echo "Missing packaged ${license_file} in macOS app." >&2
      exit 1
    fi
  done
  local sqlite_migrations="${mount_dir}/Talos.app/Contents/Resources/conf/migrations/sqlite"
  if ! find "${sqlite_migrations}" -maxdepth 1 -name '*.sql' -print -quit | grep -q .; then
    echo "Missing packaged macOS SQLite migrations: ${sqlite_migrations}" >&2
    exit 1
  fi
  local postgres_migrations="${mount_dir}/Talos.app/Contents/Resources/conf/migrations/postgres"
  if [[ ! -f "${postgres_migrations}/README.md" ]]; then
    echo "Missing packaged macOS PostgreSQL migration placeholder: ${postgres_migrations}" >&2
    exit 1
  fi

  hdiutil detach "${mount_dir}" >/dev/null
  rm -rf "${mount_dir}"
  trap - RETURN
}

verify_linux_app_server() {
  if [[ "$(uname -s)" != "Linux" ]]; then
    echo "Skipping Linux AppImage app-server inspection on non-Linux host."
    return
  fi

  local app_image
  app_image="$(find "${PRODUCT_DIR}/desktop-linux" -maxdepth 1 -name 'Talos-*.AppImage' | head -n 1)"
  local extract_dir
  extract_dir="$(mktemp -d "${TMPDIR:-/tmp}/talos-appimage.XXXXXX")"
  (
    cd "${extract_dir}"
    "${app_image}" --appimage-extract >/dev/null
  )
  trap 'rm -rf "${extract_dir}"' RETURN

  local app_server="${extract_dir}/squashfs-root/resources/app-server/app-server"
  local launcher="${extract_dir}/squashfs-root/resources/app-server/talos-linux-sandbox-launcher"
  if [[ ! -x "${app_server}" ]]; then
    echo "Missing packaged Linux app-server binary: ${app_server}" >&2
    exit 1
  fi
  for license_file in LICENSE NOTICE THIRD_PARTY_NOTICES; do
    if [[ ! -f "${extract_dir}/squashfs-root/resources/licenses/${license_file}" ]]; then
      echo "Missing packaged ${license_file} in Linux AppImage." >&2
      exit 1
    fi
  done
  if [[ ! -x "${launcher}" ]]; then
    echo "Missing packaged Linux sandbox launcher: ${launcher}" >&2
    exit 1
  fi

  rm -rf "${extract_dir}"
  trap - RETURN
}

case "${VERIFY_TARGET}" in
  server)
    require_file "${PRODUCT_DIR}/server/Talos-Server-*.tar.gz"
    verify_server_config_template
    ;;
  tui)
    require_file "${PRODUCT_DIR}/tui/Talos-TUI-*.tar.gz"
    require_file "${PRODUCT_DIR}/tui/SHA256SUMS"
    verify_tui
    verify_tui_checksums
    ;;
  desktop-mac)
    require_file "${PRODUCT_DIR}/desktop-mac/Talos-*.dmg"
    verify_macos_app_server
    ;;
  desktop-linux)
    require_file "${PRODUCT_DIR}/desktop-linux/Talos-*.AppImage"
    require_file "${PRODUCT_DIR}/desktop-linux/SHA256SUMS"
    (
      cd "${PRODUCT_DIR}/desktop-linux"
      shasum -a 256 -c SHA256SUMS
    )
    verify_linux_app_server
    ;;
  all)
    require_file "${PRODUCT_DIR}/server/Talos-Server-*.tar.gz"
    require_file "${PRODUCT_DIR}/tui/Talos-TUI-*.tar.gz"
    require_file "${PRODUCT_DIR}/tui/SHA256SUMS"
    require_file "${PRODUCT_DIR}/desktop-mac/Talos-*.dmg"
    require_file "${PRODUCT_DIR}/desktop-linux/Talos-*.AppImage"
    require_file "${PRODUCT_DIR}/desktop-linux/SHA256SUMS"
    (
      cd "${PRODUCT_DIR}/desktop-linux"
      shasum -a 256 -c SHA256SUMS
    )
    verify_server_config_template
    verify_tui
    verify_tui_checksums
    verify_macos_app_server
    verify_linux_app_server

    if [[ "${TALOS_VERIFY_MAC_SIGNATURE:-0}" == "1" ]]; then
      dmg_path="$(find "${PRODUCT_DIR}/desktop-mac" -maxdepth 1 -name "Talos-*.dmg" | head -n 1)"
      spctl -a -t open --context context:primary-signature -v "${dmg_path}"
      xcrun stapler validate "${dmg_path}"
    fi
    ;;
  *)
    echo "Unsupported verification target: ${VERIFY_TARGET}" >&2
    exit 1
    ;;
esac

echo "Release artifacts verified (${VERIFY_TARGET}): ${PRODUCT_DIR}"
