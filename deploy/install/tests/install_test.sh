#!/usr/bin/env bash
set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALLER="${TEST_DIR}/../install.sh"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/talos-install-tests.XXXXXX")"
REAL_CURL="$(command -v curl)"
REAL_MV="$(command -v mv)"

cleanup() {
  rm -rf "${TEST_ROOT}"
}
trap cleanup EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

assert_eq() {
  local actual="$1"
  local expected="$2"
  local context="$3"
  [[ "${actual}" == "${expected}" ]] || fail "${context}: expected '${expected}', got '${actual}'"
}

assert_file() {
  local path="$1"
  [[ -f "${path}" ]] || fail "missing file: ${path}"
}

assert_executable() {
  local path="$1"
  [[ -x "${path}" ]] || fail "missing executable: ${path}"
}

sha256_file() {
  local path="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${path}" | awk '{print $1}'
  else
    shasum -a 256 "${path}" | awk '{print $1}'
  fi
}

write_mock_uname() {
  local bin_dir="$1"
  mkdir -p "${bin_dir}"
  cat >"${bin_dir}/uname" <<'EOF'
#!/bin/sh
case "${1:-}" in
  -s) printf '%s\n' "${MOCK_UNAME_S:?}" ;;
  -m) printf '%s\n' "${MOCK_UNAME_M:?}" ;;
  *) exit 2 ;;
esac
EOF
  chmod 755 "${bin_dir}/uname"
}

write_mock_release_curl() {
  local bin_dir="$1"
  cat >"${bin_dir}/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

latest_url="https://fixtures.invalid/releases/latest"
for argument in "$@"; do
  case "${argument}" in
    "${latest_url}"|https://github.com/apilots/talos-releases/releases/latest)
      printf '%s' "https://fixtures.invalid/releases/tag/v4.0.0"
      exit 0
      ;;
    https://github.com/apilots/talos/releases/latest) exit 22 ;;
  esac
done

mapped_args=()
for argument in "$@"; do
  case "${argument}" in
    =https) argument="=https,file" ;;
    https://github.com/apilots/talos/releases/download/*) exit 22 ;;
    https://github.com/apilots/talos-releases/releases/download/*)
      argument="file://${MOCK_RELEASE_ROOT}/${argument#https://github.com/apilots/talos-releases/releases/}"
      ;;
    https://fixtures.invalid/releases/download/*)
      argument="file://${MOCK_RELEASE_ROOT}/${argument#https://fixtures.invalid/releases/}"
      ;;
  esac
  mapped_args+=("${argument}")
done
exec "${REAL_CURL}" "${mapped_args[@]}"
EOF
  chmod 755 "${bin_dir}/curl"
}

write_signal_curl() {
  local bin_dir="$1"
  cat >"${bin_dir}/curl" <<'EOF'
#!/bin/sh
kill -TERM "${PPID}"
exit 0
EOF
  chmod 755 "${bin_dir}/curl"
}

write_failing_current_mv() {
  local bin_dir="$1"
  cat >"${bin_dir}/mv" <<'EOF'
#!/bin/sh
destination=""
for argument in "$@"; do
  destination="${argument}"
done
case "${destination}" in
  */current) exit 73 ;;
esac
exec "${REAL_MV:?}" "$@"
EOF
  chmod 755 "${bin_dir}/mv"
}

write_failing_cleanup_rm() {
  local bin_dir="$1"
  cat >"${bin_dir}/rm" <<'EOF'
#!/bin/sh
exit 74
EOF
  chmod 755 "${bin_dir}/rm"
}

