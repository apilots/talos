# Local Agent benchmarks

Run a local benchmark with Docker, Docker Buildx, and uv. The launcher selects the daemon's native Linux platform. Image preparation follows each suite's pinned native recipes or published images.
Run the commands from the Talos repository root.

```bash
./talos-bench/run.sh --list
./talos-bench/run.sh terminal-bench
./talos-bench/run.sh terminal-bench@2.0
./talos-bench/run.sh swe-bench-lite
./talos-bench/run.sh mcp-atlas
./talos-bench/run.sh deep-swe
TALOS_BENCH_ORACLE=1 ./talos-bench/run.sh terminal-bench@4.1.0
TALOS_BENCH_ORACLE=1 ./talos-bench/run.sh deep-swe
TALOS_BENCH_ORACLE=1 ./talos-bench/run.sh frontier-harness@1.1.0
```

The five suite commands that omit Oracle mode run controlled Talos cases without model credentials. Terminal-Bench defaults to 4.1. Its fixture produces placeholder SQL with an expected reward of 0.0. Oracle mode runs the upstream reference solution and skips the Talos build and model setup.

To reuse a build, set `TALOS_BENCH_PRODUCT` to a native Linux TUI product directory containing `bin/talos`, `bin/talos-linux-sandbox-launcher`, `conf/talos.yaml`, and `conf/migrations/sqlite/`. Build from a revision that provides the trajectory and MCP import commands used by the suites.

## Select a Terminal-Bench version

Use `terminal-bench` or `terminal-bench@4.1.0` for Harbor's 66-task 4.1.0 release. It reads [registry.json](../../talos-bench/suites/terminal-bench/registry.json), defaults to `production-planning`, and exports ATIF for Talos runs. Use `terminal-bench@2.0` for the separate 89-task Laude release. It reads [registry-2.0.json](../../talos-bench/suites/terminal-bench/registry-2.0.json) and defaults to `regex-log`.

