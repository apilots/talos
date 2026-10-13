#!/usr/bin/env bash
set -euo pipefail

TUI_INSTALL_E2E_VERSION="${TUI_INSTALL_E2E_VERSION:-$(awk '
  /^\[workspace.package\]/ { in_workspace_package = 1; next }
  /^\[/ { in_workspace_package = 0 }
  in_workspace_package && /^version = / {
    gsub(/"/, "", $3)
    print $3
    exit
  }
' "${REPO_ROOT}/talos-rs/Cargo.toml")}"
TUI_INSTALL_DOCKER_IMAGE="talos-tui-install-e2e:ubuntu-24.04"

normalize_tui_install_platform() {
  case "$1" in
    Linux) printf 'linux\n' ;;
    Darwin) printf 'macos\n' ;;
    *)
      echo "Unsupported TUI installation test platform: $1" >&2
      return 1
      ;;
  esac
}

normalize_tui_install_arch() {
  case "$1" in
    x86_64|amd64) printf 'amd64\n' ;;
    arm64|aarch64) printf 'arm64\n' ;;
    *)
      echo "Unsupported TUI installation test architecture: $1" >&2
      return 1
      ;;
  esac
}

verify_tui_install_runner() {
  local actual_platform="$1"
  local actual_arch="$2"
  local expected_platform="${TUI_INSTALL_EXPECTED_PLATFORM:-}"
  local expected_arch="${TUI_INSTALL_EXPECTED_ARCH:-}"

  if [[ -n "${expected_platform}" || -n "${expected_arch}" ]]; then
    if [[ -z "${expected_platform}" || -z "${expected_arch}" ]]; then
      echo "TUI installation runner expectations require both platform and architecture" >&2
      return 1
    fi
    if [[ "${actual_platform}" != "${expected_platform}" || "${actual_arch}" != "${expected_arch}" ]]; then
      echo "TUI installation runner mismatch: expected ${expected_platform}-${expected_arch}, got ${actual_platform}-${actual_arch}" >&2
      return 1
    fi
  fi

  echo "CASE tui_install_runner PASS platform=${actual_platform} arch=${actual_arch}"
}

build_tui_release_fixture() {
  local scenario_root="$1"
  local product_dir="${TUI_INSTALL_PRODUCT_DIR:-${scenario_root}/products}"
  local release_dir="${scenario_root}/releases/download/v${TUI_INSTALL_E2E_VERSION}"

  if [[ -z "${TUI_INSTALL_PRODUCT_DIR:-}" ]]; then
    PROFILE=release \
      PRODUCT_DIR="${product_dir}" \
      "${REPO_ROOT}/build.sh" tui
  fi
  PROFILE=release PRODUCT_DIR="${product_dir}" \
    "${REPO_ROOT}/build/scripts/verify-release-artifacts.sh" tui

  mkdir -p "${release_dir}"
  cp "${product_dir}/tui"/Talos-TUI-*.tar.gz "${release_dir}/"
  cp "${product_dir}/tui/SHA256SUMS" "${release_dir}/checksums.txt"
}

