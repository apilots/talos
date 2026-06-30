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

# Format all Rust crates in the workspace.
fmt:
    cargo fmt --all

# Check formatting without modifying files.
fmt-check:
    cargo fmt --all -- --check

# Build selected packages or the current workspace selection.
build *args:
    cargo build "$@"

# Type-check selected packages or the current workspace selection.
check *args:
    CARGO_TARGET_DIR=target/talos-check cargo check "$@"

# Run tests through nextest. Prefer scoped runs such as `just test -p talos-service`.
test *args:
    @if ! cargo nextest --version >/dev/null 2>&1; then \
      echo "cargo-nextest is required. Run: just install-tools" >&2; \
      exit 127; \
    fi
    RUST_MIN_STACK={{ rust_min_stack }} cargo nextest run --no-fail-fast "$@"

# Run clippy for selected packages or the current workspace selection.
clippy *args:
    cargo clippy --tests "$@"

# Apply clippy fixes for selected packages. Prefer `just fix -p <package>`.
fix *args:
    cargo clippy --fix --tests --allow-dirty "$@"

# Regenerate app protocol schema and TypeScript artifacts.
gen-app-protocol:
    cargo run -p talos-app-protocol --example generate_app_protocol_artifacts

# Run E2E scenarios. Pass runner args such as `--case 01_health_and_models`.
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
