# Scenario resource lifecycle

The existing numbered discovery/selection is the only case catalog. Every case,
including exec-based group17/18/19 wrappers, runs through the same before/after
case boundary. `TALOS_E2E_RUN_ID` identifies one invocation; no second scheduler,
Docker CLI shim, or scenario registry is introduced.

## Contract

- Before a case, allocate private temporary storage and launch its shell in an
  owned process group. Set TMPDIR/TMP/TEMP, a private tmux socket namespace and
  the existing E2E home/temp/native output roots. Unix sockets use a short
  marked private directory under /tmp for TMPDIR and tmux, registered for
  case/run cleanup and linked from the case tmp path. This avoids Chromium/tmux
  socket limits in nested worktrees. Detached children in either path are owned.
  Reports remain outside disposable storage. Never infer ownership from a port.
- Keep scenario-specific traps/finally blocks for graceful shutdown. Register
  detached process groups and Docker resources at their existing creation sites,
  before a later assertion can fail. `lifecycle` supplies the concrete Python
  registration/create helpers; `common.sh` exposes the same shell interface.
- After every case, terminate owned process groups, then remove owned containers,
  volumes/networks and temporary files. Already-removed resources are harmless;
  a real cleanup failure is reported and makes acceptance fail. Preserve the
  original case failure as the primary result and append cleanup diagnostics.
- On INT/TERM, stop admission and clean the active case and run resources. The
  run finalizer retries registered leftovers. SIGKILL cannot be trapped; retained
  ownership records permit an explicit scoped cleanup, never a global prune.
- Images built by this run have unique tags and run labels, are reused across
  the selected group/cases, and are removed at run end. Supplied images, base
  images, package/Cargo caches and BuildKit caches remain external. Do not build
  over a caller's image tag. Never remove a resource with another run's labels.
- New cases inherit directory/process lifecycle automatically. A resource-owning
  fixture must use the registration interface for detached/out-of-process
  resources. New resource kinds require a real current owner and directed cleanup
  tests, not a generic plugin mechanism.

## Ownership coverage

| Existing cases | Resource owner and adoption |
| --- | --- |
| 01–03,05–08,12,13 | common case shell group/home/temp; existing stop_server/TUI/native S3 gateway traps; private tmux server cleanup |
| 09 | case shell group and private temp cover inline subprocess/Chrome; existing app-server helpers keep graceful shutdown |
| 11,17,18,19/04,20/01–02 | multi_node Cluster, Desktop and UI fixtures; private case roots and registered detached processes |
| 14,15 | sandbox/TUI helpers; owned Docker containers and once-per-run built images |
| 19 except04 | SyncCluster/NAT Docker containers/networks; run-owned candidate images; immutable Sync image remains external |
| 20/03–04 | prepared Chrome/Electron private process groups, registered at creation; temporary profiles are case-owned |
| 20/05 | Browser container/volume and media/native processes registered; supplied Worker image remains external |

Binary preparation uses explicit executable overrides first, then the existing
Cargo build functions, once per selected invocation. The sandbox launcher follows
the chosen runtime's directory/profile; do not compile an unused debug graph when
CI supplied release binaries. Invalid explicit overrides fail without fallback.

Verification: runner tests use real controlled child processes and Docker stubs
for success/failure/interrupt/cleanup-error and ownership preservation; directed
Docker acceptance checks actual resource removal without paid model calls.

## Resource-owner interface

The case execution boundary is mandatory and automatic for every discovered case;
there is no per-case opt-in or empty hook to maintain. At actual creation sites:

```bash
# common.sh must already be sourced by a scenario.
e2e_docker volume "fixture-${TALOS_E2E_RUN_ID}"       # case-owned
image="$(e2e_image fixture-image "${SUPPLIED_IMAGE:-}" -f Dockerfile .)"
e2e_docker container "fixture-${TALOS_E2E_RUN_ID}" --rm "${image}" command
```

```python
from lifecycle import container_args, docker_create, own_process

volume = docker_create('volume', unique_name, [])
# For an existing docker command builder, register before running it:
command = ['docker', 'run', *container_args(unique_container), '-d', image]
# Only processes explicitly launched in a new session use own_process.
child = own_process(subprocess.Popen(argv, start_new_session=True))
```

Use existing fixture UUID/name generation for repeated case resources. Files must
live under `TALOS_E2E_TMP_ROOT` (or TMPDIR), not a fixed repository/global temp
path; persistent evidence belongs in `TALOS_SCENARIO_REPORT_DIR`. Ordinary shell
children inherit the owned group. Detached product/MCP children that outlive their
parent are collected only by this case's private cwd/profile path, never by
executable name. Caller-provided paths/images remain externally owned.

Before adding a resource-owning case, identify its current shared fixture, register
creation before any assertion can fail, preserve graceful teardown, and verify the
failure/interruption path leaves unrelated resources intact. Do not weaken business
assertions to accommodate cleanup. A cleanup error retains the ledger at
`$TALOS_E2E_RESOURCE_ROOT/resources.jsonl`; with the same run ID/root, use
`python3 talos-e2e/scripts/lifecycle.py cleanup` to retry only recorded resources.

Docker-hosted Rust builds put target output in a registered case-owned named
volume. A root container must not write disposable build artifacts into a host
bind directory: a non-root Linux runner cannot reliably remove those files.
Retain the existing report bind mount for evidence, and retire the build volume
after its container. Native fixture assets resolve beside the emitted bundle;
fixture relocation must not retain repository-global output/profile paths.
