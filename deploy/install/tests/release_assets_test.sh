#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -ne 2 ]]; then
  echo "Usage: release_assets_test.sh <release-assets-dir> <version>" >&2
  exit 2
fi

ASSETS_DIR="$(cd "$1" && pwd)"
VERSION="$2"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/talos-release-assets.XXXXXX")"

cleanup() {
  rm -rf "${TEST_ROOT}"
}
trap cleanup EXIT

sha256_file() {
  local path="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${path}" | awk '{print $1}'
  else
    shasum -a 256 "${path}" | awk '{print $1}'
  fi
}

expected_checksum() {
  local asset_name="$1"
  awk -v target="${asset_name}" '
    {
      path = $2
      sub(/^\.\//, "", path)
      if (path == target) {
        print tolower($1)
        exit
      }
    }
  ' "${ASSETS_DIR}/checksums.txt"
}

[[ -x "${ASSETS_DIR}/install.sh" ]] || {
  echo "Final release assets are missing executable install.sh" >&2
  exit 1
}
[[ -f "${ASSETS_DIR}/checksums.txt" ]] || {
  echo "Final release assets are missing checksums.txt" >&2
  exit 1
}

for target in linux-amd64 linux-arm64 macos-amd64 macos-arm64; do
  asset_name="Talos-TUI-${VERSION}-${target}.tar.gz"
  asset_path="${ASSETS_DIR}/${asset_name}"
  [[ -f "${asset_path}" ]] || {
    echo "Final release assets are missing ${asset_name}" >&2
    exit 1
  }
  checksum="$(expected_checksum "${asset_name}")"
  [[ -n "${checksum}" ]] || {
    echo "Final checksums do not include ${asset_name}" >&2
    exit 1
  }
  [[ "$(sha256_file "${asset_path}")" == "${checksum}" ]] || {
    echo "Final checksum does not match ${asset_name}" >&2
    exit 1
  }
done

case "$(uname -s)" in
  Linux) platform="linux" ;;
  Darwin) platform="macos" ;;
  *) echo "Unsupported test platform: $(uname -s)" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  x86_64|amd64) arch="amd64" ;;
  arm64|aarch64) arch="arm64" ;;
  *) echo "Unsupported test architecture: $(uname -m)" >&2; exit 1 ;;
esac

asset_name="Talos-TUI-${VERSION}-${platform}-${arch}.tar.gz"
release_dir="${TEST_ROOT}/releases/download/v${VERSION}"
mkdir -p "${release_dir}" "${TEST_ROOT}/home"
cp "${ASSETS_DIR}/${asset_name}" "${release_dir}/${asset_name}"
cp "${ASSETS_DIR}/checksums.txt" "${release_dir}/checksums.txt"

install_output="$(
  HOME="${TEST_ROOT}/home" \
    TALOS_INSTALL_ROOT="${TEST_ROOT}/install" \
    TALOS_BIN_DIR="${TEST_ROOT}/bin" \
    TALOS_RELEASE_BASE_URL="file://${TEST_ROOT}/releases" \
    "${ASSETS_DIR}/install.sh" --version "${VERSION}"
)"
printf '%s\n' "${install_output}"

version_output="$(HOME="${TEST_ROOT}/home" "${TEST_ROOT}/bin/talos" --version)"
[[ "${version_output}" == "talos ${VERSION}" ]] || {
  echo "Installed release asset returned unexpected version: ${version_output}" >&2
  exit 1
}

echo "Final release assets installation passed for ${platform}-${arch}."
