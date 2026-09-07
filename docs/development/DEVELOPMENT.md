# Development workflow

[Chinese](DEVELOPMENT_zh.md)

Run repository commands from the repository root. The root `justfile` executes
Rust commands in `talos-rs` and exposes shared E2E and generation workflows.

## Setup

```bash
just install-tools
just setup-git-hooks
```

Frontend dependencies are managed separately:

```bash
pnpm --dir talos-app install
```

## Scoped validation

```bash
just fmt-check
just check -p talos-service
just test -p talos-service
just clippy -p talos-service
```

Use `just fix -p <package>` only after reviewing the affected package. Shared
or high-impact changes may require broader checks after scoped validation.

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
Local iteration --> pre-commit --> pre-push --> PR / main CI --> release tag
 scoped behavior    staged files  changed areas  path filters    native install
 + quality gates    + fast checks + quality      + quality       before dispatch
```

| Stage | Current responsibility |
| --- | --- |
| Local | Scoped `just test -p <package>` and frontend scenarios, plus applicable quality gates |
| pre-commit | Changed-file policy, selected frontend ESLint, shell syntax, Rust formatting |
| pre-push | Changed areas trigger frontend pipeline or Rust format/check and error-contract guards |
| PR/main CI | Path-filtered workflows; Rust quality checks do not run the full behavioral suite |
| Release | Native installation matrix must pass before release build dispatch |

See [hook scripts](../../scripts/run-pre-push-checks.sh) and
[Rust workflow](../../.github/workflows/talos-rust.yml) for exact commands.
CI success does not substitute for unrun scoped tests, snapshots or real-path
acceptance. Error changes require source/disclosure, transport/replay coverage
and generation per [Error contract](../architecture/ERROR_MODEL.md).
For documentation-only changes, verify links, examples, ASCII layout and source
claims; do not claim runtime behavior was tested by a Markdown check.

## Plans and durable evidence

Keep staged work in local `IMPLEMENTATION_PLAN.md` (ignored by Git). Specs own
design, not execution checklists. Preserve lasting decisions and verification
references in tracked specs or PR evidence before retiring a local plan. Record
which checks actually ran; historical verification is not a new test result.

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
with the source definition.

## Build storage

```bash
just target-size
just clean-incremental
```

Use `just clean-debug` only when a clean debug rebuild is necessary. Removing
the whole target directory is not a routine verification step.

See [Release workflow](RELEASE.md) for versioned product releases.