create_release_bundle() {
  local release_root="$1"
  local version="$2"
  local platform="$3"
  local arch="$4"
  local checksum_mode="${5:-valid}"
  local release_dir="${release_root}/download/v${version}"
  local staging_dir="${TEST_ROOT}/bundle-${version}-${platform}-${arch}"
  local asset="Talos-TUI-${version}-${platform}-${arch}.tar.gz"

  mkdir -p "${release_dir}" "${staging_dir}/bin" "${staging_dir}/conf/migrations/sqlite" \
    "${staging_dir}/conf/migrations/postgres"
  cat >"${staging_dir}/bin/talos" <<EOF
#!/bin/sh
set -eu
test -f "\${TALOS_RESOURCE_DIR:?}/conf/talos.yaml"
test -f "\${TALOS_RESOURCE_DIR}/conf/migrations/sqlite/001_init.sql"
case "\${TALOS_RESOURCE_DIR}" in
  */current) exit 9 ;;
esac
printf 'talos ${version} ${platform}-${arch}\n'
EOF
  chmod 755 "${staging_dir}/bin/talos"
  printf 'gateway:\n  auth: false\n' >"${staging_dir}/conf/talos.yaml"
  printf 'SELECT 1;\n' >"${staging_dir}/conf/migrations/sqlite/001_init.sql"
  printf 'Reserved\n' >"${staging_dir}/conf/migrations/postgres/README.md"
  tar -czf "${release_dir}/${asset}" -C "${staging_dir}" bin conf

  local checksum
  checksum="$(sha256_file "${release_dir}/${asset}")"
  if [[ "${checksum_mode}" == "invalid" ]]; then
    checksum="$(printf '0%.0s' {1..64})"
  fi
  printf '%s  ./%s\n' "${checksum}" "${asset}" >"${release_dir}/checksums.txt"
}

run_installer() {
  local case_root="$1"
  local release_root="$2"
  local os="$3"
  local arch="$4"
  local version="$5"
  shift 5

  local mock_bin="${case_root}/mock-bin"
  write_mock_uname "${mock_bin}"
  HOME="${case_root}/home" \
    PATH="${mock_bin}:${PATH}" \
    MOCK_UNAME_S="${os}" \
    MOCK_UNAME_M="${arch}" \
    TALOS_INSTALL_ROOT="${case_root}/install" \
    TALOS_BIN_DIR="${case_root}/bin" \
    TALOS_RELEASE_BASE_URL="file://${release_root}" \
    "${INSTALLER}" --version "${version}" "$@"
}

test_installs_linux_amd64_bundle() {
  local case_root="${TEST_ROOT}/linux-amd64"
  local release_root="${case_root}/releases"
  mkdir -p "${case_root}/home/.talos"
  printf 'keep\n' >"${case_root}/home/.talos/sentinel"
  create_release_bundle "${release_root}" "1.2.3" "linux" "amd64"
  create_release_bundle "${release_root}" "1.2.4" "linux" "amd64"

  run_installer "${case_root}" "${release_root}" "Linux" "x86_64" "1.2.3"
  local repeat_output
  repeat_output="$(run_installer "${case_root}" "${release_root}" "Linux" "x86_64" "1.2.3")"
  grep -Fq "Talos TUI 1.2.3 is already installed at" <<<"${repeat_output}" \
    || fail "repeat install did not report the existing version"
  if grep -Fq "Downloading Talos TUI" <<<"${repeat_output}"; then
    fail "repeat install unexpectedly downloaded an immutable version"
  fi

  local version_dir="${case_root}/install/versions/1.2.3-linux-amd64"
  assert_executable "${version_dir}/bin/talos"
  assert_file "${version_dir}/conf/talos.yaml"
  assert_file "${version_dir}/.talos-release-checksum"
  assert_file "${version_dir}/.talos-bundle-manifest"
  assert_file "${case_root}/home/.talos/sentinel"
  [[ ! -e "${case_root}/install/.install-lock" ]] || fail "successful install left its lock"
  assert_eq "$(readlink "${case_root}/install/current")" \
    "versions/1.2.3-linux-amd64" "active Linux version"
  assert_eq "$("${case_root}/bin/talos")" \
    "talos 1.2.3 linux-amd64" "installed Linux launcher"

  run_installer "${case_root}" "${release_root}" "Linux" "x86_64" "1.2.4"
  assert_eq "$(readlink "${case_root}/install/current")" \
    "versions/1.2.4-linux-amd64" "upgraded Linux version"
  assert_eq "$("${case_root}/bin/talos")" \
    "talos 1.2.4 linux-amd64" "upgraded Linux launcher"

  printf 'modified\n' >>"${case_root}/install/versions/1.2.4-linux-amd64/conf/talos.yaml"
  if run_installer "${case_root}" "${release_root}" "Linux" "x86_64" "1.2.4"; then
    fail "modified installed bundle should fail integrity validation"
  fi
  assert_eq "$(readlink "${case_root}/install/current")" \
    "versions/1.2.4-linux-amd64" "active version after integrity failure"
}

