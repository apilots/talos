# Development workflow

[Chinese](DEVELOPMENT_zh.md)

Run repository commands from the repository root. The root `justfile` executes
Rust commands in `talos-rs` and exposes shared E2E and generation workflows.

## Setup

Repository policy and release scripts require Python 3.11 or newer for standard
library TOML parsing. Keep that interpreter on `PATH` when running Git hooks.

```bash
just install-tools
just setup-git-hooks
```

Rust builds generate the Agent gRPC bindings and require `protoc` on `PATH`.
Install it with `brew install protobuf` on macOS or
`sudo apt-get install protobuf-compiler` on Debian/Ubuntu before running the
Rust checks. The Docker builder and source-build CI provision it explicitly.

Rust product builds consume the public `talos-cloud-sdk = "=0.1.3"` and
`talos-sync-sdk` registry packages; no adjacent Cloud source checkout is used.
Isolated authentication E2E scenarios launch a configured Cloud fixture binary,
selected with `TALOS_CLOUD_SERVER_BIN` or found as `cloud-server` on `PATH`.
Public PR checks use secretless scenarios; provider-backed checks run only through
the maintainer-dispatched workflow on `main`. See
[contribution guidance](../../CONTRIBUTING.md).

Frontend dependencies are managed separately:

```bash
pnpm --dir talos-app install
```

## Commit subjects

