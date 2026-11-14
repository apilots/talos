# MCP-Atlas

Run the controlled case from the repository root with native Docker.

```bash
TALOS_BENCH_REPEATS=2 ./talos-bench/run.sh mcp-atlas@2.0.0
TALOS_BENCH_ORACLE=1 ./talos-bench/run.sh mcp-atlas
```

The suite version follows the upstream MCP-Atlas 2.0.0 changelog. Talos provides the agent loop and Harbor runs the trial. The suite pins the claim scorer at commit `e3dfef3` and the multi-architecture index of sandbox image `1.2.7`. [Upstream changelog](https://github.com/scaleapi/mcp-atlas/blob/e3dfef3ce93287c3d7b9e765c3a021f252eefbff/CHANGELOG.md) · [Sandbox image recipe](https://github.com/scaleapi/mcp-atlas/blob/e3dfef3ce93287c3d7b9e765c3a021f252eefbff/Makefile)

The case asks for the first word in the upstream `Barber Shop.csv` example. Harbor connects Talos to the sandbox on an internal Docker network. Talos imports the suite's MCP configuration through its plugin CLI. The bridge uses MCP Python SDK 2.3.0 to expose the sandbox's read-only filesystem tool over standard MCP stdio. The local fixture requests ToolSearch, calls the discovered tool, and extracts the answer from its returned CSV header. [MCP-Atlas example](https://github.com/scaleapi/mcp-atlas/tree/e3dfef3ce93287c3d7b9e765c3a021f252eefbff) · [MCP Python SDK release](https://pypi.org/project/mcp/2.3.0/)

Harbor passes `answer.txt` to a separate verifier. The pinned upstream claim scorer calls a loopback judge that checks the expected word. The run directory contains the Talos trajectory and answer, judge verdict, scored CSV, and Harbor reward. The oracle command verifies the same grading path with a fixed submission. The first build downloads the pinned sandbox, dependencies and scorer. The controlled agent fixture uses a loopback model and synthetic tokens. [Upstream scorer](https://github.com/scaleapi/mcp-atlas/blob/e3dfef3ce93287c3d7b9e765c3a021f252eefbff/services/scoring/score_claims.py)

For a live-model smoke, use the [configured-model command](../../../docs/reference/BENCHMARKS.md#run-with-a-configured-model) with `mcp-atlas@2.0.0` and its default task. Harbor limits outbound agent traffic to the model endpoint. The Atlas sandbox remains in the task network and the independent verifier uses the controlled claim judge.

## Published cases

`public-cases.json` contains five exact task records from the [pinned public split](https://huggingface.co/datasets/ScaleAI/MCP-Atlas/tree/8c563b55d7c967755f474299848049834d624617). Use `TALOS_BENCH_TASKS` with the configured OpenAI model, config, and dotenv to select one or more IDs.

Set the configured-model variables and pass the saved selection to the launcher.

```bash
TALOS_BENCH_TASKS="$(python3 -c 'import json; print(",".join(
    task["TASK"] for task in json.load(open("talos-bench/suites/mcp-atlas/public-cases.json"))["tasks"]
))')" ./talos-bench/run.sh mcp-atlas@2.0.0
```

| Task ID | Workflow |
| --- | --- |
| `686d2c79e1db37ea23ddda26` | Find the initial commit in a local Snake game repository |
| `68993ef3cf3e953b8ab83fe3` | Combine a historical event date with local Git history |
| `6863f438a500a4b36aab0f24` | Research two historical suspension-bridge records |
| `68a398aa2f58036e8d45ee0c` | Compare public transportation in two Santiago communes |
| `68993ef3cf3e953b8ab83fdf` | Find a walking route to a charging station near Tomorrowland |

These tasks expose their original tool menus through the pinned sandbox. The agent and sandbox share a per-task allowlist for the OpenAI endpoint and the selected public data services. Live data and rate limits can affect tool results. Harbor collects the sandbox log with the trial artifacts.

The separate verifier sends the published claims and final answer to the upstream scorer using the OpenAI judge `gpt-4.1-2025-04-14`. The judge receives the OpenAI credential from the configured Talos dotenv. Set `TALOS_BENCH_ATLAS_JUDGE_MODEL` to use another Chat Completions model that supports structured outputs and temperature zero. Harbor records the coverage reward, per-claim verdicts, raw result, Talos transcript, and captured answer. A judge failure leaves the trial unscored. The report treats coverage of at least 0.75 as passing and records full coverage separately. The scores identify the OpenAI judge and this five-case selection. [Upstream scoring metrics](https://github.com/scaleapi/mcp-atlas/blob/e3dfef3ce93287c3d7b9e765c3a021f252eefbff/README.md#6-score)

## Export

Export a completed Talos trial locally using its Harbor trial directory.

```bash
uv run --python 3.12 python talos-bench/suites/mcp-atlas/export.py \
  output/talos-bench/run.EXAMPLE/mcp-atlas/barber-shop-first-word__EXAMPLE \
  output/talos-bench/atlas-export
```

The export writes the upstream `outputs.csv` columns from the recorded Talos messages, tool calls, tool results and the scored answer. It copies the original scored CSV, coverage statistics, and judge verdicts. `run_config.json` records the sandbox pin, scorer revision, actual judge, split, and the dataset revision for a published case. [Upstream CSV writer](https://github.com/scaleapi/mcp-atlas/blob/e3dfef3ce93287c3d7b9e765c3a021f252eefbff/run_eval.py)
