# SWE-bench Lite

Run the suite from the checkout root with a native Linux ARM64 or AMD64 Docker daemon, Buildx, and `uv`. The default case is `sympy__sympy-20590`.

```sh
TALOS_BENCH_REPEATS=2 talos-bench/run.sh swe-bench-lite
TALOS_BENCH_ORACLE=1 talos-bench/run.sh swe-bench-lite
```

The first command uses a controlled local Chat Completions provider. Talos edits the pinned SymPy checkout, Harbor archives its Git patch, and the separate verifier applies that patch to a clean checkout. SWE-bench 5.0.2 loads the upstream task script and computes the official report. The oracle submits the upstream gold patch through the same verifier. The agent image contains a single-commit checkout. The test inputs are built into the verifier image.

## Five-case evaluation

The suite registry pins five tasks from the [SWE-bench Lite test split](https://huggingface.co/datasets/SWE-bench/SWE-bench_Lite/tree/b0dde1093fe417d83b7184254edf8199c1f0dff5). They cover symbolic object layout, polynomial factorization, complex sign rewriting, reference-frame mechanics, and integer partition iteration. The upstream tasks share the SymPy 1.7 dependency setup, so a native ARM64 run builds the shared environment once and keeps separate checkouts and verifiers for each base commit. Each task uses the original evaluation script and test expectations.

The local real-model profile gives Talos 900 seconds per task. The verifier and the controlled and Oracle runs each retain their 300-second deadline.

Use the [configured-model settings](../../../docs/reference/BENCHMARKS.md#run-with-a-configured-model) with this selection:

```sh
TALOS_BENCH_TASKS=sympy__sympy-20590,sympy__sympy-19254,sympy__sympy-19487,sympy__sympy-20049,sympy__sympy-20154 \
talos-bench/run.sh swe-bench-lite
```

## Evidence and packaging

Harbor saves the patch, verifier report, and image provenance under `output/talos-bench/run.*`. Talos runs also save the CLI trajectory. Real-model runs retain an inference transcript for upstream submission staging.

To stage a completed real-model run with one attempt per instance, use a new output directory:

```sh
python3 talos-bench/suites/swe-bench-lite/export.py output/talos-bench/run.ID output/swe-lite-export
```

This writes `all_preds.jsonl`, per-instance logs and patches, and inference transcripts. Use the model recorded in `all_preds.jsonl` as the package model. For a GPT-6.1 Terra run, use the pinned upstream submission CLI:

```sh
uvx --python 3.12 \
  --from 'swebench[datasets] @ git+https://github.com/SWE-bench/SWE-bench.git@02e7a74ffd0b707aab73d203fe87bdc7c76afc8e' \
  swebench submit package output/swe-lite-export/logs --split lite --model openai:gpt-6.1-terra \
  --predictions output/swe-lite-export/all_preds.jsonl --trajs output/swe-lite-export/trajs \
  --out output/swe-lite-package
```

The command loads the public Lite split and regrades the saved test output. Use the same CLI with `submit verify output/swe-lite-package/entry --split lite --logs output/swe-lite-package/submission-repo/logs` to verify the package locally. [Submission requirements](https://github.com/SWE-bench/experiments#-leaderboard-participation)

The AMD64 agent build uses the upstream instance image pinned by manifest digest and checks its worktree against the benchmark base commit. The ARM64 build uses native recipes for the base image and SymPy test environment. A cold ARM64 build uses GitHub, Docker Hub, GitHub Container Registry, Ubuntu, Anaconda, and PyPI. The default instance is published in both Lite and Verified. [Pinned SWE-bench task](https://github.com/SWE-bench/swe-bench-tasks/tree/3d07b464b7b311a0cbfb5ed5b2d8a3b96f84a33d/tasks/sympy__sympy-20590)
