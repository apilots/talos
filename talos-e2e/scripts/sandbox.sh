#!/usr/bin/env bash
set -euo pipefail

SANDBOX_DOCKER_IMAGE="talos-sandbox-e2e:ubuntu-24.04"

run_native_sandbox_case() {
  local case_name="$1"
  local helper=""

  if [[ "$(uname -s)" == "Linux" ]]; then
    (
      cd "${TALOS_RS_DIR}"
      cargo build -p talos-linux-sandbox --bin talos-linux-sandbox-launcher
    )
    helper="${TALOS_RS_DIR}/target/debug/talos-linux-sandbox-launcher"
  fi

  (
    cd "${REPO_ROOT}"
    SHELL=/bin/sh TALOS_LINUX_SANDBOX_HELPER="${helper}" \
      just test -p talos-tools "sandbox_e2e_tests::${case_name}" --no-capture --run-ignored ignored-only
  )
}

run_linux_docker_sandbox_case() {
  local case_name="$1"
  local dockerfile="${TALOS_RS_DIR}/crates/action/linux-sandbox/tests/docker/ubuntu-24.04.Dockerfile"
  local target_dir="${TALOS_E2E_TMP_ROOT}/sandbox-linux-target"
  mkdir -p "${target_dir}"

  docker build -t "${SANDBOX_DOCKER_IMAGE}" -f "${dockerfile}" "${REPO_ROOT}"
  docker run --rm --privileged \
    -v "${REPO_ROOT}:/workspace/talos" \
    -v "${target_dir}:/workspace/talos/talos-rs/target" \
    "${SANDBOX_DOCKER_IMAGE}" \
    bash -lc "cargo build -p talos-linux-sandbox --bin talos-linux-sandbox-launcher && SHELL=/bin/sh TALOS_LINUX_SANDBOX_HELPER=/workspace/talos/talos-rs/target/debug/talos-linux-sandbox-launcher just test -p talos-tools sandbox_e2e_tests::${case_name} --no-capture --run-ignored ignored-only"
}

run_linux_docker_sandbox_group() {
  local dockerfile="${TALOS_RS_DIR}/crates/action/linux-sandbox/tests/docker/ubuntu-24.04.Dockerfile"
  local target_dir="${TALOS_E2E_TMP_ROOT}/sandbox-linux-target"
  mkdir -p "${target_dir}"

  docker build -t "${SANDBOX_DOCKER_IMAGE}" -f "${dockerfile}" "${REPO_ROOT}"
  docker run --rm --privileged \
    -v "${REPO_ROOT}:/workspace/talos" \
    -v "${target_dir}:/workspace/talos/talos-rs/target" \
    "${SANDBOX_DOCKER_IMAGE}" \
    bash -lc "cargo build -p talos-linux-sandbox --bin talos-linux-sandbox-launcher && SHELL=/bin/sh TALOS_LINUX_SANDBOX_HELPER=/workspace/talos/talos-rs/target/debug/talos-linux-sandbox-launcher just test -p talos-tools sandbox_e2e_tests --no-capture --run-ignored ignored-only"
}

run_sandbox_case() {
  local case_name="$1"
  run_native_sandbox_case "${case_name}"
  if [[ "$(uname -s)" == "Darwin" && "${TALOS_E2E_GROUP_NAME:-}" != "14_sandbox" ]]; then
    run_linux_docker_sandbox_case "${case_name}"
  fi
}
