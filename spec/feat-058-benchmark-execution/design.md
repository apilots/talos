# Phase two benchmark execution

Status: active

## Background and goals

[Issue #147](https://github.com/apilots/talos/issues/147) extends the [first benchmark implementation](../feat-055-local-agent-benchmarks/design.md) to native ARM64 and AMD64 Docker, standard model selection, complete usage, and upstream runner compatibility. The [artifact contract](../feat-057-portable-benchmark-runs/design.md) defines the upstream outputs and publication requirements. This specification defines how to select and execute those runs locally.

Keep one Talos CLI invocation and one adapter per runner. Keep Agent Kernel behavior, permission resolution, and sandbox policy outside this work. Keep actual leaderboard publication outside this work.

## Inputs and execution

- The existing launcher selects a versioned suite and its task set. Each suite owns an immutable upstream source. Use the existing task selection override for named cases. FrontierHarness accounts for its complete pinned split and reports any unsupported tasks individually. Each other versioned suite has five pinned real cases for local evaluation. Local regression cases remain available.
- Resolve the Docker daemon platform once. Accept a matching explicit platform and fail on an architecture mismatch. Select native images or build the upstream recipes natively. Record the selected image and platform. When required images, dependencies, or GPUs are unavailable, report which cases need a different host or upstream environment.
- Support the runner's `-m provider/model` and standard provider environment variables alongside the existing Talos config and dotenv path. Use Talos's catalog and provider settings. New local OpenAI runs use the Responses provider and `openai:gpt-6.1-terra`. Record the actual model for each attempt, including historical Sol results. Keep credentials confined to the runner and ephemeral Talos runtime. An adapter loaded by the runner also checks the task architecture against the Talos ELF binary.
- Retain Harbor 0.25 and Pier 0.3.1 and verify the adapter under FrontierHarness's documented Harbor 0.22. Keep version differences at the adapter or invocation boundary. Support direct upstream invocation with a Talos product installed from a pinned revision and the adapter on `PYTHONPATH`.

## Suite selections and acceptance

| Suite | Initial acceptance | Local evaluation selection |
| --- | --- | --- |
| FrontierHarness v1.1 | Run `terminal-bench/log-summary-date-ranges` from the pinned Hub dataset, first with Oracle and then with Talos. | Account for all 62 Hub tasks and run eligible cases on native Docker. |
| Terminal-Bench 4.1 and 2.0 | Run `production-planning` and `regex-log` with Talos and the upstream verifier. Retain ATIF for 4.1. | Five real cases from each version's registry. |
| SWE-bench Lite | Run `sympy__sympy-20590`. Retain the actual Git patch, harness logs, and verifier outcome. | Five pinned real cases from the Lite split. |
| DeepSWE | Run `python-statemachine-state-data-scoping` through Pier with the task egress policy and independent verifier. | Five pinned real cases with unchanged graders and egress rules. |
| MCP-Atlas | Run `barber-shop-first-word` through Talos and the upstream MCP sandbox and claim scorer. Pin the judge separately from the agent. | Five pinned real cases from the public split, with their tools and claims. |

## Run evidence

Use the original job, trial, verifier, and artifact files as the basis of the exports. Keep the Talos summary as an index across those files. Per-attempt usage includes input, output, cache read and cache write. Preserve the existing per-request trajectory and use a usage sidecar where the runner cannot represent cache write. Keep a valid zero score and distinguish infrastructure failures. Report cost only for an explicit model price schedule. The task digest and provenance describe the input, and full-selection coverage and export validation determine eligibility under the artifact contract.

Evaluate the pinned local selections on native ARM64 with the configured model. Generate one HTML report per suite from the recorded runs. The report lists all selected tasks, scores and attempts, with links to raw evidence and verified diagnoses. Count a task only when its original verifier produced a score and the required trajectory and submissions are present. Keep controlled runs outside this selection. Official leaderboard profiles and their coverage requirements are documented in the artifact contract.

## Verification

Run the FrontierHarness smoke first on native ARM64 Docker. Check its exact Hub source, verifier reward, declared output, ATIF, and usage. Preflight the complete FrontierHarness inventory and the five-case selections before executing them. Record host-dependent gaps and validate native AMD64 separately on an AMD64 daemon. Validate result artifacts with upstream tools and keep actual logs and results under ignored `output/`.