Non-merge commits follow the Conventional Commit types and subject forms defined
in the [Release workflow](RELEASE.md#commit-subjects). Install the repository
hooks with `just setup-git-hooks`; validate the contract with
`just test-commit-messages`.

## Scoped validation

```bash
just fmt-check
just check -p talos-service
just test -p talos-service
just clippy -p talos-service
```

Run `just fmt` after Rust edits. Broad Rust changes require `just fix -p <package>`
before committing, after reviewing the affected package. Shared or high-impact
changes may require broader checks after scoped validation. Use `just test`
through cargo-nextest rather than substituting bare `cargo test`.

Frontend validation is run from `talos-app`:

```bash
pnpm --dir talos-app lint
pnpm --dir talos-app test
pnpm --dir talos-app typecheck
pnpm --dir talos-app build
pnpm --dir talos-app check:repo
```

Use `pnpm --dir talos-app --filter <package> <script>` for a scoped frontend
check, for example `pnpm --dir talos-app --filter @talos/sdk-core typecheck`.

## Validation by delivery stage

```text
Local iteration --> pre-commit --> pre-push --> PR / main CI --> source Release
 scoped behavior    staged files  changed areas  path filters    public gates
 + quality gates    + fast checks + quality      + quality       before publication
```

| Stage | Current responsibility |
| --- | --- |
| Local | Scoped `just test -p <package>` and frontend scenarios, plus applicable quality gates |
| pre-commit | Changed-file policy, selected frontend ESLint, shell syntax, Rust formatting |
| pre-push | Product changes trigger frontend or Rust checks; tags validate metadata and require publisher/CI candidate provenance |
| PR/main CI | Shared content-aware scopes include build/workflow inputs; version-only changes run metadata/policy checks |
| Release | Public workflow runs source checks before builds and native installation before publication |

See [hook scripts](../../scripts/run-pre-push-checks.sh) and
[Rust workflow](../../.github/workflows/talos-rust.yml) for exact commands.
Shared Rust/Node/pnpm versions come from `.github/release-toolchain.env`. Rust
caches are isolated by platform, architecture, profile, toolchain and the resolved
Cargo graph. Cache hits never replace compilation or behavioral checks.
Changes limited to `talos-app/web/src/**` and `talos-app/web/public/**` run
frontend quality and Computer-registration acceptance against the real native
service, plus lightweight resource acceptance or the Browser group according to
its existing owner selection. Changes limited to `talos-app/desktop/src/**` and
`talos-app/desktop/resources/**` run frontend quality and native Browser cases
`20/02`, `20/03`, and `20/04` in the dedicated Browser job, without generic E2E,
Rust contract tests, or a Worker image. Desktop media, preload, IPC, and controller
owners also retain `20/05` and its packaged Docker Worker image.

SDK, dependency, configuration, build, backend, scenario, toolchain, and mixed
Web/Desktop inputs retain the full route. The existing unified selector owns all
these decisions. Its own routing definitions and regressions are checked by the
always-on policy workflow; including those files does not widen an otherwise
bounded product diff. Routing-only changes remain conservative. The fixed
`talos-pr-e2e` gate requires selected checks to succeed, unselected jobs to skip,
and valid scope outputs; an empty acceptance selection for product changes fails.

CI success does not substitute for unrun scoped tests, snapshots or real-path
acceptance. Error changes require source/disclosure, transport/replay coverage
and generation per [Error contract](../architecture/ERROR_MODEL.md).
For documentation-only changes, verify links, examples, ASCII layout and source
claims; do not claim runtime behavior was tested by a Markdown check.

## Behavioral verification

- Verify observable behavior across affected parsing, service, transport, SDK, UI/TUI, and runtime boundaries. Add regression coverage for executable bug fixes. Talos reasoning, tool calls, context injection, session state, or runtime policy changes require integration tests.
- Unit and local integration tests are deterministic: no real external services, external network, unstable timing, or implicit process-global environment. Controlled local test servers are allowed for transport checks; pass flags, config, or dependencies instead of mutating process environment.
- Real integration E2E may use external services, networks, and models. Record prerequisites and unmet conditions; simulated tests do not replace real acceptance.
- Manual validation exercises the complete affected user path with all required services, including frontend and backend when both are needed. Screenshots or logs demonstrate behavior, not just a static page opening.
- Reuse test helpers and fixtures. Avoid tests of static definitions or negative tests for removed logic. Rust-specific assertions and layout are defined in [Test organization](RUST.md#test-organization).

## Plans and durable evidence

For complex or staged work, maintain a task-owned `IMPLEMENTATION_PLAN.md`
(ignored by Git) with stages, success criteria, checks, and factual status.
Reuse the current task's plan; when the root plan belongs to another task, keep
this plan alongside the current task's spec. Simple edits need no new plan.
Specs describe architecture, objects, APIs, state schemas, errors, and safety
constraints. Tracked documentation may include reusable verification procedures
and acceptance checklists, but actual self-test/acceptance records, result tables,
logs and screenshots belong only in ignored root `output/`, not verification-only
spec or deployment Markdown. Keep implementation decisions in their owning
contracts and summarize actual verification, limitations and blockers in PR
evidence before retiring a local plan. Historical verification is not a new result.

At stage boundaries, check scope, non-goals, reusable infrastructure, and the
smallest viable change. Complete that stage's checks and cleanup before starting
the next authorized stage. Partial implementation or compilation alone is not
stage completion; a completed stage does not require an extra approval unless
the task or an applicable contract explicitly requires it.

Keep non-mechanical changes within approximately 800 changed lines and complex
logic changes within 500; split larger work into reviewable stages and identify
the first deployable slice. Split modules by responsibility, including pages,
dialogs, lists, search, forms, runtime panels, and TUI panels. Target Rust modules
below 500 lines excluding tests; do not add responsibilities to files above
approximately 800 lines without a recorded reason. Keep orchestration files
focused and move related tests, docs, and invariants with extracted code.

## Runtime configuration

Talos uses `${TALOS_HOME:-$HOME/.talos}` as its runtime home. The main files are:

```text
$TALOS_HOME/conf/talos.yaml
$TALOS_HOME/.env
$TALOS_HOME/data/talos.db
$TALOS_HOME/logs/talos.log
```

Keep provider secrets in `.env`; configuration references the corresponding
`TALOS_LLM_*_API_KEY` variables. Repository defaults live under `conf/` and are
copied or upgraded by the runtime rather than edited in a user's home directly.

`talos.yaml`, `talos-dev.yaml`, and `talos-prod.yaml` share the same configuration
schema. The startup entrypoint selects Server or Worker capabilities. Both Docker
images package `talos-prod.yaml` as `conf/talos.yaml`; deployment examples supply
their listener, TLS and registration settings using that same schema. Keep every
writable field in maintained templates: managed upgrades remove keys absent from
the selected template. Derived paths and private registration state stay outside YAML.

## End-to-end tests

```bash
just e2e-case 01_health_and_models
just e2e-group 09_app_server
```

Use the smallest scenario that proves the changed behavior. See
[`talos-e2e/README.md`](../../talos-e2e/README.md) for scenario ownership and
environment requirements.

## Generated artifacts

Regenerate App Server schema and TypeScript protocol output after changing the
current protocol shape:

```bash
just gen-app-protocol
```

Generated files are changed only by their owning generator and are committed
with the source definition. Configuration type changes require the configuration
schema generator; experimental API changes include their experimental schemas.
Dependency changes update and commit `talos-rs/Cargo.lock` and applicable Bazel,
Buck, Nix, or other lockfiles. CI uses the committed Cargo graph with `--locked`;
update dependencies explicitly during development, never by regenerating the graph
in CI. Version preparation updates local workspace lock versions as described in
[Release workflow](RELEASE.md). New compile-time resources (`include_str!`, `include_bytes!`, migrations,
schemas, fixtures) require corresponding build declarations.

## Build storage

```bash
just target-size
just clean-incremental
```

Use `just clean-debug` only when a clean debug rebuild is necessary. Removing
the whole target directory is not a routine verification step. Wait for Rust
build locks and caches; do not kill build commands by PID.

See [Release workflow](RELEASE.md) for versioned product releases.
