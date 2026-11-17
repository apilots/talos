# MiniWoB++ browser automation

This suite lists all 125 environments registered in MiniWoB++ v1.1. Select an environment by its page name using `TALOS_BENCH_TASKS`. Harbor runs the selected native Docker task in Chromium through Talos Browser MCP and collects its original episode result, ATIF and Talos diagnostics. See the [browser benchmark specification](../../../spec/feat-060-browser-automation-benchmarks/design.md) and the [MiniWoB++ release](https://github.com/Farama-Foundation/miniwob-plusplus/releases/tag/v1.1).

Use a locally available native Talos Server or Worker image with the packaged Chromium, Browser runtime and official MCP snapshot. Build one with `just docker-worker-build` when needed. Bench uses this image for the browser resources and runs the current or explicitly provided Talos CLI product in it.

```bash
export TALOS_BENCH_BROWSER_IMAGE=talos-worker:local
./talos-bench/run.sh miniwob@1.1

TALOS_BENCH_MODEL=openai:gpt-6.1-terra \
TALOS_BENCH_CONFIG=/path/to/talos/conf/talos.yaml \
TALOS_BENCH_DOTENV=/path/to/talos/.env \
TALOS_BENCH_TASKS=click-test-2,click-button-sequence,enter-text,choose-list,email-inbox-reply \
./talos-bench/run.sh miniwob@1.1 --reasoning-effort light
```

The default controlled case performs its browser operations through the same product tools as a configured model. The registry pins the upstream release and seed. Each invocation starts a fresh browser profile, and repeated attempts use the same seed for comparison. The original page controls its own deadline. The controller starts the episode with the upstream test mode and reads its reward through private DevTools. The controller stays outside the tool catalog. The task uses the product's Browser MCP for interaction, and the verifier checks the recorded tool calls.

Harbor records success as 1 when the episode completes with upstream raw reward 1 and the Talos trace uses Browser MCP for the task. An unsuccessful completed episode records 0. The raw and time-adjusted upstream rewards remain in `episode.json`. Missing episode evidence or a failed CLI prevents a valid score.

Generate the model report from saved runs. The profile filter uses the reasoning effort recorded in each trial.

```bash
python3 talos-bench/report.py output/miniwob/index.html talos-bench/suites/miniwob/registry.json \
  --model openai:gpt-6.1-terra --reasoning-effort light \
  --task click-test-2 --task click-button-sequence --task enter-text \
  --task choose-list --task email-inbox-reply
```
