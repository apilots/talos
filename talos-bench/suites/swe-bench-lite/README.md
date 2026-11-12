# SWE-bench Lite

Run Talos on the pinned `sympy__sympy-20590` instance from the checkout root with an ARM64 host, a local ARM64 Docker daemon with Buildx, and `uv`.

```sh
TALOS_BENCH_REPEATS=2 talos-bench/run.sh swe-bench-lite
TALOS_BENCH_ORACLE=1 talos-bench/run.sh swe-bench-lite
```

The first command uses a controlled local Chat Completions provider. Talos edits the pinned SymPy checkout, Harbor archives its Git patch, and the separate verifier applies that patch to a clean checkout. SWE-bench 5.0.2 loads the upstream task script and computes the official report. The oracle submits the upstream gold patch through the same verifier. The agent image contains a single-commit checkout. The test inputs are built into the verifier image.

Harbor saves the patch, verifier report, and image provenance under `output/talos-bench/run.*`. Talos runs also save the CLI trajectory.

The upstream task image targets AMD64, so this suite owns native ARM64 recipes for the base image and SymPy test environment. A cold build uses GitHub, Docker Hub, GitHub Container Registry, Ubuntu, Anaconda, and PyPI. The selected instance is published in both Lite and Verified. [Pinned SWE-bench task](https://github.com/SWE-bench/swe-bench-tasks/tree/3d07b464b7b311a0cbfb5ed5b2d8a3b96f84a33d/tasks/sympy__sympy-20590)