test_maps_remaining_target_aliases() {
  local linux_root="${TEST_ROOT}/linux-arm64"
  local linux_releases="${linux_root}/releases"
  create_release_bundle "${linux_releases}" "3.0.0" "linux" "arm64"
  run_installer "${linux_root}" "${linux_releases}" "Linux" "aarch64" "3.0.0"
  assert_eq "$("${linux_root}/bin/talos")" \
    "talos 3.0.0 linux-arm64" "installed Linux ARM64 launcher"

  local macos_root="${TEST_ROOT}/macos-amd64"
  local macos_releases="${macos_root}/releases"
  create_release_bundle "${macos_releases}" "3.0.0" "macos" "amd64"
  run_installer "${macos_root}" "${macos_releases}" "Darwin" "x86_64" "3.0.0"
  assert_eq "$("${macos_root}/bin/talos")" \
    "talos 3.0.0 macos-amd64" "installed macOS AMD64 launcher"
}

test_installs_macos_arm64_bundle() {
  local case_root="${TEST_ROOT}/macos-arm64"
  local release_root="${case_root}/releases"
  create_release_bundle "${release_root}" "2.0.0" "macos" "arm64"

  run_installer "${case_root}" "${release_root}" "Darwin" "arm64" "v2.0.0"

  assert_eq "$(readlink "${case_root}/install/current")" \
    "versions/2.0.0-macos-arm64" "active macOS version"
  assert_eq "$("${case_root}/bin/talos")" \
    "talos 2.0.0 macos-arm64" "installed macOS launcher"
}

test_checksum_failure_preserves_current_version() {
  local case_root="${TEST_ROOT}/checksum"
  local release_root="${case_root}/releases"
  create_release_bundle "${release_root}" "1.0.0" "linux" "amd64"
  create_release_bundle "${release_root}" "1.1.0" "linux" "amd64" "invalid"
  run_installer "${case_root}" "${release_root}" "Linux" "amd64" "1.0.0"

  if run_installer "${case_root}" "${release_root}" "Linux" "amd64" "1.1.0"; then
    fail "checksum mismatch should fail"
  fi

  assert_eq "$(readlink "${case_root}/install/current")" \
    "versions/1.0.0-linux-amd64" "current version after checksum failure"
  assert_eq "$("${case_root}/bin/talos")" \
    "talos 1.0.0 linux-amd64" "launcher after checksum failure"
}

test_installs_latest_release_by_default() {
  local case_root="${TEST_ROOT}/latest"
  local release_root="${case_root}/releases"
  local mock_bin="${case_root}/mock-bin"
  create_release_bundle "${release_root}" "4.0.0" "linux" "amd64"
  write_mock_uname "${mock_bin}"
  write_mock_release_curl "${mock_bin}"

  HOME="${case_root}/home" \
    PATH="${mock_bin}:${PATH}" \
    MOCK_UNAME_S="Linux" \
    MOCK_UNAME_M="x86_64" \
    MOCK_RELEASE_ROOT="${release_root}" \
    REAL_CURL="${REAL_CURL}" \
    TALOS_INSTALL_ROOT="${case_root}/install" \
    TALOS_BIN_DIR="${case_root}/bin" \
    TALOS_RELEASE_BASE_URL="https://fixtures.invalid/releases" \
    "${INSTALLER}"

  assert_eq "$("${case_root}/bin/talos")" \
    "talos 4.0.0 linux-amd64" "latest release launcher"
}

