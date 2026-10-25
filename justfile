set working-directory := "talos-rs"
set positional-arguments

rust_min_stack := "8388608"

# Display available commands.
help:
    just -l

# Install local developer command helpers.
install-tools:
    cargo install --locked just
    cargo install --locked cargo-nextest

# Enable the repository-managed Git hooks for this clone.
setup-git-hooks:
    git -C .. config core.hooksPath .githooks

# Verify categorized and uncategorized commit subject behavior.
test-commit-messages:
    ../scripts/check-commit-message-tests.sh

# Verify release note category and filtering behavior.
test-release-notes:
    ../scripts/generate-release-notes-tests.sh

# Verify changed-file policy behavior.
test-change-policy:
    python3 ../scripts/check_changed_files_tests.py

# Run the checks applied to the current staged snapshot.
check-staged:
    ../scripts/run-pre-commit-checks.sh

# Format all Rust crates in the workspace.
fmt:
    cargo fmt --all

# Check formatting without modifying files.
fmt-check:
    cargo fmt --all -- --check

# Build selected packages or the current workspace selection.
build *args:
    cargo build "$@"

# Run the development server with repository resources.
dev-server *args:
    TALOS_RESOURCE_DIR="$PWD/.." cargo run -p talos-server --bin talos-server "$@"

# Run the standalone Agent worker with repository resources.
dev-worker *args:
    TALOS_RESOURCE_DIR="$PWD/.." cargo run -p talos-worker-bin --bin talos-worker "$@"

# Run the development Web UI with repository resources in the process environment.
dev-web *args:
    TALOS_RESOURCE_DIR="$PWD/.." pnpm --dir ../talos-app dev "$@"

# Type-check selected packages or the current workspace selection.
check *args:
    cargo check "$@"

# Run tests through nextest. Prefer scoped runs such as `just test -p talos-service`.
test *args:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -n "${TALOS_TEST_ARCHIVE:-}" ]]; then
      test -f "${TALOS_TEST_ARCHIVE}"
      if [[ "$(uname -s)" == Linux ]]; then test -x "${TALOS_LINUX_SANDBOX_HELPER:?Missing supplied sandbox helper}"; fi
      RUST_MIN_STACK={{ rust_min_stack }} cargo-nextest nextest run --no-fail-fast --archive-file "${TALOS_TEST_ARCHIVE}" --workspace-remap "$PWD" "$@"
    else
      if [[ "$(uname -s)" == Linux ]]; then
        if [[ -n "${TALOS_LINUX_SANDBOX_HELPER:-}" ]]; then test -x "${TALOS_LINUX_SANDBOX_HELPER}";
        else cargo build -p talos-linux-sandbox --bin talos-linux-sandbox-launcher; fi
      fi
      RUST_MIN_STACK={{ rust_min_stack }} cargo nextest run --no-fail-fast "$@"
    fi

# Compile exactly the caller's package selection for relocation to test consumers.
test-archive *args:
    cargo nextest archive "$@"

# Build the single-platform reusable sandbox environment, separately from PR tests.
sandbox-base *args:
    docker buildx build --platform linux/amd64 --target runtime -f crates/action/linux-sandbox/tests/docker/ubuntu-24.04.Dockerfile "$@" ..

# Run fast Clippy checks for selected packages without linting path dependencies.
clippy *args:
    cargo clippy --no-deps "$@"

# Run Clippy against test targets without linting path dependencies.
clippy-tests *args:
    cargo clippy --no-deps --tests "$@"

# Apply clippy fixes for selected packages. Prefer `just fix -p <package>`.
fix *args:
    cargo clippy --fix --tests --allow-dirty "$@"

# Regenerate app protocol schema and TypeScript artifacts.
gen-app-protocol:
    cargo run -p talos-app-protocol --example generate_app_protocol_artifacts

# Regenerate brand and cross-platform application icon assets from the vector master.
gen-logo-assets:
    ../scripts/generate-logo-assets.sh

# Run E2E scenarios. Pass runner args such as `--group 01_models` or `--case 01_models/health_and_models`.
e2e *args:
    ../talos-e2e/run_e2e.sh "$@"

# Run one E2E scenario by relative path or basename without .sh.
e2e-case case:
    ../talos-e2e/run_e2e.sh --case "{{case}}"

# Run all E2E scenarios under one scenario group.
e2e-group group:
    ../talos-e2e/run_e2e.sh --group "{{group}}"

# Remove incremental build state while preserving compiled dependencies.
clean-incremental:
    rm -rf target/debug/incremental target/release/incremental target/talos-check/debug/incremental

# Remove debug build artifacts. Use when target/debug has grown too large.
clean-debug:
    rm -rf target/debug target/talos-check

# Remove release build artifacts.
clean-release:
    rm -rf target/release

# Show target directory disk usage by top-level bucket.
target-size:
    du -sh target/* 2>/dev/null | sort -h

# Product images consume the public synchronization SDK through Cargo.lock.
docker-server-build image="talos-server:local":
    #!/usr/bin/env bash
    set -euo pipefail
    cd ..
    docker build -f build/server/Dockerfile -t "{{image}}" ..

# Standalone execution image; no WebUI or Server process is included.
docker-worker-build image="talos-worker:local":
    #!/usr/bin/env bash
    set -euo pipefail
    cd ..
    docker build -f build/worker/Dockerfile -t "{{image}}" ..