Set `TALOS_BENCH_TASKS` to a selectable case ID from the chosen version. The 2.0 fixture uses `regex-log`. To run the upstream solution for another case, use `TALOS_BENCH_ORACLE=1 TALOS_BENCH_TASKS=music-harmony ./talos-bench/run.sh terminal-bench@4.1.0`. See the [Terminal-Bench suite guide](../../talos-bench/README.md#terminal-bench-tasks) for release pins, artifact collection, and execution prerequisites.

## Run with a configured model

Select a catalog key and point the runner at an existing Talos config and dotenv. The provider's API key must use a `${TALOS_...}` reference in that config.

```bash
export TALOS_BENCH_MODEL=openai:gpt-6.1-terra
export TALOS_BENCH_CONFIG=/path/to/talos/conf/talos.yaml
export TALOS_BENCH_DOTENV=/path/to/talos/.env
./talos-bench/run.sh terminal-bench
```

The runner transfers the selected provider settings and its single dotenv assignment to the isolated task container. Talos reads them through its usual configuration loader. With the provider key already exported as `OPENAI_API_KEY`, omit the Talos config and dotenv and set `TALOS_BENCH_MODEL=openai:gpt-6.1-terra`. The adapter uses the shipped Talos catalog and its OpenAI Responses connection. `OPENAI_BASE_URL` can override the catalog endpoint.

Pass `--reasoning-effort light|medium|high|xhigh` when the selected model supports that effort. The flag takes precedence over `TALOS_BENCH_REASONING_EFFORT`. With neither setting, the launcher uses the selected model's catalog value. The override applies to the run's model configuration and is recorded in its evidence.

Use `TALOS_BENCH_AGENT_ENV` with one `KEY=VALUE` per line for non-secret agent runtime options. The launcher passes these options through Harbor or Pier's `--agent-env` to the Talos process. Provider credentials use the configured dotenv path.

Set `TALOS_BENCH_AGENT_ENV=TALOS_BENCH_COLLABORATION_TOOLS=0` for a local single-Agent diagnostic. The isolated Talos config disables collaboration tools and records that setting in `agent/talos-run.json` and the summary. Normal benchmark runs enable collaboration tools. Suite reports retain diagnostic scores in the attempt history. Compare runs using the same setting.

Set `TALOS_BENCH_DELEGATION_WATCH=1` through `TALOS_BENCH_AGENT_ENV` to sample delegation and workspace progress every 30 seconds. For tasks without a Git worktree, set watched paths to enable automatic stopping. It stops after 12 minutes without a changed submission, changed Git workspace, or completed child Turn when there are at least 12 child sessions, depth four, and three additional child sessions over the last three minutes. `TALOS_BENCH_WATCH_PATHS` accepts colon-separated submission files and directories. The Bench writes `agent/talos-delegation.jsonl` and publishes the latest `agent/talos-delegation-events.json` at each sample and on exit. The event file records lifecycle, typed warnings, active Turn state, and tool timings without tool bodies. Each snapshot retains the last durable metrics event per Turn, including its input and cache-read counters. These observations remain distinct from final CLI usage. At CLI exit, it also records observed child call usage and any finalized child Turn totals. These separate counters can be incomplete and do not define a workload-wide rate. An external stop can leave up to one sampling interval absent from the event file. The summary records the stop separately from a numeric verifier result. The upstream grader and deadlines remain those of the pinned task.

Use the summary for score and job time, `agent/talos-run.json` for the launch profile and CLI duration, and `agent/usage.json` for cumulative input, output, cache-read, and cache-write tokens. The launch profile is saved before the CLI starts. It retains a null exit code and CLI duration if the runner terminates the process before exit is recorded. The summary records the runner's observed agent phase separately as `agentSeconds`. Per-request usage is in the Talos trajectory. Configured-model software benchmark runs retain the transcript with tool arguments and results for local attribution. The root Turn cost estimate requires complete request coverage, four token counters on every call, matching CLI totals, and catalog rates for uncached input, cache reads, cache writes and output. Unknown counters leave that estimate unavailable. The summary reports pass rate over scored attempts and uses the suite’s declared passing threshold, defaulting to reward 1.0. Aggregate cost is available once every requested attempt has priced usage.

ATIF export retains recorded steps after a nonzero or interrupted CLI exit. A failed first request may contain only the original user step; the exporter never invents assistant content. Failed trajectories carry `talos_exit_code` and `talos_execution_status` in their `extra` metadata. The original launch profile, error and verifier reward remain independent evidence. Successful runs still require agent content.

`agent/provider-http-failures.json` retains allowlisted HTTP and transport failure events from the temporary product log. Each records the protocol, attempt, elapsed time, retry decision and HTTP status or transport failure. The event includes an upstream request ID when present and valid. The launch profile’s `gateway_retries` remains null until collection succeeds, then counts decisions to retry. Cancellation during the wait can prevent a resend. Internal HTTP retries are separate from task reruns and model rounds.

## Review a representative selection

Generate one HTML report per versioned suite from its registry and saved runs. The report includes every selected task and each archived attempt with the chosen model. It links the original score, verifier logs, trace, token usage, and submissions. A scored zero and an incomplete attempt have separate statuses. Use `--reasoning-effort` to restrict the report to the exact recorded effort. Use repeated `--task` arguments when the registry also contains tasks outside the selection.

```bash
python3 talos-bench/report.py output/terminal-bench-2.0/index.html \
  talos-bench/suites/terminal-bench/registry-2.0.json \
  --model openai:gpt-6.1-terra --reasoning-effort light
```

Save launcher output for a task in `<task>.log` beside the report to retain evidence when a run fails before writing a summary. Pass `--notes` with a JSON object keyed by `<run-directory-name>/<task>` for an attempt diagnosis or by `<task>` for a case note. Regenerate the report after retries.

The benchmark product build bundles ripgrep 15.1.0 for native ARM64 and AMD64 Linux. The benchmark runtime puts the product's `bin/rg` on the Agent's PATH and records `ripgrep-version.txt` before running Talos. When supplying a product built elsewhere, include a native `bin/rg` to give the Agent the same search tool. For local prompt comparisons, use two copies of the same product.

An optional product file `workspace-AGENTS.md` seeds the temporary Talos workspace through the existing bootstrap mechanism. It is recorded beside the Agent logs. Keep the pinned task instruction, task environment, and verifier unchanged across groups.

## Run FrontierHarness v1.1

Use `frontier-harness@1.1.0` in the model command. Harbor resolves the published Hub dataset by its pinned content digest. The launcher lists 62 selectable IDs. Set `TALOS_BENCH_TASKS` to a comma-separated list of full IDs from the selectable entries. The default is `terminal-bench/log-summary-date-ranges` and records the official verifier result, `/app/summary.csv`, and ATIF for model runs. On ARM64 the launcher builds the upstream Dockerfiles natively. The local summary records the dataset ref, task digest, platform, and paths to the original Harbor outputs.

For a verifier with a published AMD64-only image, the ARM64 launcher verifies the task digest and removes its verifier image pointer in a disposable copy so Harbor builds the original verifier Dockerfile. Each `provenance/<task>.json` file links the adaptation to its upstream task. The two guest workloads use native AMD64 Docker, including the Runta backend. Use the [batch guide](../../talos-bench/suites/frontier-harness/README.md) for cases that need prepared native images.

Check that Harbor resolves the pinned dataset and default task without starting a trial or pulling an image.

```bash
dataset=$(uv run --python 3.12 python talos-bench/summarize.py dataset-ref \
  talos-bench/suites/frontier-harness/registry.json)
uvx --from harbor==0.25.0 harbor run --dataset "$dataset" --env docker \
  --include-task-name terminal-bench/log-summary-date-ranges --agent oracle --dry-run --yes
```

The dry run checks dataset resolution. Full local execution also requires a native environment and verifier for each task. Keep the declared task selection and report any cases that need a different host.

Use `--backend local` to select the default Docker backend. Select `--backend runta` to execute the registered FrontierHarness v1.1 cases with an OpenAI catalog model on a prepared Runta runtime:

```bash
export TALOS_BENCH_RUNTA_CHECKPOINT=my-frontier-checkpoint
export TALOS_BENCH_RUNTA_SECRET=my-openai-secret
unset TALOS_BENCH_CONFIG TALOS_BENCH_DOTENV TALOS_BENCH_PRODUCT
TALOS_BENCH_MODEL=openai:gpt-6.1-sol \
TALOS_BENCH_TASKS=terminal-bench/regex-log \
./talos-bench/run.sh frontier-harness@1.1.0 --backend runta --reasoning-effort medium
```

Authenticate the Runta CLI and put it on `PATH`, or set `TALOS_BENCH_RUNTA_CLI` to its executable path. The Runta secret must already contain the OpenAI provider key. The launcher uses the checkpoint's model catalog and a scoped authorization stub. Local model files and product paths apply to the local backend.

Prepare the checkpoint on a native Linux AMD64 Runta runtime with systemd, Docker Buildx, and uv. Put the Talos checkout at `/work/talos-pilot/source` and run `./talos-bench/install.sh HEAD /work/talos-pilot/install` from that checkout. The installed product must match the checkpoint checkout's revision. The invocation uses that product and overlays the calling checkout's tracked Bench files, including any working-tree edits. Put the Runta CA Compose override at `/work/talos-pilot/runta-ca.yaml`. Mount the host CA bundle read-only for the main task container and set the container's `SSL_CERT_FILE` to that mount. Save a cold checkpoint before any jobs run.

The launcher restores a fresh runtime and runs the selected task and attempt set with the shared Docker launcher. It downloads the original job and evidence to `output/talos-bench/run.*/`. Each cloud invocation creates a fresh result directory. If the job archive cannot be retrieved or verified, use the printed Runta runtime name to inspect the retained job.

Keep every recovery attempt in a new batch directory. Reports must show each task’s original outcome, additional model rerun count, total attempts and first passing attempt; count verifier-only regrades separately. Give each repaired case a concise cause → fix note with the tested revision. Distinguish an unchanged rerun from a run using a fix, and preserve every original outcome.

For direct Harbor use, run `./talos-bench/install.sh <Talos-revision> <install-dir>`. This builds the pinned Talos product and installs the current benchmark adapter. Export `TALOS_BENCH_PRODUCT=<install-dir>/product`, add `<install-dir>/adapter` to `PYTHONPATH`, and export the provider key. Run `uvx --with PyYAML==6.0.3 --from harbor==0.25.0 harbor run -a harbor_talos:AtifTalosAgent -m openai/gpt-6.1-terra` with the target dataset and environment options. Harbor 0.22.0 uses the same adapter and selects its own ATIF schema version. The installer includes the native Compose file for local rebuilds.

## Run the representative cases

Export the model settings from [Run with a configured model](#run-with-a-configured-model) in the same shell. Each command uses the suite's pinned default case. Each invocation creates its own job, verifier result, and summary. The [suite reference](../../talos-bench/README.md) documents task selection, host requirements, sandbox coverage, and official submission routes.

```bash
./talos-bench/run.sh frontier-harness@1.1.0
./talos-bench/run.sh terminal-bench@4.1.0
./talos-bench/run.sh terminal-bench@2.0
./talos-bench/run.sh swe-bench-lite@1.0
./talos-bench/run.sh deep-swe@1.1
./talos-bench/run.sh mcp-atlas@2.0.0
```

The MCP-Atlas smoke case can use a configured agent with Harbor's provider-only network policy. Its independent verifier uses the controlled claim judge. Inspect `summary.json` and the original runner logs after each run before increasing the selection.

## Check runner compatibility

Run the compatibility checks locally on native ARM64 Docker. Set the product
directory created by `talos-bench/install.sh` to reuse the same build for each
check. The fixtures use loopback models and preserve the pinned verifier. The
DeepSWE fixture commits a placeholder file so Pier exercises its patch
collection and original verifier. Its expected task reward is zero.

```bash
unset TALOS_BENCH_MODEL TALOS_BENCH_CONFIG TALOS_BENCH_DOTENV TALOS_BENCH_TASKS
export TALOS_BENCH_PRODUCT=/path/to/install/product
TALOS_BENCH_HARBOR_VERSION=0.22.0 ./talos-bench/run.sh terminal-bench@4.1.0
./talos-bench/run.sh terminal-bench@4.1.0
./talos-bench/run.sh terminal-bench@2.0
./talos-bench/run.sh deep-swe@1.1
./talos-bench/run.sh swe-bench-lite@1.0
./talos-bench/run.sh mcp-atlas@2.0.0
TALOS_BENCH_ORACLE=1 ./talos-bench/run.sh frontier-harness@1.1.0
```

## Benchmark Browser MCP

Use `miniwob@1.1` with a native packaged Talos browser image. Harbor collects the upstream episode outcome and the ATIF trace of Talos's built-in Browser MCP actions. The [MiniWoB++ suite guide](../../talos-bench/suites/miniwob/README.md) covers the 125 registered environments, image selection, repeatable seeds and per-model reports.

## Repeat and compare runs

```bash
TALOS_BENCH_REPEATS=3 ./talos-bench/run.sh
```

The runner builds Talos once in agent modes. Each task runs the requested number of attempts in fresh containers, sequentially. Each invocation creates an independent directory under `output/talos-bench/run.*`. The 4.1 job directory is `terminal-bench-4.1.0`. The 2.0 regression still uses `regex-log` for the single task of that name.

| File | Evidence |
| --- | --- |
| `build.json` | Checkout revision and dirty state, product source, binary SHA-256, platform, and requested tasks and attempts in agent modes |
| `summary.json` | Suite version, per-task score and attempt count, per-attempt elapsed time, token usage, observed model and tool timings, and links to raw evidence |
| `<job>/result.json` and runner metadata | Original job result and resolved inputs |
| `provenance/<task>.json` | DeepSWE source revision, local ARM64 image IDs and verifier file hashes |
| `<job>/<trial>/agent/talos-trajectory.json` | Existing `trajectory/readTurn` response with all pages collected, including model requests, attempts, TTFT, usage, and outcomes |
| `<job>/<trial>/agent/talos-transcript.json` | Canonical message blocks collected for Terminal-Bench 4.1's ATIF export |
| `<job>/<trial>/agent/trajectory.json` | Terminal-Bench 4.1 ATIF with user and assistant messages, recorded reasoning, tool arguments and observations, and total token usage |
| `<job>/<trial>/agent/` and `<job>/<trial>/verifier/` | CLI stdout, stderr with cumulative usage and tool and compaction metrics, duration, native architecture, verifier logs and reward |
| `<job>/<trial>/artifacts/` and `artifacts/manifest.json` | Host copies of the submissions and the runner's collection status |

The per-trial `rootCacheRead` record carries the root Turn coverage and reconciliation status. The verified cache-read share divides provider-reported cached reads by total input tokens, including cache reads and writes. It is weighted by input tokens. A call with an explicit zero contributes its full input to the denominator. Missing counters, truncated trajectory records, or inconsistent totals leave the share unavailable and retain the observed counts for inspection. When delegation capture is available, `workloadUsage` reports observed descendant totals and coverage. A combined priced estimate requires all calls to reconcile with finalized child Turns as well as the root Turn. Older snapshots that contain only input and cache-read evidence can produce a descendant estimate from the reported totals. The summary labels that estimate separately and leaves the combined cost unavailable. These estimates cover Agent Turns. Separate auxiliary requests and external judge charges have their own accounting.

Compare the same task digest, model settings, and platform across builds. CLI wall time includes startup, trajectory export, and shutdown. Observed model and tool times sum individual measured durations. Talos limits detailed request measurements to the first 128 model requests. Use Turn totals for overall token usage and model rounds.

A verifier score of zero is valid. The summary sets `complete: true` only when scoring and all required trace and submission checks pass. Inspect the runner and verifier logs when the command exits nonzero. If a Turn pauses or is interrupted, inspect stderr for the measurements received so far.

To regenerate the summary for a saved `regex-log` job, run `uv run --python 3.12 python talos-bench/summarize.py summary <run-dir> real <attempts>`. For other selections, pass the task list and job name. For a Pier job, also pass `--suite deep-swe --runner pier`.

## Recheck a saved submission

Pass the original Harbor trial directory. The command checks the pinned task digest, restores the archived files into a fresh native container, and runs the upstream verifier. The recheck result and its link to the original trial are written under `output/talos-bench/recheck.*`. This command is for registered cases whose verifier can score the saved files in a fresh task environment.

```bash
uv run --python 3.12 --with harbor==0.25.0 python talos-bench/recheck.py \
  output/talos-bench/run.example/regex-log/regex-log__example
```

## Retain results for upstream evaluation

Keep the entire run directory, including the original runner job, verifier logs, task outputs, and build and provenance files. The local `complete` flag applies to the selected trials. Official eligibility also depends on the published dataset and coverage, accepted execution settings, and the suite's required artifacts. Check those requirements before running a full set or preparing a submission.

See the [suite reference](../../talos-bench/README.md) for current commands and published submission routes. The [phase two design](../../spec/feat-059-benchmark-execution/design.md) defines execution and coverage. The [artifact contract](../../spec/feat-058-portable-benchmark-runs/design.md) defines per-suite exports and eligibility checks.

## Runta Arena custom agent

Upload `talos-bench/runtime/arena_agent.py` as the only Python file in an unlisted
GitHub Gist. Select the custom harness in Runta Evaluations and supply the Gist
revision link, or paste its source in deployments that expose an inline editor.
The entry point exports `CustomAgent` and uses Harbor's supplied model connection.
It supports OpenAI Chat Completions and Responses, including explicit endpoint,
credential-variable and model overrides. Supported reasoning efforts are `low`,
`medium`, `high`, `xhigh` and `max`; `low` maps to Talos's `light` domain value.
The Arena entry defaults to `max` and sends a 131,072-token output limit through
the selected protocol's request parameters.

The adapter pins Talos release `v0.1.19`, its product SHA-256 values and the Bench
runtime files from the corresponding source commit. It downloads and verifies the native Linux
product inside the task during setup. Small runtime files and configuration are
uploaded through Harbor's environment interface.
The task environment must be compatible with that release's native Linux binary.
The pinned Linux product supports Debian 12's glibc 2.36 baseline; its native and
container acceptance matrix is documented in the
[release workflow](../development/RELEASE.md). The adapter records
installation output and release provenance in the agent logs.
No build is performed during a trial. Existing Bench code executes the CLI,
parses usage and exports the recorded transcript to ATIF.

Start with one smoke task and one attempt using an API-key provider. Inspect the
raw verifier outcome, `trajectory.json`, `talos-run.json`, stdout/stderr and
`arena-provenance.json` before increasing the task selection. A successful smoke
validates the integration. A qualifying Arena run follows the Arena's current
participation rules and has its own public evaluation link.

The secretless PR harness runs `python talos-bench/runtime/arena_smoke.py` with
Harbor 0.25.0 and Docker. It exercises the actual fixed release, installation,
model connection, default `max` effort, the output limit, a Bash tool call, usage
and ATIF through Harbor's
Docker environment. No Rust build or paid model is used. Evidence defaults to a
new directory under `output/arena/hosted-smoke/`; an explicit `--output` must name
a new directory. The fixed-release smoke uses Debian 12. Portable release acceptance separately exercises the
same new package in the minimum and newer Linux runtime environments.