test_falls_back_to_archived_releases_before_and_after_migration() {
  local case_root="${TEST_ROOT}/legacy-fallback"
  local release_root="${case_root}/releases"
  local mock_bin="${case_root}/mock-bin"
  create_release_bundle "${release_root}" "4.0.0" "linux" "amd64"
  write_mock_uname "${mock_bin}"
  write_mock_release_curl "${mock_bin}"

  HOME="${case_root}/home-latest" \
    PATH="${mock_bin}:${PATH}" \
    MOCK_UNAME_S="Linux" \
    MOCK_UNAME_M="x86_64" \
    MOCK_RELEASE_ROOT="${release_root}" \
    REAL_CURL="${REAL_CURL}" \
    TALOS_INSTALL_ROOT="${case_root}/install-latest" \
    TALOS_BIN_DIR="${case_root}/bin-latest" \
    "${INSTALLER}"
  assert_eq "$("${case_root}/bin-latest/talos")" \
    "talos 4.0.0 linux-amd64" "archived latest release fallback"

  HOME="${case_root}/home-version" \
    PATH="${mock_bin}:${PATH}" \
    MOCK_UNAME_S="Linux" \
    MOCK_UNAME_M="x86_64" \
    MOCK_RELEASE_ROOT="${release_root}" \
    REAL_CURL="${REAL_CURL}" \
    TALOS_INSTALL_ROOT="${case_root}/install-version" \
    TALOS_BIN_DIR="${case_root}/bin-version" \
    "${INSTALLER}" --version "4.0.0"
  assert_eq "$("${case_root}/bin-version/talos")" \
    "talos 4.0.0 linux-amd64" "archived explicit version fallback"
}

test_rejects_unsupported_platform() {
  local case_root="${TEST_ROOT}/unsupported"
  local release_root="${case_root}/releases"
  mkdir -p "${release_root}"

  if run_installer "${case_root}" "${release_root}" "FreeBSD" "amd64" "1.0.0"; then
    fail "unsupported platform should fail"
  fi
  [[ ! -e "${case_root}/install/current" ]] || fail "unsupported platform changed current"
}

test_rejects_active_install_lock() {
  local case_root="${TEST_ROOT}/active-lock"
  local release_root="${case_root}/releases"
  create_release_bundle "${release_root}" "5.0.0" "linux" "amd64"
  mkdir -p "${case_root}/install/.install-lock"

  if run_installer "${case_root}" "${release_root}" "Linux" "amd64" "5.0.0"; then
    fail "active installer lock should reject a concurrent installation"
  fi
  [[ ! -e "${case_root}/install/current" ]] || fail "locked installation changed current"
}

test_rejects_relative_install_paths() {
  local case_root="${TEST_ROOT}/relative-paths"
  local mock_bin="${case_root}/mock-bin"
  mkdir -p "${case_root}/home" "${case_root}/releases"
  write_mock_uname "${mock_bin}"

  if (
    cd "${case_root}"
    HOME="${case_root}/home" \
      PATH="${mock_bin}:${PATH}" \
      MOCK_UNAME_S="Linux" \
      MOCK_UNAME_M="amd64" \
      TALOS_INSTALL_ROOT="install" \
      TALOS_BIN_DIR="bin" \
      TALOS_RELEASE_BASE_URL="file://${case_root}/releases" \
      "${INSTALLER}" --version "5.0.0"
  ); then
    fail "relative installation paths should fail"
  fi
  [[ ! -e "${case_root}/install" ]] || fail "relative path validation created an install root"
}

