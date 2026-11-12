# Local Talos Agent benchmarks

Status: verified

## Background and scope

Issue #140 needs a repeatable way to run Talos against external benchmarks and inspect execution and scoring. Keep the evaluation harness at repository root in `talos-bench/`, alongside product E2E checks.

## Design

- The unattended CLI reuses Session model selection, permission state, execution, and Timeline events. It reports the existing execution metrics on stderr and optionally writes the canonical `trajectory/readTurn` response. An explicitly requested transcript exports canonical message blocks to a separate file.
- One launcher selects a versioned suite, pins its runner and task inputs, and provisions native ARM64 task environments. Terminal-Bench 4.1 is the default. Explicit Terminal-Bench 2.0 selection preserves the regex-log baseline. Harbor runs Terminal-Bench, SWE-bench Lite, and MCP-Atlas. Pier runs DeepSWE.
- Each suite owns its case recipes and verifier. Terminal-Bench archives task outputs and exports Harbor ATIF from Talos events. SWE-bench Lite grades the archived worktree patch with the official scorer. MCP-Atlas calls the upstream sandbox through a Talos MCP plugin and sends the final answer to the upstream claim scorer. DeepSWE uses the pinned task verifier.
- Record the Talos build, task revision and digest, native architecture, submission, reward, execution metrics and trace in each ignored output directory. Mark a summary complete only after the runner and all required score, trajectory, and submission checks pass. Preserve a verifier reward of zero in the summary. Recheck saved Terminal-Bench submissions with the same pinned verifier.
- Controlled providers and the Atlas judge use local endpoints. Configured model runs reuse Talos's model catalog and dotenv loader. Benchmarks run in disposable containers and score independently of the Agent.

## Verification

Run the documented controlled and Oracle paths using native ARM64 Docker. Verify the archived patch or answer, verifier reward, and ATIF with Harbor's validator. Run the unattended CLI E2E check and scoped repository checks. Actual run records belong under `output/`.

## Boundaries

The local subset supports development and regression. Full benchmark coverage, real-provider comparisons, a real Atlas judge, and official submissions require separate runs. The harness does not alter the Agent Kernel.
