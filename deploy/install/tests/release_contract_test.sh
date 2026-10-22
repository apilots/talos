#!/usr/bin/env bash
set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${TEST_DIR}/../../.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/talos-release-contract.XXXXXX")"

cleanup() {
  rm -rf "${TEST_ROOT}"
}
trap cleanup EXIT

if PRODUCT_DIR="${TEST_ROOT}/products" \
  TUI_PACKAGE_VERSION="0.0.0-version-mismatch" \
  "${REPO_ROOT}/build.sh" tui >"${TEST_ROOT}/build.log" 2>&1; then
  echo "Mismatched TUI package and binary versions should fail" >&2
  exit 1
fi

grep -Fq "does not match Talos binary version" "${TEST_ROOT}/build.log" || {
  cat "${TEST_ROOT}/build.log" >&2
  echo "Version mismatch did not return the expected error" >&2
  exit 1
}

assets_dir="${TEST_ROOT}/release-assets"
bundle_dir="${TEST_ROOT}/bundle"
mkdir -p "${assets_dir}" "${bundle_dir}/bin" \
  "${bundle_dir}/conf/migrations/sqlite" "${bundle_dir}/conf/migrations/postgres"
cat >"${bundle_dir}/bin/talos" <<'EOF'
#!/bin/sh
test -f "${TALOS_RESOURCE_DIR:?}/conf/talos.yaml" || exit 1
printf 'talos 1.2.3\n'
EOF
chmod 755 "${bundle_dir}/bin/talos"
printf 'gateway:\n  auth: false\n' >"${bundle_dir}/conf/talos.yaml"
printf 'SELECT 1;\n' >"${bundle_dir}/conf/migrations/sqlite/001_init.sql"
printf 'Reserved\n' >"${bundle_dir}/conf/migrations/postgres/README.md"
for target in linux-amd64 linux-arm64 macos-arm64; do
  tar -czf "${assets_dir}/Talos-TUI-1.2.3-${target}.tar.gz" -C "${bundle_dir}" bin conf
done
(
  cd "${assets_dir}"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum ./*.tar.gz >checksums.txt
  else
    shasum -a 256 ./*.tar.gz >checksums.txt
  fi
)
cat >"${assets_dir}/install.sh" <<'EOF'
#!/bin/sh
set -eu
case "${1:-}" in
  --help)
    if [ "${RELEASE_TEST_MODE}" = legacy ]; then
      printf 'Usage: install.sh [--version <version>]\n'
      exit 0
    fi
    ;;
  --package)
    printf 'local attempt\n' >>"${RELEASE_TEST_LOCAL_CALLS}"
    if [ "${RELEASE_TEST_MODE}" != current ]; then
      printf 'simulated unsupported or broken local installation\n' >&2
      exit 73
    fi
    ;;
esac
exec "${RELEASE_TEST_INSTALLER}" "$@"
EOF
chmod 755 "${assets_dir}/install.sh"

for mode in legacy current broken; do
  output="${TEST_ROOT}/${mode}.log"
  local_calls="${TEST_ROOT}/${mode}-local-calls"
  if RELEASE_TEST_MODE="${mode}" \
    RELEASE_TEST_INSTALLER="${REPO_ROOT}/deploy/install/install.sh" \
    RELEASE_TEST_LOCAL_CALLS="${local_calls}" \
    "${TEST_DIR}/release_assets_test.sh" "${assets_dir}" 1.2.3 >"${output}" 2>&1; then
    [[ "${mode}" != broken ]] || {
      echo "An advertised but failing local installer must fail release validation" >&2
      exit 1
    }
  else
    [[ "${mode}" == broken ]] || { cat "${output}" >&2; exit 1; }
    grep -Fq 'simulated unsupported or broken local installation' "${output}"
  fi
  if [[ "${mode}" == legacy ]]; then
    [[ ! -e "${local_calls}" ]] || {
      echo "Current validation must not require --package from older tagged installers" >&2
      exit 1
    }
  elif [[ "${mode}" == current ]]; then
    [[ "$(wc -l <"${local_calls}" | tr -d ' ')" == 2 ]] || {
      echo "Current installers must complete both local installation attempts" >&2
      exit 1
    }
  fi
done

echo "Talos release contract tests passed."
