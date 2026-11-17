# Phase two benchmark execution

Status: active

## Background and goals

[Issue #147](https://github.com/apilots/talos/issues/147) extends the [first benchmark implementation](../feat-055-local-agent-benchmarks/design.md) to native ARM64 and AMD64 Docker, standard model selection, complete usage, and upstream runner compatibility. The [artifact contract](../feat-058-portable-benchmark-runs/design.md) defines the upstream outputs and publication requirements. This specification defines how to select and execute those runs locally.

Keep one Talos CLI invocation and one adapter per runner. Keep Agent Kernel behavior, permission resolution, and sandbox policy outside this work. Keep actual leaderboard publication outside this work.

## Inputs and execution

- The existing launcher selects a versioned suite and its task set. Each suite owns an immutable upstream source and inventories its pinned public task IDs. The task selection override accepts registered case names. Platform, image, service, and dependency failures are reported by the task preparation or runner after launch. The local Docker backend is the default. FrontierHarness v1.1 also supports a Runta backend for registered tasks, using a prepared native AMD64 checkpoint and the shared launcher inside the restored runtime. Local regression cases remain available.
- Resolve the Docker daemon platform once. Accept a matching explicit platform and fail on an architecture mismatch. Select native images or build the upstream recipes natively. Record the selected image and platform. When required images, dependencies, or GPUs are unavailable, report which cases need a different host or upstream environment.
- Support the runner's `-m provider/model` and standard provider environment variables alongside the existing Talos config and dotenv path. Use Talos's catalog and provider settings. The launcher can override the selected model's supported reasoning effort for a run. New local OpenAI runs use the Responses provider and `openai:gpt-6.1-terra`. Record the actual model and effort for each attempt. The Runta backend uses a scoped secret stub for OpenAI authentication. Keep credentials confined to the runner and ephemeral Talos runtime. An adapter loaded by the runner also checks the task architecture against the Talos ELF binary.
- Retain Harbor 0.25 and Pier 0.3.1 and verify the adapter under FrontierHarness's documented Harbor 0.22. Keep version differences at the adapter or invocation boundary. Support direct upstream invocation with a Talos product installed from a pinned revision and the adapter on `PYTHONPATH`.

## Suite selections and acceptance

| Suite | Initial acceptance | Local evaluation selection |
| --- | --- | --- |
| FrontierHarness v1.1 | Run `terminal-bench/log-summary-date-ranges` from the pinned Hub dataset, first with Oracle and then with Talos. | Account for all 62 Hub tasks and run eligible cases on native Docker. |
| Terminal-Bench 4.1 and 2.0 | Run `production-planning` and `regex-log` with Talos and the upstream verifier. Retain ATIF for 4.1. | Independent 66-task and 89-task catalogs. A selected task reports its own environment requirements during preparation or execution. |
| SWE-bench Lite | Run `sympy__sympy-20590`. Retain the actual Git patch, harness logs, and verifier outcome. | All 300 test instances. Run the selected task with the Docker daemon's native platform. |
| DeepSWE | Run `python-statemachine-state-data-scoping` through Pier with the task egress policy and independent verifier. | All 113 pinned tasks with unchanged graders and egress rules. |
| MCP-Atlas | Run `barber-shop-first-word` through Talos and the upstream MCP sandbox and claim scorer. Pin the judge separately from the agent. | All 500 public IDs in the catalog. Run prepared public cases with their scorer inputs and compatible sandbox servers. |
| MiniWoB++ | Run `click-test-2` through Browser MCP with the upstream episode reward. | All 125 registered environments from v1.1. |

## Run evidence

Use the original job, trial, verifier, and artifact files as the basis of the exports. Keep the Talos summary as an index across those files. Root Turn usage includes input, output, cache read and cache write. A cache-read percentage requires the retained call records to cover the input total and match the CLI counters. Keep observed descendant usage separate and retain its coverage. Report combined Agent Turn usage only when all started Turns can be reconciled. A cost estimate requires explicit counters and the corresponding model price schedule. Preserve the existing per-request trajectory and use a usage sidecar where the runner cannot represent cache write. Keep a valid zero score and distinguish infrastructure failures. The task digest and provenance describe the input, and full-selection coverage and export validation determine eligibility under the artifact contract.

Evaluate the pinned local selections on native ARM64 with the configured model. Generate one HTML report per suite from the recorded runs. The report lists all selected tasks, scores and attempts, with links to raw evidence and verified diagnoses. Count a task only when its original verifier produced a score and the required trajectory and submissions are present. Keep controlled runs outside this selection. Official leaderboard profiles and their coverage requirements are documented in the artifact contract.

Record the upstream snapshot, matched tasks and scoring unit for any leaderboard comparison. A partial cohort can support a score comparison on those tasks. Official submissions declare their model and configuration and satisfy the leaderboard's environment, coverage and trial policy. Retain earlier attempts separately when reporting an improved result.

## Verification

Run the FrontierHarness smoke first on native ARM64 Docker. Check its exact Hub source, verifier reward, declared output, ATIF, and usage. Verify catalog membership against each pinned upstream snapshot and preflight selected tasks before execution. Run the benchmark runner compatibility checks locally on native ARM64, and record host-dependent gaps by case. Validate result artifacts with upstream tools and keep actual logs and results under ignored `output/`.
