#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="${SCRIPT_DIR}"
RUST_DIR="${REPO_ROOT}/talos-rs"
BUILD_ASSET_DIR="${REPO_ROOT}/build"
TARGET="${1:-all}"
PROFILE="${PROFILE:-dev}"
PRODUCT_DIR="${PRODUCT_DIR:-${REPO_ROOT}/dist/products/${PROFILE}}"

usage() {
  cat <<'EOF'
Usage: ./build.sh [all|server|server-webui|tui|webui|desktop|desktop-mac|desktop-linux]

Targets:
  all      Build deployable server+WebUI, TUI, and packaged Desktop products.
  server   Build the Talos server binary.
  server-webui  Build a deployable server+WebUI tarball.
  tui      Build the terminal UI binary.
  webui    Build the browser WebUI bundle.
  desktop      Build the unpackaged Electron Desktop shell for local debug.
  desktop-mac    Build the macOS Desktop DMG package.
  desktop-linux  Build the Linux Desktop AppImage package.

Environment:
  PROFILE=release  Build Rust targets in release mode.
  PRODUCT_DIR=path  Override unified product output directory.
EOF
}

require_rust() {
  if command -v rustc >/dev/null 2>&1; then
    rustc --version
    return
  fi
  echo "Rust is not installed. Installing Rust..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  # shellcheck disable=SC1090
  source "${HOME}/.cargo/env"
}

require_pnpm() {
  if command -v pnpm >/dev/null 2>&1; then
    pnpm --version
    return
  fi
  echo "pnpm is required to build WebUI/Desktop." >&2
  exit 1
}

cargo_profile_args() {
  if [[ "${PROFILE}" == "release" ]]; then
    echo "--release"
  fi
}

rust_target_dir() {
  if [[ "${PROFILE}" == "release" ]]; then
    echo "${RUST_DIR}/target/release"
  else
    echo "${RUST_DIR}/target/debug"
  fi
}

workspace_version() {
  awk '
    /^\[workspace.package\]/ { in_workspace_package = 1; next }
    /^\[/ { in_workspace_package = 0 }
    in_workspace_package && /^version = / {
      gsub(/"/, "", $3)
      print $3
      exit
    }
  ' "${RUST_DIR}/Cargo.toml"
}

package_arch() {
  case "$(uname -m)" in
    x86_64|amd64)
      echo "amd64"
      ;;
    aarch64|arm64)
      echo "arm64"
      ;;
    *)
      uname -m
      ;;
  esac
}

package_platform() {
  case "$(uname -s)" in
    Darwin)
      echo "macos"
      ;;
    Linux)
      echo "linux"
      ;;
    *)
      uname -s | tr '[:upper:]' '[:lower:]'
      ;;
  esac
}

desktop_version() {
  node -e "console.log(require('${REPO_ROOT}/talos-app/desktop/package.json').version)"
}

reset_product_dir() {
  rm -rf "${PRODUCT_DIR}"
  mkdir -p "${PRODUCT_DIR}"
}

copy_rust_binary_as() {
  local binary_name="$1"
  local output_path="$2"
  mkdir -p "$(dirname "${output_path}")"
  cp "$(rust_target_dir)/${binary_name}" "${output_path}"
  chmod 755 "${output_path}"
}

copy_conf_resource() {
  local output_dir="$1"
  mkdir -p "${output_dir}/conf"
  cp "${RUST_DIR}/conf/talos.yaml" "${output_dir}/conf/talos.yaml"
}

cleanup_desktop_package_dir() {
  local package_dir="$1"
  find "${package_dir}" -mindepth 1 -maxdepth 1 -type d \
    \( -name "mac-*" -o -name "linux-unpacked" -o -name "linux-*-unpacked" \) \
    -exec rm -rf {} +
  rm -f "${package_dir}"/builder-*.yml
}

write_sha256sums() {
  local package_dir="$1"
  local checksum_file="${package_dir}/SHA256SUMS"
  rm -f "${checksum_file}"
  (
    cd "${package_dir}"
    for artifact in *.tar.gz *.dmg *.AppImage *.blockmap; do
      [[ -e "${artifact}" ]] || continue
      shasum -a 256 "${artifact}"
    done
  ) >"${checksum_file}"
}

create_tui_tarball() {
  local package_dir="$1"
  local version="${TUI_PACKAGE_VERSION:-$(workspace_version)}"
  local archive_path="${package_dir}/Talos-TUI-${version}-$(package_platform)-$(package_arch).tar.gz"
  tar -czf "${archive_path}" -C "${package_dir}" bin libexec conf
}

build_server() {
  echo "Building Talos server..."
  cargo build --manifest-path "${RUST_DIR}/Cargo.toml" -p talos-server --bin talos-server $(cargo_profile_args)
  copy_rust_binary_as "talos-server" "${PRODUCT_DIR}/server/talos-server"
  copy_conf_resource "${PRODUCT_DIR}/server"
}

