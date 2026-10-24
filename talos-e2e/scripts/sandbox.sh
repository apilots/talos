#!/usr/bin/env bash
set -euo pipefail

SANDBOX_DOCKER_IMAGE="${SANDBOX_DOCKER_IMAGE:-}"

run_native_sandbox_case() {
  local case_name="$1" helper=""
  if [[ "$(uname -s)" == Linux ]]; then
    build_sandbox_binary
    helper="${TALOS_E2E_SANDBOX_BIN}"
  fi
  if [[ -n "${TALOS_TEST_ARCHIVE:-}" ]]; then set --; else set -- -p talos-tools; fi
  (
    cd "${REPO_ROOT}"
    SHELL=/bin/sh TALOS_LINUX_SANDBOX_HELPER="${helper}" \
      just test "$@" "sandbox_e2e_tests::${case_name}" --no-capture --run-ignored ignored-only
  )
}

run_linux_docker_sandbox_tests() {
  local filter="$1" image
  local dockerfile="${TALOS_RS_DIR}/crates/action/linux-sandbox/tests/docker/ubuntu-24.04.Dockerfile"
  local mounts=(-v "${REPO_ROOT}:/workspace/talos" -w /workspace/talos/talos-rs)
  if [[ -n "${TALOS_TEST_ARCHIVE:-}" ]]; then
    # Keep the original source path for tests embedding CARGO_MANIFEST_DIR.
    mounts=(-v "${REPO_ROOT}:${REPO_ROOT}" -w "${TALOS_RS_DIR}")
    [[ "${TALOS_TEST_ARCHIVE}" == "${REPO_ROOT}/"* ]] || { echo 'Test archive must be inside the source checkout' >&2; return 1; }
    test -f "${TALOS_TEST_ARCHIVE}"
    test -x "${TALOS_E2E_SANDBOX_BIN:?Missing supplied sandbox helper}"
    mounts+=(-e "TALOS_TEST_ARCHIVE=${TALOS_TEST_ARCHIVE}" -e "TALOS_LINUX_SANDBOX_HELPER=${TALOS_E2E_SANDBOX_BIN}")
  else
    local target_volume="sandbox-target-${TALOS_E2E_RUN_ID}-${RANDOM}"
    e2e_docker volume "${target_volume}" >/dev/null
    mounts+=(-v "${target_volume}:/workspace/talos/talos-rs/target")
  fi
  image="$(e2e_image talos-sandbox-e2e "${SANDBOX_DOCKER_IMAGE}" -f "${dockerfile}" "${REPO_ROOT}")"
  e2e_docker container "sandbox-${TALOS_E2E_RUN_ID}-${RANDOM}" --rm --privileged \
    "${mounts[@]}" "${image}" bash -lc '
      set -euo pipefail
      export SHELL=/bin/sh
      if [[ -n "${TALOS_TEST_ARCHIVE:-}" ]]; then
        just test "$1" --no-capture --run-ignored ignored-only
      else
        cargo build -p talos-linux-sandbox --bin talos-linux-sandbox-launcher
        export TALOS_LINUX_SANDBOX_HELPER="$PWD/target/debug/talos-linux-sandbox-launcher"
        just test -p talos-tools "$1" --no-capture --run-ignored ignored-only
      fi' sandbox "${filter}"
}

run_linux_docker_sandbox_case() { run_linux_docker_sandbox_tests "sandbox_e2e_tests::$1"; }
run_linux_docker_sandbox_group() { run_linux_docker_sandbox_tests sandbox_e2e_tests; }

run_sandbox_case() {
  local case_name="$1"
  run_native_sandbox_case "${case_name}"
  if [[ "$(uname -s)" == Darwin && "${TALOS_E2E_GROUP_NAME:-}" != 14_sandbox ]]; then
    run_linux_docker_sandbox_case "${case_name}"
  fi
}
