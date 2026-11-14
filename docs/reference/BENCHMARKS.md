# Local Agent benchmarks

Run a local benchmark with Docker, Docker Buildx, and uv. The launcher selects the daemon's native Linux platform. Image preparation follows each suite's pinned native recipes or published images.
Run the commands from the Talos repository root.

```bash
./talos-bench/run.sh --list
./talos-bench/run.sh terminal-bench
./talos-bench/run.sh terminal-bench@2.0
./talos-bench/run.sh swe-bench-lite
./talos-bench/run.sh mcp-atlas
TALOS_BENCH_ORACLE=1 ./talos-bench/run.sh terminal-bench@4.1.0
TALOS_BENCH_ORACLE=1 ./talos-bench/run.sh deep-swe
TALOS_BENCH_ORACLE=1 ./talos-bench/run.sh frontier-harness@1.1.0
```

The first four suite commands run controlled Talos cases without model credentials. Terminal-Bench defaults to 4.1. Its fixture produces placeholder SQL with an expected reward of 0.0. Oracle mode runs the upstream reference solution and skips the Talos build and model setup. Use the Oracle command for DeepSWE without model credentials.

To reuse a build, set `TALOS_BENCH_PRODUCT` to a native Linux TUI product directory containing `bin/talos`, `bin/talos-linux-sandbox-launcher`, `conf/talos.yaml`, and `conf/migrations/sqlite/`. Build from a revision that provides the trajectory and MCP import commands used by the suites.

## Run with a configured model

Select a catalog key and point the runner at an existing Talos config and dotenv. The provider's API key must use a `${TALOS_...}` reference in that config.

```bash
TALOS_BENCH_MODEL=openai:gpt-6.1-terra \
TALOS_BENCH_CONFIG=/path/to/talos/conf/talos.yaml \
TALOS_BENCH_DOTENV=/path/to/talos/.env \
./talos-bench/run.sh terminal-bench
```

The runner transfers the selected provider settings and its single dotenv assignment to the isolated task container. Talos reads them through its usual configuration loader. With the provider key already exported as `OPENAI_API_KEY`, omit the Talos config and dotenv and set `TALOS_BENCH_MODEL=openai:gpt-6.1-terra`. The adapter uses the shipped Talos catalog and its OpenAI Responses connection. `OPENAI_BASE_URL` can override the catalog endpoint.

Use `TALOS_BENCH_AGENT_ENV` with one `KEY=VALUE` per line for non-secret agent runtime options. The launcher passes these options through Harbor or Pier's `--agent-env` to the Talos process. Provider credentials use the configured dotenv path.

Set `TALOS_BENCH_AGENT_ENV=TALOS_BENCH_COLLABORATION_TOOLS=0` for a local single-Agent diagnostic. The isolated Talos config disables collaboration tools and records that setting in `agent/talos-run.json` and the summary. Normal benchmark runs enable collaboration tools. Suite reports retain diagnostic scores in the attempt history. Compare runs using the same setting.

Set `TALOS_BENCH_DELEGATION_WATCH=1` through `TALOS_BENCH_AGENT_ENV` to sample delegation and workspace progress every 30 seconds. For tasks without a Git worktree, set watched paths to enable automatic stopping. It stops after 12 minutes without a changed submission, changed Git workspace, or completed child Turn when there are at least 12 child sessions, depth four, and three additional child sessions over the last three minutes. `TALOS_BENCH_WATCH_PATHS` accepts colon-separated submission files and directories. The Bench writes `agent/talos-delegation.jsonl` and publishes the latest `agent/talos-delegation-events.json` at each sample and on exit. The event file records lifecycle, outcomes and tool timings without tool bodies. An external stop can leave up to one sampling interval absent from the event file. The summary records the stop separately from a numeric verifier result. The upstream grader and deadlines remain those of the pinned task.

Use the summary for score and job time, `agent/talos-run.json` for the launch profile and CLI duration, and `agent/usage.json` for cumulative input, output, cache-read, and cache-write tokens. The launch profile is saved before the CLI starts. It retains a null exit code and CLI duration if the runner terminates the process before exit is recorded. The summary records the runner's observed agent phase separately as `agentSeconds`. Per-request usage is in the Talos trajectory. Configured-model software benchmark runs retain the transcript with tool arguments and results for local attribution. With catalog prices, the summary estimates cost by pricing uncached input, cache reads, cache writes, and output separately. Missing cache counters contribute no measured cached tokens. The summary reports pass rate over scored attempts and uses the suite’s declared passing threshold, defaulting to reward 1.0. Aggregate cost is available once every requested attempt has priced usage.

## Review a representative selection

