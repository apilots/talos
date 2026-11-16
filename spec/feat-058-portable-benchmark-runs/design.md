# Portable benchmark runs and official artifacts

Status: active

## Background

The [local benchmark harness](../feat-055-local-agent-benchmarks/design.md) runs registered tasks with upstream verifiers and retains evidence for Agent Kernel development. [Issue #147](https://github.com/apilots/talos/issues/147) extends that path to repeatable model-backed runs on native Docker and exact upstream task sets. The same Talos adapter must work in upstream runners when a published evaluation requires a different environment or dataset.

## Goals and boundaries

- Run a pinned selection independently in local native Docker. Use the same Talos build and model configuration contract to repeat it or to run the full upstream selection on a host that meets its requirements.
- Retain the artifacts that upstream scorers and submission tools consume. Document how to validate and prepare a submission for each suite with a published process.
- Keep local and official results tied to their resolved inputs and coverage. Publishing jobs, contacting maintainers, and submitting leaderboard entries require separate authorization.

## Current implementation

The launcher selects suite registries and delegates execution to Harbor or Pier. It retains runner jobs, verifier results, required submissions, and Talos diagnostics. Terminal-Bench 4.1 and the FrontierHarness v1.1 smoke path export Harbor-validated ATIF. SWE-bench Lite captures patches and inference trajectories for upstream scoring and packaging. MCP-Atlas can use a configured model with its controlled judge and exports the upstream CSV format. DeepSWE uses the upstream Pier verifier. The local summary gates on evidence for the requested trials. Full dataset execution and submission eligibility remain pending.

## Reproducible runs

- Use the existing suite registries for versioned local task selections. For an upstream selection, resolve the published dataset or task revision to an immutable digest. Record exact task IDs and attempts even when the runner accepts a filter. Refresh pins explicitly so old selections can be replayed.
- Resolve the Docker daemon's native architecture before preparing tasks. Use a compatible pinned image or build from the pinned upstream Dockerfile for that architecture. Record the resulting image identity and any changes to the upstream environment. Keep the task, verifier, and scoring rules intact. Report unsupported platform or resource requirements before launching the affected trials. Use a compatible host when a suite requires the canonical image or a GPU.
- Reuse the existing Talos CLI, Provider configuration, and runtime paths on local and official runners. Keep model and reasoning effort explicit. Record the suite and split, runner and verifier versions, task and image digests, build revision and binary hash, platform, attempt count, resource and timeout settings, model identity, and judge settings where applicable. Keep credentials and private configuration out of retained or packaged artifacts.

## Artifact and submission contract

Retain the original runner job and verifier outputs. Use the existing per-suite adapter or upstream tooling to produce any required native files from the captured run. Keep `summary.json`, detailed token and cache usage, and timing as Talos diagnostics. A valid zero score remains a completed result when its evidence is complete. Keep failed trials and classify them under the relevant upstream rules.

Before declaring an export eligible, check the exact dataset and coverage, attempt counts, image and execution settings, judge, and upstream artifact requirements. A local native rebuild or alternate judge is recorded as an environment difference. Eligibility requires the upstream rules to allow that difference. If required data is missing or the environment differs from what upstream accepts, identify the requirement and rerun the same adapter with compliant settings. Capture any required transcript during inference through Talos's explicit transcript option.

| Suite | Preserve and validate | Official path |
| --- | --- | --- |
| Terminal-Bench 4.1 | Harbor jobs and trials, verifier rewards and logs, task-declared outputs, and valid ATIF at the trial trajectory path. | Run the exact unmodified leaderboard dataset with default timeouts and resources and at least five trials per task. Upload publicly readable Harbor trials with ATIF for rewarded trials, then use the leaderboard CLI to prepare an entry for review. [Submission guide](https://github.com/harbor-framework/terminal-bench/blob/v4.1.0/leaderboard/SUBMIT.md) |
| Terminal-Bench 2.0 | Preserve the 2.0 dataset identity, Harbor trial, verifier results, and task outputs separately. | Apply the submission rules for the target version. Use the 4.1 profile for the 4.1 leaderboard. [Terminal-Bench run guide](https://www.tbench.ai/run) |
| SWE-bench Lite | Export `all_preds.jsonl`, per-instance patch, scorer report and test output, and inference-time trajectories. Use the official SWE-bench tooling to package the evaluation and verify the verdicts from saved logs. | Evaluate the full Lite split. The upstream submission flow uses a public artifact repository and a leaderboard entry pointing to it. [Submission instructions](https://github.com/SWE-bench/experiments#-leaderboard-participation) |
| DeepSWE | Preserve the Pier job, Git patches, trajectories, verifier logs and per-instance scores. Keep the agent egress policy in force. | The published run uses Pier on Modal. Confirm acceptance and the required bundle with the maintainers before an official run. [Run and submission guide](https://deepswe.datacurve.ai/run#submit-to-the-leaderboard) |
| MCP-Atlas | Export `task_id,raw_conversation_history,response` from recorded messages and tool calls. Retain `run_config.json`, sandbox and judge identity, scored CSV, and coverage statistics. Validate the CSV with the upstream claim scorer and exact claims. | The public harness covers 500 tasks. The published full leaderboard uses 1,000, including a private split. Treat public-split scores as such. The public docs provide no self-service submission procedure. [Harness](https://github.com/scaleapi/mcp-atlas#5-run-the-full-eval), [Leaderboard](https://labs.scale.com/leaderboard/mcp_atlas) |
| FrontierHarness v1.1 | Resolve the exact Hub dataset and retain native runner jobs and verifiers, task-declared outputs, provenance, and per-attempt usage. Keep infrastructure failures distinguishable from model failures. | The published comparison uses 62 pinned tasks and four scored attempts per task under the hosted model and runner settings. Validate a compatible Talos adapter against the upstream runner. A public submission procedure has yet to be verified. [Dataset](https://hub.harborframework.com/datasets/frontier-harness-eval/frontier-harness-eval), [Scoring](https://frontierharness.org/about/) |

## Impact and verification

Keep selection, preparation, and export with the owning suite under `talos-bench/`. Keep shared native platform detection, Talos invocation, and diagnostics in the existing launcher and runtime. Reuse the suite's official verifier and packaging tools. Document repeatable commands in the [benchmark guide](../../docs/reference/BENCHMARKS.md).

1. Run a representative controlled or Oracle trial per suite in native Docker. Verify the platform, upstream score, required native files, and recorded inputs. Confirm missing required files make that run incomplete.
2. Run the pinned local selection with a real model. Confirm the resolved selection is stable across repeats and retain per-trial scores, usage, durations, and outputs. For the external smoke, select `terminal-bench/log-summary-date-ranges` from `frontier-harness-eval/frontier-harness-eval@v1.1.0` at `sha256:9236400fc1a6619eb72a663445c580417be9bbbab32f816c7dff76d0a96ab24a`.
3. Run the upstream validator or scorer on the exported files and compare against the original verifier result. Use the official package and dry-run tools where available. Verify that partial coverage, changed settings, or a missing upstream route are reported independently of local completion.
4. Before an official run, confirm the exact dataset, allowed environment, coverage, and submission procedure. Where full coverage requires a different host, verify the same Talos adapter there and retain the upstream runner outputs.
