# MCP-Atlas

Run the controlled case from the repository root with native ARM64 Docker.

```bash
TALOS_BENCH_REPEATS=2 ./talos-bench/run.sh mcp-atlas@2.0.0
TALOS_BENCH_ORACLE=1 ./talos-bench/run.sh mcp-atlas
```

The suite version follows the upstream MCP-Atlas 2.0.0 changelog. Talos provides the agent loop and Harbor runs the trial. The suite pins the claim scorer at commit `e3dfef3` and starts the separately versioned sandbox image `1.2.7` by its ARM64 digest. [Upstream changelog](https://github.com/scaleapi/mcp-atlas/blob/e3dfef3ce93287c3d7b9e765c3a021f252eefbff/CHANGELOG.md) · [Sandbox image recipe](https://github.com/scaleapi/mcp-atlas/blob/e3dfef3ce93287c3d7b9e765c3a021f252eefbff/Makefile)

The case asks for the first word in the upstream `Barber Shop.csv` example. Harbor connects Talos to the sandbox on an internal Docker network. Talos imports the suite's MCP configuration through its plugin CLI. The bridge uses MCP Python SDK 2.3.0 to expose the sandbox's read-only filesystem tool over standard MCP stdio. The local fixture requests ToolSearch, calls the discovered tool, and extracts the answer from its returned CSV header. [MCP-Atlas example](https://github.com/scaleapi/mcp-atlas/tree/e3dfef3ce93287c3d7b9e765c3a021f252eefbff) · [MCP Python SDK release](https://pypi.org/project/mcp/2.3.0/)

Harbor passes `answer.txt` to a separate verifier. The pinned upstream claim scorer calls a loopback judge that checks the expected word. The run directory contains the Talos trajectory and answer, judge verdict, scored CSV, and Harbor reward. The oracle command verifies the same grading path with a fixed submission. The first build downloads the pinned sandbox, dependencies and scorer. Trials use local fixture endpoints and synthetic tokens. [Upstream scorer](https://github.com/scaleapi/mcp-atlas/blob/e3dfef3ce93287c3d7b9e765c3a021f252eefbff/services/scoring/score_claims.py)

This case measures the integration path. Real-model and dataset-wide evaluation require sandbox tool coverage, official dataset claims, and a configured judge.