build_server_webui() {
  echo "Building deployable Talos server+WebUI package..."
  require_pnpm
  cargo build --manifest-path "${RUST_DIR}/Cargo.toml" -p talos-server --bin talos-server $(cargo_profile_args)
  pnpm --dir "${REPO_ROOT}/talos-app" --filter talos-web build

  local version="${SERVER_PACKAGE_VERSION:-$(workspace_version)}"
  local archive_dir="${PRODUCT_DIR}/server"
  local staging_dir="${PRODUCT_DIR}/.staging/talos-server"
  local archive_path="${archive_dir}/Talos-Server-${version}-$(package_platform)-$(package_arch).tar.gz"
  rm -rf "${staging_dir}" "${archive_dir}"
  mkdir -p "${staging_dir}/bin" "${staging_dir}/webui" "${archive_dir}"

  if [[ -d "${BUILD_ASSET_DIR}/server" ]]; then
    cp -R "${BUILD_ASSET_DIR}/server/." "${staging_dir}/"
  fi
  copy_rust_binary_as "talos-server" "${staging_dir}/bin/talos-server"
  copy_conf_resource "${staging_dir}"
  cp -R "${REPO_ROOT}/talos-app/web/dist/." "${staging_dir}/webui/"
  tar -czf "${archive_path}" -C "${staging_dir}" .
  rm -rf "${PRODUCT_DIR}/.staging"
}

build_tui() {
  echo "Building Talos TUI..."
  cargo build --manifest-path "${RUST_DIR}/Cargo.toml" -p app-server --bin app-server $(cargo_profile_args)
  cargo build --manifest-path "${RUST_DIR}/Cargo.toml" -p talos-ui-tui --bin talos $(cargo_profile_args)
  rm -rf "${PRODUCT_DIR}/tui"
  mkdir -p "${PRODUCT_DIR}/tui"
  copy_rust_binary_as "talos" "${PRODUCT_DIR}/tui/bin/talos"
  copy_rust_binary_as "app-server" "${PRODUCT_DIR}/tui/libexec/app-server"
  copy_conf_resource "${PRODUCT_DIR}/tui"
  create_tui_tarball "${PRODUCT_DIR}/tui"
  write_sha256sums "${PRODUCT_DIR}/tui"
}

build_webui() {
  echo "Building Talos WebUI..."
  require_pnpm
  pnpm --dir "${REPO_ROOT}/talos-app" --filter talos-web build
  rm -rf "${PRODUCT_DIR}/webui"
  mkdir -p "${PRODUCT_DIR}/webui"
  cp -R "${REPO_ROOT}/talos-app/web/dist/." "${PRODUCT_DIR}/webui/"
}

build_desktop() {
  echo "Building Talos Desktop..."
  require_pnpm
  pnpm --dir "${REPO_ROOT}/talos-app" --filter talos-desktop build
  rm -rf "${PRODUCT_DIR}/desktop"
  mkdir -p "${PRODUCT_DIR}/desktop"
  cp -R "${REPO_ROOT}/talos-app/desktop/dist/." "${PRODUCT_DIR}/desktop/"
  copy_conf_resource "${PRODUCT_DIR}/desktop"
  mkdir -p "${PRODUCT_DIR}/desktop/web"
  cp -R "${REPO_ROOT}/talos-app/web/dist/." "${PRODUCT_DIR}/desktop/web/"
}

build_desktop_dmg() {
  echo "Building Talos Desktop DMG..."
  require_pnpm
  TALOS_DESKTOP_PACKAGE_TARGET="desktop-mac" \
  TALOS_DESKTOP_PACKAGE_DIR="${PRODUCT_DIR}/desktop-mac" \
    pnpm --dir "${REPO_ROOT}/talos-app" --filter talos-desktop package:mac
  cleanup_desktop_package_dir "${PRODUCT_DIR}/desktop-mac"
}

build_desktop_linux() {
  echo "Building Talos Desktop Linux AppImage..."
  require_pnpm
  TALOS_DESKTOP_PACKAGE_TARGET="desktop-linux" \
  TALOS_PACKAGE_ARCH="$(package_arch)" \
  TALOS_DESKTOP_PACKAGE_DIR="${PRODUCT_DIR}/desktop-linux" \
    pnpm --dir "${REPO_ROOT}/talos-app" --filter talos-desktop package:linux
  cleanup_desktop_package_dir "${PRODUCT_DIR}/desktop-linux"
  write_sha256sums "${PRODUCT_DIR}/desktop-linux"
}

require_rust
reset_product_dir

case "${TARGET}" in
  all)
    build_server_webui
    build_tui
    build_desktop_dmg
    build_desktop_linux
    ;;
  server)
    build_server
    ;;
  server-webui)
    build_server_webui
    ;;
  tui)
    build_tui
    ;;
  webui)
    build_webui
    ;;
  desktop)
    build_desktop
    ;;
  desktop-mac)
    build_desktop_dmg
    ;;
  desktop-linux)
    build_desktop_linux
    ;;
  -h|--help|help)
    usage
    ;;
  *)
    usage >&2
    exit 1
    ;;
esac

echo "Build target '${TARGET}' completed."
echo "Products written to: ${PRODUCT_DIR}"