Generate one HTML report per versioned suite from its registry and saved runs. The report includes every selected task and each archived attempt with the chosen model. It links the original score, verifier logs, trace, token usage, and submissions. A scored zero and an incomplete attempt have separate statuses. Use repeated `--task` arguments when the registry also contains tasks outside the selection.

```bash
python3 talos-bench/report.py output/terminal-bench-2.0/index.html \
  talos-bench/suites/terminal-bench/registry-2.0.json --model openai:gpt-6.1-terra
```

Save launcher output for a task in `<task>.log` beside the report to retain evidence when a run fails before writing a summary. Pass `--notes` with a JSON object keyed by `<run-directory-name>/<task>` for an attempt diagnosis or by `<task>` for a case note. Regenerate the report after retries.

The benchmark product build bundles ripgrep 15.1.0 for native ARM64 and AMD64 Linux. The benchmark runtime puts the product's `bin/rg` on the Agent's PATH and records `ripgrep-version.txt` before running Talos. When supplying a product built elsewhere, include a native `bin/rg` to give the Agent the same search tool. For local prompt comparisons, use two copies of the same product.

An optional product file `workspace-AGENTS.md` seeds the temporary Talos workspace through the existing bootstrap mechanism. It is recorded beside the Agent logs. Keep the pinned task instruction, task environment, and verifier unchanged across groups.

## Run FrontierHarness v1.1

Use `frontier-harness@1.1.0` in the model command. Harbor resolves the published Hub dataset by its pinned content digest. The default `terminal-bench/log-summary-date-ranges` records the official verifier result, `/app/summary.csv`, and ATIF for model runs. Select the second smoke case with `TALOS_BENCH_TASKS=terminal-bench/music-harmony`. On ARM64 the launcher builds the upstream Dockerfiles natively for the registered cases. The local summary records the dataset ref, task digest, platform, and paths to the original Harbor outputs.

The published `music-harmony` verifier image is AMD64. On ARM64 the launcher verifies the published task digest, makes a disposable copy, and removes its verifier image pointer so Harbor builds the original verifier Dockerfile. `provenance/music-harmony.json` links the local adaptation to the original task. Use the published environment when an upstream comparison requires the exact task package.

Resolve the full pinned dataset without launching trials or pulling task images.

```bash
dataset=$(uv run --python 3.12 python talos-bench/summarize.py dataset-ref \
  talos-bench/suites/frontier-harness/registry.json)
uvx --from harbor==0.25.0 harbor run --dataset "$dataset" --env docker \
  --agent oracle --dry-run --yes
```

The dry run checks dataset resolution. Full local execution also requires a native environment and verifier for each task. Keep the declared task selection and report any cases that need a different host.

For direct Harbor use, run `./talos-bench/install.sh <Talos-revision> <install-dir>`. This builds the pinned Talos product and installs the current benchmark adapter. Export `TALOS_BENCH_PRODUCT=<install-dir>/product`, add `<install-dir>/adapter` to `PYTHONPATH`, and export the provider key. Run `uvx --with PyYAML==6.0.3 --from harbor==0.25.0 harbor run -a harbor_talos:AtifTalosAgent -m openai/gpt-6.1-terra` with the target dataset and environment options. Harbor 0.22.0 uses the same adapter and selects its own ATIF schema version. The installer includes the native Compose file for local rebuilds.

## Run the representative cases

Configure the model first. Each command uses the suite's pinned default case. Run them individually so each framework has its own job, verifier, and summary. The local evaluation accounts for the full pinned FrontierHarness v1.1 inventory and uses five pinned real cases each for Terminal-Bench 2.0, Terminal-Bench 4.1, SWE-bench Lite, DeepSWE, and MCP-Atlas. The official submission requirements are documented separately.

```bash
./talos-bench/run.sh frontier-harness@1.1.0
./talos-bench/run.sh terminal-bench@4.1.0
./talos-bench/run.sh terminal-bench@2.0
./talos-bench/run.sh swe-bench-lite@1.0
./talos-bench/run.sh deep-swe@1.1
./talos-bench/run.sh mcp-atlas@2.0.0
```

The MCP-Atlas smoke case can use a configured agent with Harbor's provider-only network policy. Its independent verifier uses the controlled claim judge. Inspect `summary.json` and the original runner logs after each run before increasing the selection.

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

See the [suite reference](../../talos-bench/README.md) for current commands and published submission routes. The [phase two design](../../spec/feat-058-benchmark-execution/design.md) defines execution and coverage. The [artifact contract](../../spec/feat-057-portable-benchmark-runs/design.md) defines per-suite exports and eligibility checks.
