# Talos Agent benchmark suites

See the [local benchmark guide](../docs/reference/BENCHMARKS.md) for commands, model configuration, and run artifacts.

The [FrontierHarness v1.1 batch guide](suites/frontier-harness/README.md) covers the pinned full inventory, native ARM64 admission, per-task execution, and HTML review.

The [MiniWoB++ browser suite](suites/miniwob/README.md) runs seeded upstream browser tasks through the built-in Browser MCP and scores each original episode.

The suite selects its runner, task registry, container preparation and agent adapter. The shared entry point handles the Talos product, provider configuration, repetitions and result summaries. `runtime/` contains the Talos product Dockerfile and the code shared by the Harbor and Pier agent adapters. Each `suites/<suite>/` directory contains a registry and the runner-specific configuration. Running without a suite argument selects Terminal-Bench.

The default suites pin released frameworks and immutable task inputs so that separate runs can use the same grader. Check the upstream release before refreshing a pin. [Harbor](https://pypi.org/project/harbor/) · [Terminal-Bench](https://github.com/harbor-framework/terminal-bench/releases/latest) · [SWE-bench](https://pypi.org/project/swebench/) · [MCP-Atlas](https://github.com/scaleapi/mcp-atlas/blob/main/CHANGELOG.md) · [Pier](https://pypi.org/project/datacurve-pier/)

Terminal-Bench uses Harbor 0.25.0 and builds each upstream task Dockerfile natively using its Compose configuration. Both the 4.1 `production-planning` fixture and the 2.0 `regex-log` fixture use a deterministic loopback Chat Completions provider and run a Bash tool call before scoring. The secretless `05_cli/exec_unattended` case covers the product CLI contract in hosted CI.

## Choose a version and tasks

Use `./talos-bench/run.sh --list` to show pinned task IDs by suite version. Set `TALOS_BENCH_TASKS` to a comma-separated list of exact selectable IDs from one version. Omitting the variable runs the suite's default task. FrontierHarness and Terminal-Bench include inventory entries with a reason in parentheses.

## Terminal-Bench tasks

| Launcher selector | Registry | Pinned release | Selectable | Default case |
| --- | --- | --- | --- | --- |
| `terminal-bench` or `terminal-bench@4.1.0` | [registry.json](suites/terminal-bench/registry.json) | [Harbor Terminal-Bench 4.1.0](https://github.com/harbor-framework/terminal-bench/tree/4070d6cef56a7d20cb5dc98d5e10ada88a6b9440/tasks) | 63 of 66 | `production-planning` |
| `terminal-bench@2.0` | [registry-2.0.json](suites/terminal-bench/registry-2.0.json) | [Laude Terminal-Bench 2.0](https://github.com/laude-institute/terminal-bench-2/tree/69671fbaac6d67a7ef0dfec016cc38a64ef7a77c) | 81 of 89 | `regex-log` |

The two selectors resolve independent upstream datasets. Use the IDs and results for the chosen version. FrontierHarness v1.1 has its own dataset and task digests, including when an ID shares a name with a Terminal-Bench case.

The 4.1 fixture writes three placeholder SQL files through Talos and expects a reward of 0.0 from the independent verifier. The separate Oracle command verifies that the same task rewards the upstream solution. Talos runs in 4.1 export `agent/trajectory.json` in ATIF v1.8, validated by Harbor, beside the diagnostic trace.

Harbor collects the 4.1 task's declared submissions. The 2.0 task definitions at this pin have no artifact declarations. Talos supplements the registered submission paths for the existing 2.0 representative cases and retains Harbor's verifier evidence for the rest. Some tasks require a compatible native architecture, specific hardware, or upstream services.

Use the upstream oracle to validate a task's container and scoring before using model tokens. Oracle mode skips the Talos build and provider setup.

```bash
TALOS_BENCH_ORACLE=1 \
TALOS_BENCH_TASKS=log-summary-date-ranges,polyglot-c-py \
./talos-bench/run.sh terminal-bench@2.0
```

Use the same comma-separated task list with the [configured-model command](../docs/reference/BENCHMARKS.md#run-with-a-configured-model) to measure Talos. The 2.0 default is `regex-log`, which supports the controlled provider. Each task list applies to the selected version.

Before enrolling more cases:

1. Pin the upstream suite revision and keep each task's instruction, verifier and oracle together. Keep separate jobs and scores for suites with different runners or scoring contracts. FrontierHarness's [runner template](https://github.com/frontier-harness-eval/eval/blob/main/skills/frontierharness-eval/reference.md#runner-templates) uses Terminal-Bench 2.0 for its Harbor cases and Pier for DeepSWE.
2. Audit the base images, downloaded executables, toolchains, task working directory, resource requirements and network dependencies for native Linux ARM64. Run the official oracle and confirm its reward before enrolling the case for real models.
3. Record each case's verifier inputs in `submission_paths`. Include the directory when the verifier checks its contents. Harbor copies these paths to the host before scoring.
4. Compare a fixed set of tasks and a fixed model configuration with the same timeout and repeat policy. Retain the resolved task digests, Talos build, verifier output and trace. Keep infrastructure failures distinct from zero-reward attempts and reserve cases for checking whether improvements generalize.

## DeepSWE tasks

DeepSWE uses Pier 0.3.1. The registry indexes all 113 tasks at the pinned revision. Its default task is `python-statemachine-state-data-scoping`. The preparation step derives the native agent and verifier Dockerfiles from the pinned upstream recipes, substitutes their base images in a disposable task copy, and records both image IDs. Pier removes finished containers and retains these local images for future runs. The grader, test patch, Oracle solution, network policy, and patch collection come from the upstream task at its recorded revision.

To run Talos, use the [configured-model command](../docs/reference/BENCHMARKS.md#run-with-a-configured-model) with `deep-swe` as the suite. The Pier adapter takes the exact provider hostname from its HTTPS base URL. Pier filters agent egress to that host and grades the submitted Git patch in the separate verifier. The result includes DeepSWE's fail-to-pass and pass-to-pass scores. See the [DeepSWE run guide](https://deepswe.datacurve.ai/run) and the [FrontierHarness corpus pin](https://github.com/frontier-harness-eval/eval/blob/main/skills/frontierharness-eval/reference.md#corpus-pin-and-pi-control).

## SWE-bench Lite and MCP-Atlas

SWE-bench Lite registers all 300 test instances. The native ARM64 recipe covers the six SymPy 1.7 instances. Native AMD64 uses each instance's pinned upstream image. The runner collects the Git patch and sends it to the independent SWE-bench 5.0.2 verifier. See the [SWE-bench Lite suite](suites/swe-bench-lite/README.md).

MCP-Atlas indexes all 500 public task IDs. The adapter bundles five prepared public tasks with their tool menus and scorer inputs. Harbor collects the final answer and runs the upstream scorer in a separate verifier with a configured OpenAI judge. See the [MCP-Atlas suite](suites/mcp-atlas/README.md).

## Add a suite

Give the suite its own directory with a `registry.json` containing its name, version, task pins and submission paths. Its `suite.sh` declares the runner executable and package pin, default task, optional fixture task and agent arguments. The default fixture reward is 1.0. Its `prepare_suite` function prepares the selected tasks and sets `suite_args`. Harbor suites can reuse the Talos adapter's `capture=patch` or `capture=answer` for an independent verifier. Keep container recipes in the owning suite and map new runner result formats at the summary boundary.

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

Stdout streams the final assistant text. Stderr includes any waiting reason and a line prefixed with `TALOS_EXEC_METRICS ` containing the cumulative `AgentRunMetrics`, including token usage, or JSON `null` when no metrics arrived. Tool completion and compaction lines include their existing metrics. Pass `--trajectory /path/to/new.json` to save the canonical `trajectory/readTurn` response with all pages collected. The file contains JSON `null` if the turn has not settled. Add `--transcript /path/to/blocks.json` to save canonical user, assistant, and tool blocks separately. Use `talos plugin import-mcp /path/to/server.json` to import and enable one server through the existing Plugin service.
