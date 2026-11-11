# Talos Agent benchmarks

Run a local benchmark with a native ARM64 Docker daemon, Docker Buildx, and uv. From the Talos repository root:

```bash
./talos-bench/run.sh --list
./talos-bench/run.sh terminal-bench
./talos-bench/run.sh terminal-bench@2.0
./talos-bench/run.sh swe-bench-lite
./talos-bench/run.sh mcp-atlas
TALOS_BENCH_ORACLE=1 ./talos-bench/run.sh terminal-bench@4.1.0
TALOS_BENCH_ORACLE=1 ./talos-bench/run.sh deep-swe
```

The suite selects its runner, task registry, container preparation and agent adapter. The shared entry point handles the Talos product, provider configuration, repetitions and result summaries. `runtime/` contains the Talos product Dockerfile and the code shared by the Harbor and Pier agent adapters. Each `suites/<suite>/` directory contains a registry and the runner-specific configuration. Running without a suite argument selects Terminal-Bench.

The default suites pin released frameworks and immutable task inputs so that separate runs can use the same grader. Check the upstream release before refreshing a pin. [Harbor](https://pypi.org/project/harbor/) · [Terminal-Bench](https://github.com/harbor-framework/terminal-bench/releases/latest) · [SWE-bench](https://pypi.org/project/swebench/) · [MCP-Atlas](https://github.com/scaleapi/mcp-atlas/blob/main/CHANGELOG.md) · [Pier](https://pypi.org/project/datacurve-pier/)

Terminal-Bench uses Harbor 0.25.0 and builds each upstream task Dockerfile natively using its Compose configuration. Both the 4.1 `production-planning` fixture and the 2.0 `regex-log` fixture use a deterministic loopback Chat Completions provider and run a Bash tool call before scoring. The secretless `05_cli/exec_unattended` case covers the product CLI contract in hosted CI.

To reuse a build, set `TALOS_BENCH_PRODUCT` to an ARM64 TUI product directory containing `bin/talos`, `bin/talos-linux-sandbox-launcher`, `conf/talos.yaml`, and `conf/migrations/sqlite/`. Build from the current checkout for the trajectory and MCP import commands used by the suites.

## Run with a configured model

Select a catalog key and point the runner at an existing Talos config and dotenv. The provider's API key must use a `${TALOS_...}` reference in that config.

```bash
TALOS_BENCH_MODEL=deepseek:deepseek-v4-flash \
TALOS_BENCH_CONFIG=/path/to/talos/conf/talos.yaml \
TALOS_BENCH_DOTENV=/path/to/talos/.env \
./talos-bench/run.sh terminal-bench
```

The runner transfers the selected provider settings and its single dotenv assignment to the isolated task container. Talos reads them through its usual configuration loader. Use the summary for score and total job time, and the trial's `agent/talos-run.json` and `agent/talos.stderr.txt` for CLI duration and token usage. A verifier score of zero is a valid real-model result.

## Terminal-Bench tasks

The Terminal-Bench suite has versioned registries. `terminal-bench` and `terminal-bench@4.1.0` select the 4.1 source release and currently enroll `production-planning`. Harbor builds the 4.1 agent and separate verifier from the upstream Dockerfiles and transfers the three SQL submissions declared by that task. Use `terminal-bench@2.0` to reproduce the earlier 2.0 cases.

The 4.1 fixture writes three placeholder SQL files through Talos and expects a reward of 0.0 from the independent verifier. The separate Oracle command verifies that the same task rewards the upstream solution. Talos runs in 4.1 export `agent/trajectory.json` in ATIF v1.8, validated by Harbor, beside the diagnostic trace.

| Version | Task | Coverage |
| --- | --- | --- |
| 2.0 | `regex-log` | Extract a matching log line |
| 2.0 | `log-summary-date-ranges` | Aggregate timestamped logs across date ranges |
| 2.0 | `polyglot-c-py` | Produce a program that runs as both C and Python |
| 2.0 | `multi-source-data-merger` | Merge structured data into Parquet and report conflicts |
| 4.1.0 | `production-planning` | Produce a rolling manufacturing plan and SQL writebacks |

Use the upstream oracle to validate a task's container and scoring before using model tokens. Oracle mode skips the Talos build and provider setup.

```bash
TALOS_BENCH_ORACLE=1 \
TALOS_BENCH_TASKS=log-summary-date-ranges,polyglot-c-py \
./talos-bench/run.sh terminal-bench@2.0
```

Use the same comma-separated task list with the configured model command to measure Talos. The 2.0 default is `regex-log`, which supports the controlled provider. Each task list applies to the selected version.

Before enrolling more cases:

1. Pin the upstream suite revision and keep each task's instruction, verifier and oracle together. Keep separate jobs and scores for suites with different runners or scoring contracts. FrontierHarness's [runner template](https://github.com/frontier-harness-eval/eval/blob/main/skills/frontierharness-eval/reference.md#runner-templates) uses Terminal-Bench 2.0 for its Harbor cases and Pier for DeepSWE.
2. Audit the base images, downloaded executables, toolchains, task working directory, resource requirements and network dependencies for native Linux ARM64. Run the official oracle and confirm its reward before enrolling the case for real models.
3. Record each case's verifier inputs in `submission_paths`. Include the directory when the verifier checks its contents. Harbor copies these paths to the host before scoring.
4. Compare a fixed set of tasks and a fixed model configuration with the same timeout and repeat policy. Retain the resolved task digests, Talos build, verifier output and trace. Keep infrastructure failures distinct from zero-reward attempts and reserve cases for checking whether improvements generalize.

## DeepSWE tasks

DeepSWE uses Pier 0.3.1. Its default task is `python-statemachine-state-data-scoping`. Select `anko-typed-variable-bindings` with `TALOS_BENCH_TASKS` to exercise a Go repository. The native agent and separate verifier Dockerfiles are under `suites/deep-swe/cases/`. The preparation step checks their build steps against the pinned upstream recipes, substitutes local ARM64 image references in a disposable task copy, and records both image IDs. Pier removes finished containers and retains these local images for future runs. The grader, test patch, Oracle solution, network policy, and patch collection come from the upstream task at its recorded revision.

To run Talos, use the configured-model command with `deep-swe` as the suite. The Pier adapter takes the exact provider hostname from its HTTPS base URL. Pier filters agent egress to that host and grades the submitted Git patch in the separate verifier. The result includes DeepSWE's fail-to-pass and pass-to-pass scores. See the [DeepSWE run guide](https://deepswe.datacurve.ai/run) and the [FrontierHarness corpus pin](https://github.com/frontier-harness-eval/eval/blob/main/skills/frontierharness-eval/reference.md#corpus-pin-and-pi-control).

## SWE-bench Lite and MCP-Atlas

SWE-bench Lite starts Talos in a checkout of the pinned base commit. It collects the resulting Git patch and sends it to the independent verifier, which runs the official SWE-bench 5.0.2 evaluation and report code. See the [SWE-bench Lite suite](suites/swe-bench-lite/README.md).

MCP-Atlas uses the claim scorer from Scale's 2.0.0 harness revision and the published 1.2.7 sandbox. A standard Talos MCP plugin forwards tool calls to that sandbox. Harbor collects the final answer and runs the scorer in a separate verifier with a loopback judge. The bundled CSV case uses the controlled model and judge without external credentials. See the [MCP-Atlas suite](suites/mcp-atlas/README.md).

## Add a suite

Give the suite its own directory with a `registry.json` containing its name, version, task pins and submission paths. Its `suite.sh` declares the runner executable and package pin, default task, optional fixture task and agent arguments. The default fixture reward is 1.0. Its `prepare_suite` function prepares the selected tasks and sets `suite_args`. Harbor suites can reuse the Talos adapter's `capture=patch` or `capture=answer` for an independent verifier. Keep container recipes in the owning suite and map new runner result formats at the summary boundary.

## Recheck a saved submission

Pass the original Harbor trial directory. The command checks the pinned task digest, restores the archived files into a fresh native ARM64 container, and runs the upstream verifier. The recheck result and its link to the original trial are written under `output/talos-bench/recheck.*`. This command is for registered cases whose verifier can score the saved files in a fresh task environment.

```bash
uv run --python 3.12 --with harbor==0.25.0 python talos-bench/recheck.py \
  output/talos-bench/run.example/regex-log/regex-log__example
```

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
| `<job>/<trial>/agent/talos-trajectory.json` | Existing Talos Turn trajectory metadata, including each model request and attempt, TTFT, usage, outcome, tool queue and execution timings, and compaction metrics |
| `<job>/<trial>/agent/trajectory.json` | Terminal-Bench 4.1 ATIF with user and assistant messages, recorded reasoning, tool arguments and observations, and total token usage |
| `<job>/<trial>/agent/` and `<job>/<trial>/verifier/` | CLI stdout, stderr, duration, native architecture, verifier logs and reward |
| `<job>/<trial>/artifacts/` and `artifacts/manifest.json` | Host copies of the submissions and the runner's collection status |

Compare the same task digest, model settings and platform across builds. CLI wall time includes startup, trajectory export and shutdown. Observed model and tool times sum individual measured durations. Talos limits detailed request measurements to the first 128 model requests. Use Turn totals for overall token usage and model rounds. The summary counts scored and traced trials independently and preserves missing rewards for infrastructure failures. Terminal trajectory metadata can be unavailable after an interrupted or waiting Turn. In that case the export retains measurements already seen on the Timeline. Terminal-Bench 4.1 requests completed transcript blocks explicitly. Historical `regex-log` jobs can be summarized with `python3 talos-bench/summarize.py summary <run-dir> real <attempts>`. For other selections, pass the task list and job name. For a Pier job, also pass `--suite deep-swe --runner pier`.

## Official results

Harbor can upload completed jobs to Harbor Hub. Uploads are private by default. Terminal-Bench's leaderboard guide specifies the exact published dataset, default resources and timeouts, full task coverage with at least five trials per task, and a publicly readable ATIF trajectory for each rewarded trial. The full 4.1 dataset requires a GPU-capable runner for some tasks. Check community eligibility with the maintainers before scheduling a leaderboard run. The local 4.1 task exercises ATIF export and scoring with the controlled provider. [Terminal-Bench run guide](https://www.tbench.ai/run) · [Leaderboard submission guide](https://github.com/harbor-framework/terminal-bench/blob/v4.1.0/leaderboard/SUBMIT.md)

DeepSWE's run guide directs new model or agent submissions to the maintainers. Its published scores use Pier on Modal. [DeepSWE leaderboard instructions](https://deepswe.datacurve.ai/run#submit-to-the-leaderboard)

SWE-bench Lite documents a submission through the `SWE-bench/experiments` repository. Its submission package includes predictions, execution logs, and per-instance trajectories hosted in a public artifact repository. [SWE-bench leaderboard instructions](https://github.com/SWE-bench/experiments#-leaderboard-participation)

Scale publishes the MCP-Atlas runner, scorer, and 500-task public split. Its leaderboard also reports results over the full 1,000 tasks, including the private split. Scale's public harness and leaderboard documentation describe local evaluation without a self-service leaderboard submission procedure. [MCP-Atlas harness](https://github.com/scaleapi/mcp-atlas) · [Scale leaderboard](https://labs.scale.com/leaderboard/mcp_atlas)

## Unattended Talos CLI

```bash
talos exec --session "$session_id" --model provider:model --dangerously-skip-permissions -- "$prompt"
```

Set `gateway.auth: false` in the isolated runtime config for anonymous operation. The flag saves Full Access on the Session before executing the turn. Invocations that reuse the Session without the flag use its saved permission mode. The flag leaves the Talos configuration file intact. Use it inside a disposable task environment.

| Exit code | Meaning |
| --- | --- |
| 0 | Turn completed |
| 1 | Execution failed or the stream closed before settlement |
| 2 | Invalid CLI arguments |
| 3 | Turn paused for input |

Stdout streams the final assistant text. Stderr includes any waiting reason and a line prefixed with `TALOS_EXEC_USAGE ` containing the final cumulative token counts, or JSON `null` when no metrics arrived. Cache reads and writes are separate optional counters. These counts describe the turn. Pass `--trajectory /path/to/new.json` to save execution measurements and terminal trajectory metadata. Add `--trajectory-include-content` to include canonical user, assistant, and tool blocks. Use `talos plugin import-mcp /path/to/server.json` to import and enable one server through the existing Plugin service.
