# Local Agent benchmarks

Run a local benchmark with a native ARM64 Docker daemon, Docker Buildx, and uv.
Run the commands from the Talos repository root.

```bash
./talos-bench/run.sh --list
./talos-bench/run.sh terminal-bench
./talos-bench/run.sh terminal-bench@2.0
./talos-bench/run.sh swe-bench-lite
./talos-bench/run.sh mcp-atlas
TALOS_BENCH_ORACLE=1 ./talos-bench/run.sh terminal-bench@4.1.0
TALOS_BENCH_ORACLE=1 ./talos-bench/run.sh deep-swe
```

The first four suite commands run controlled Talos cases without model credentials. Terminal-Bench defaults to 4.1. Its fixture produces placeholder SQL with an expected reward of 0.0. Oracle mode runs the upstream reference solution and skips the Talos build and model setup. Use the Oracle command for DeepSWE without model credentials.

To reuse a build, set `TALOS_BENCH_PRODUCT` to an ARM64 TUI product directory containing `bin/talos`, `bin/talos-linux-sandbox-launcher`, `conf/talos.yaml`, and `conf/migrations/sqlite/`. Build from the current checkout for the trajectory and MCP import commands used by the suites.

## Run with a configured model

Select a catalog key and point the runner at an existing Talos config and dotenv. The provider's API key must use a `${TALOS_...}` reference in that config.

```bash
TALOS_BENCH_MODEL=deepseek:deepseek-v4-flash \
TALOS_BENCH_CONFIG=/path/to/talos/conf/talos.yaml \
TALOS_BENCH_DOTENV=/path/to/talos/.env \
./talos-bench/run.sh terminal-bench
```

The runner transfers the selected provider settings and its single dotenv assignment to the isolated task container. Talos reads them through its usual configuration loader. Use the summary for score and total job time, and the trial's `agent/talos-run.json` and `agent/talos.stderr.txt` for CLI duration and token usage. Configured models are supported for Terminal-Bench, SWE-bench Lite, and DeepSWE. The bundled MCP-Atlas case uses the controlled provider and judge.

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

To regenerate the summary for a saved `regex-log` job, run `python3 talos-bench/summarize.py summary <run-dir> real <attempts>`. For other selections, pass the task list and job name. For a Pier job, also pass `--suite deep-swe --runner pier`.

## Recheck a saved submission

Pass the original Harbor trial directory. The command checks the pinned task digest, restores the archived files into a fresh native ARM64 container, and runs the upstream verifier. The recheck result and its link to the original trial are written under `output/talos-bench/recheck.*`. This command is for registered cases whose verifier can score the saved files in a fresh task environment.

```bash
uv run --python 3.12 --with harbor==0.25.0 python talos-bench/recheck.py \
  output/talos-bench/run.example/regex-log/regex-log__example
```

See the [suite reference](../../talos-bench/README.md) for task selection, image recipes, and official submission requirements.
