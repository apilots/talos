# Browser automation benchmarks

Status: active

## Goal

Measure Talos's ability to complete stateful browser workflows through its [built-in Browser MCP](../feat-047-browser-computer-desktop/design.md). Use the suite boundary, local launcher, artifact contracts and comparison procedures defined by the [benchmark execution design](../feat-059-benchmark-execution/design.md).

## Framework and coverage

MiniWoB++ v1.1 is the first browser suite. Its task pages provide seeded initial states, local hosting and programmatic rewards without an external account. The representative selection covers button selection, ordered actions, text entry, lists and an email reply. The suite pins the exact release commit and a seed per task. [MiniWoB++ release](https://github.com/Farama-Foundation/miniwob-plusplus/releases/tag/v1.1)

WebArena remains a candidate for broader workflows on realistic applications. Its original protocol needs the corresponding website images, initial data reset and evaluator access to each site. A separate integration must establish native hosting and repeatable reset before comparing results. [WebArena deployment](https://github.com/web-arena-x/webarena/blob/main/environment_docker/README.md)

## Execution and scoring

Build the suite image from a native packaged Talos Browser runtime, then run the usual unattended Talos CLI in Harbor under the product user with a new Talos and Chromium profile per trial. Install the selected CLI and current SQLite migrations from the same product. The built-in Browser Plugin remains responsible for all model browser actions.

The suite serves the pinned upstream HTML and scripts on loopback. A private controller finds the selected browser page through the managed Chromium DevTools session, seeds the upstream episode, starts it in test mode and reads the ending reward. The controller only initializes and observes the task. The original task clock, task logic and reward computation stay unchanged. The controller is outside the model's browser tool catalog. The verifier requires the recorded Browser MCP path for success and reports other task tools as protocol violations.

The local score uses 1 for a completed upstream raw success and 0 for a completed loss or an unresolved episode that the agent leaves after reaching it. Save both upstream raw and time-adjusted rewards, the pinned task and seed, the browser image and CLI build, the episode receipt, ATIF, canonical Browser MCP calls and outcome, duration and the shared token and cache evidence. Missing episode evidence or an unsettled Talos Turn is a failed execution. Preserve failures alongside scored outcomes.

## Verification

Run a controlled real Browser MCP sequence before the configured-model trials. Verify native ARM64, the managed browser version, the exact task manifest, the original episode outcome, Harbor scoring and the retained browser trace. Repeat against a fresh profile with the same seed. Run the five representative tasks with the chosen model profile and report completion, success and failure reasons. Compare matching tasks and protocols when relating a local result to a published reference.