test_cleanup_failure_still_releases_lock() {
  local case_root="${TEST_ROOT}/cleanup-failure"
  local mock_bin="${case_root}/mock-bin"
  mkdir -p "${case_root}/home" "${case_root}/releases" "${case_root}/tmp"
  write_mock_uname "${mock_bin}"
  write_failing_cleanup_rm "${mock_bin}"

  if HOME="${case_root}/home" \
    PATH="${mock_bin}:${PATH}" \
    TMPDIR="${case_root}/tmp" \
    MOCK_UNAME_S="Linux" \
    MOCK_UNAME_M="amd64" \
    TALOS_INSTALL_ROOT="${case_root}/install" \
    TALOS_BIN_DIR="${case_root}/bin" \
    TALOS_RELEASE_BASE_URL="file://${case_root}/releases" \
    "${INSTALLER}" --version "5.0.0" >/dev/null 2>&1; then
    fail "missing release fixture should fail"
  fi
  [[ ! -e "${case_root}/install/.install-lock" ]] \
    || fail "cleanup failure prevented lock release"
}

test_signal_exits_without_installing() {
  local case_root="${TEST_ROOT}/signal"
  local mock_bin="${case_root}/mock-bin"
  mkdir -p "${case_root}/home"
  write_mock_uname "${mock_bin}"
  write_signal_curl "${mock_bin}"

  set +e
  HOME="${case_root}/home" \
    PATH="${mock_bin}:${PATH}" \
    MOCK_UNAME_S="Linux" \
    MOCK_UNAME_M="amd64" \
    TALOS_INSTALL_ROOT="${case_root}/install" \
    TALOS_BIN_DIR="${case_root}/bin" \
    TALOS_RELEASE_BASE_URL="file://${case_root}/releases" \
    "${INSTALLER}" --version "5.0.0" >/dev/null 2>&1
  local status=$?
  set -e

  assert_eq "${status}" "143" "TERM exit status"
  [[ ! -e "${case_root}/bin/talos" ]] || fail "signalled installation exposed a command"
  [[ ! -e "${case_root}/install/.install-lock" ]] || fail "signalled installation left its lock"
}

test_activation_failure_does_not_expose_command() {
  local case_root="${TEST_ROOT}/activation-failure"
  local release_root="${case_root}/releases"
  local mock_bin="${case_root}/mock-bin"
  create_release_bundle "${release_root}" "6.0.0" "linux" "amd64"
  write_mock_uname "${mock_bin}"
  write_failing_current_mv "${mock_bin}"

  if HOME="${case_root}/home" \
    PATH="${mock_bin}:${PATH}" \
    MOCK_UNAME_S="Linux" \
    MOCK_UNAME_M="amd64" \
    REAL_MV="${REAL_MV}" \
    TALOS_INSTALL_ROOT="${case_root}/install" \
    TALOS_BIN_DIR="${case_root}/bin" \
    TALOS_RELEASE_BASE_URL="file://${release_root}" \
    "${INSTALLER}" --version "6.0.0"; then
    fail "current activation failure should fail installation"
  fi
  [[ ! -e "${case_root}/bin/talos" ]] || fail "failed activation exposed a command"
  [[ ! -e "${case_root}/install/.install-lock" ]] || fail "failed activation left its lock"
}

test_installs_linux_amd64_bundle
test_installs_macos_arm64_bundle
test_maps_remaining_target_aliases
test_checksum_failure_preserves_current_version
test_installs_latest_release_by_default
test_falls_back_to_archived_releases_before_and_after_migration
test_rejects_unsupported_platform
test_rejects_active_install_lock
test_rejects_relative_install_paths
test_cleanup_failure_still_releases_lock
test_signal_exits_without_installing
test_activation_failure_does_not_expose_command

echo "Talos installer tests passed."