run_installed_tui_acceptance() {
  local scenario_root="$1"
  local isolated_home="${scenario_root}/home"
  local install_root="${scenario_root}/install"
  local bin_dir="${scenario_root}/bin"
  local platform
  local architecture
  local config_path
  local config_template_path
  local first_run_log
  local first_run_pid
  local resource_config_path
  local talos_home="${scenario_root}/talos-home"
  local version_output
  local status_log
  local release_root="${scenario_root}/releases"

  platform="$(normalize_tui_install_platform "$(uname -s)")"
  architecture="$(normalize_tui_install_arch "$(uname -m)")"
  verify_tui_install_runner "${platform}" "${architecture}"

  mkdir -p "${isolated_home}"
  build_tui_release_fixture "${scenario_root}"
  HOME="${isolated_home}" \
    TALOS_INSTALL_ROOT="${install_root}" \
    TALOS_BIN_DIR="${bin_dir}" \
    TALOS_RELEASE_BASE_URL="file://${release_root}" \
    "${REPO_ROOT}/deploy/install/install.sh" --version "${TUI_INSTALL_E2E_VERSION}"

  local talos_command="${bin_dir}/talos"
  [[ -L "${talos_command}" ]] || {
    echo "Installed Talos command is not a symlink: ${talos_command}" >&2
    exit 1
  }
  echo "CASE tui_install_command_link PASS platform=${platform} arch=${architecture} command=${talos_command}"

  version_output="$(env -u TALOS_RESOURCE_DIR HOME="${isolated_home}" "${talos_command}" --version 2>&1)"
  if [[ "${version_output}" != "talos ${TUI_INSTALL_E2E_VERSION}" ]]; then
    echo "Installed Talos command returned an unexpected version: ${version_output}" >&2
    exit 1
  fi
  echo "CASE tui_install_version PASS platform=${platform} arch=${architecture} output=${version_output}"

  [[ ! -e "${talos_home}" ]] || {
    echo "Talos home must be empty before first-run validation: ${talos_home}" >&2
    exit 1
  }
  config_path="${talos_home}/conf/talos.yaml"
  config_template_path="${talos_home}/conf/talos.yaml.template"
  resource_config_path="${install_root}/current/conf/talos.yaml"
  first_run_log="$(scenario_report_path "installed_tui_first_run.log")"
  env -u TALOS_RESOURCE_DIR \
    HOME="${isolated_home}" \
    TALOS_HOME="${talos_home}" \
    TALOS_TUI_NO_BROWSER=1 \
    "${talos_command}" status >"${first_run_log}" 2>&1 &
  first_run_pid=$!
  for _ in {1..120}; do
    if [[ -f "${config_path}" && -f "${config_template_path}" ]]; then
      break
    fi
    if ! kill -0 "${first_run_pid}" 2>/dev/null; then
      wait "${first_run_pid}" 2>/dev/null || true
      cat "${first_run_log}" >&2
      echo "Installed Talos exited before initializing the default config" >&2
      exit 1
    fi
    sleep 0.25
  done
  if [[ ! -f "${config_path}" || ! -f "${config_template_path}" ]]; then
    kill -TERM "${first_run_pid}" 2>/dev/null || true
    wait "${first_run_pid}" 2>/dev/null || true
    cat "${first_run_log}" >&2
    echo "Installed Talos did not initialize the default config in time" >&2
    exit 1
  fi
  if kill -0 "${first_run_pid}" 2>/dev/null; then
    kill -TERM "${first_run_pid}" 2>/dev/null || true
  fi
  wait "${first_run_pid}" 2>/dev/null || true
  cmp -s "${config_path}" "${resource_config_path}"
  cmp -s "${config_template_path}" "${resource_config_path}"
  echo "CASE tui_install_first_run PASS platform=${platform} arch=${architecture} template_initialized=true"

  export TALOS_HOME="${talos_home}"
  apply_test_config_overrides "${config_path}"
  unset TALOS_HOME
  status_log="$(scenario_report_path "installed_tui_status.log")"
  if ! env -u TALOS_RESOURCE_DIR \
    HOME="${isolated_home}" \
    TALOS_HOME="${talos_home}" \
    TALOS_TUI_NO_BROWSER=1 \
    "${talos_command}" status >"${status_log}" 2>&1; then
    cat "${status_log}" >&2
    exit 1
  fi
  cat "${status_log}"

  grep -Fq "Status: Talos runtime accessible" "${status_log}"
  echo "CASE tui_install_status PASS platform=${platform} arch=${architecture} runtime_accessible=true"
  [[ -f "${config_path}" ]]
  cmp -s "${config_template_path}" "${resource_config_path}"
  [[ -f "${talos_home}/data/talos.db" ]]
  compgen -G "${talos_home}/conf/migrations/sqlite/*.sql" >/dev/null
  [[ -f "${talos_home}/conf/migrations/postgres/README.md" ]]
  echo "CASE tui_install_resources PASS platform=${platform} arch=${architecture} database=true migrations=true"

  printf '\nlegacy_install_test: true\n' >>"${config_path}"
  printf '# stale template\n' >"${config_template_path}"
  if ! env -u TALOS_RESOURCE_DIR \
    HOME="${isolated_home}" \
    TALOS_HOME="${talos_home}" \
    TALOS_TUI_NO_BROWSER=1 \
    "${talos_command}" status >>"${status_log}" 2>&1; then
    cat "${status_log}" >&2
    exit 1
  fi
  if grep -Fq "legacy_install_test" "${config_path}"; then
    echo "Legacy config key was not removed during upgrade" >&2
    exit 1
  fi
  cmp -s "${config_template_path}" "${resource_config_path}"
  compgen -G "${config_path}.bak.*" >/dev/null
  echo "CASE tui_install_config_upgrade PASS platform=${platform} arch=${architecture} template_refreshed=true config_migrated=true"

  [[ -e "${isolated_home}/.talos" ]] && {
    echo "Installer unexpectedly wrote to the isolated default Talos home" >&2
    exit 1
  }
  echo "CASE tui_install_home_isolation PASS platform=${platform} arch=${architecture}"

  echo "CASE tui_install_acceptance PASS platform=${platform} arch=${architecture} command=${talos_command}"
}

run_linux_docker_tui_install() {
  local dockerfile="${TALOS_E2E_DIR}/docker/tui-install-ubuntu-24.04.Dockerfile"
  local container_report_dir
  local docker_arch
  local target_dir

  require_command docker
  docker info >/dev/null
  docker_arch="$(docker info --format '{{.Architecture}}')"
  target_dir="${TALOS_E2E_TMP_ROOT}/tui-install-linux-target-${docker_arch}"
  container_report_dir="${SCENARIO_REPORT_DIR}/container"
  local cargo_lock_file="${target_dir}/Cargo.lock"
  mkdir -p "${target_dir}" "${container_report_dir}"
  : >"${cargo_lock_file}"
  mkdir -p "${REPO_ROOT}/talos-rs/target"

  docker build -t "${TUI_INSTALL_DOCKER_IMAGE}" -f "${dockerfile}" "${REPO_ROOT}"
  docker run --rm \
    -v "${REPO_ROOT}:/workspace/talos:ro" \
    -v "${target_dir}:/workspace/talos/talos-rs/target" \
    -v "${cargo_lock_file}:/workspace/talos/talos-rs/Cargo.lock" \
    -v "${container_report_dir}:/tmp/talos-e2e-report" \
    -e TALOS_E2E_TMP_ROOT=/tmp/talos-e2e \
    -e TALOS_SCENARIO_REPORT_DIR=/tmp/talos-e2e-report \
    "${TUI_INSTALL_DOCKER_IMAGE}" \
    bash -lc "/workspace/talos/talos-e2e/scenarios/15_tui_install/01_native_install.sh"
}
