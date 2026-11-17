# FrontierHarness v1.1 local batch

Use the [shared Bench launcher](../../../docs/reference/BENCHMARKS.md#run-frontierharness-v11) to select the local or Runta backend for registered tasks and set a model reasoning effort.

Run from the repository root with native Linux ARM64 Docker and `uv`. The inventory resolves the exact Hub dataset pin from `registry.json`. It records all 62 task digests, environment requirements and admission decisions before a batch starts. Package inspection and image manifest checks do not execute a task.

```bash
uv run talos-bench/suites/frontier-harness/preflight.py output/frontier-local/preflight.json
uv run --python 3.12 --with harbor==0.25.0 --with PyYAML==6.0.3 \
  python talos-bench/suites/frontier-harness/batch.py output/frontier-local/preflight.json \
  --product /path/to/native/talos/product --dotenv /path/to/main/.env \
  --jobs 3 --memory-mb 10000
python3 talos-bench/suites/frontier-harness/report.py output/frontier-local
```

The batch defaults to one OpenAI GPT-6.1 Terra attempt per admitted task. Pass `--model openai/<id>` to reproduce a run with another model in the Talos catalog. The dotenv supplies `TALOS_LLM_OPENAI_API_KEY`. An existing case directory preserves an active or recorded attempt and excludes it from another batch. For disjoint parallel batches, pass each qualified task name as a separate `--task` and divide the host memory budget between the batches.

Rerun preflight with `--retry-unresolved` to retry package and manifest lookups while retaining earlier reasons.

Each case directory retains the Harbor job, Talos trajectory, usage, summary, execution log and outcome. An ARM64 verifier rebuild uses a disposable copy with the published verifier image pointer removed and records the original and local digests. The original task verifier code runs unchanged. Archive a failed attempt directory before retrying so the first outcome remains reviewable.

The report writes `index.html` and `coverage.json`. Each JSON row includes the qualified task name, original digest, completion status, and paths to available outcome, summary, and original verifier evidence. A saved score counts when the original task pin, ATIF, submissions, and verifier result still verify. The Harbor source trial ID binds a regrade result to the original run. Explicit native exclusions come from `registry.json` and match both the qualified task name and its digest. These rows record the reason and have no score. The preflight skips downloading those tasks. Local model failures may carry a local zero without increasing the original-verifier score count. Local acceptance accounts for every row once each eligible case has an original-verifier score and each excluded case has its reason.

To reconcile successful retries stored outside the original batch, pass each archived single-trial summary and direct the report to a new directory. The report checks the original dataset and task pins, native platform, verifier result, ATIF file, and recorded submissions. Scored original batch rows remain in place. The retry and the previous attempt are linked in the report.

```bash
python3 talos-bench/suites/frontier-harness/report.py output/frontier-local \
  --output-dir output/frontier-reconciled \
  --supplemental-summary output/frontier-retry/cases/rstan-to-pystan/summary.json
```

Harbor Compose tasks can declare artifacts from multiple services. The summary checks each declared service and source path against the collection manifest, even when several services write to the same path. A reward with missing required artifacts remains visible in the raw result and is excluded from the completed score count.

If the cold start reaches the task's environment deadline, inspect the Harbor exception and Docker BuildKit history for the failed phase. Prepare the admitted task's unchanged Dockerfile outside the scored trial.

```bash
uv run --python 3.12 --with harbor==0.25.0 \
  python talos-bench/suites/frontier-harness/prebuild.py output/frontier-local/preflight.json \
  output/frontier-local/prepared terminal-bench/prove-plus-comm agent
```

Pass `--prepared-images output/frontier-local/prepared` to the batch command for a prepared retry. Build the `verifier` role too when its separate upstream image lacks ARM64 support. Each receipt records the task pin, native image ID, and preparation command. The batch checks those values and updates only the image pointers in a disposable task copy. The scored run keeps the original deadlines. For a published native image, use the exact ARM64 manifest and record its local image ID in the corresponding preparation receipt.

For `terminal-bench/gsea-proteomics` at the registered dataset pin, the `agent` preparation appends [native Java setup](images/gsea-proteomics-arm64.Dockerfile) to the original build. The original GSEA 4.4.0 launchers use system Java after removal of their bundled x86-64 JDK. Prepare both roles and pass `--prepared-images` for this task.

For a reviewed Compose task, pass its exact qualified name with `--task`. The `agent.json` receipt must include a `services` map with an image receipt for every Compose service. Each service receipt uses the same `taskRef`, `image`, `imageId` and `platform` fields. The batch generates a Compose override with the validated native images and preserves the original service commands, dependencies and health checks. The main service and agent receipts must identify the same image.

For an archived trial with complete submissions and a separate verifier, use Harbor `trial regrade` with the same pinned task and native verifier recipe. Set `DOCKER_DEFAULT_PLATFORM=linux/arm64` and `PYTHONPATH` to the benchmark runtime directory so Harbor can resolve the recorded agent type. Keep the regrade output separate from the source trial.

For the pinned `terminal-bench/filter-js-from-html` case, add `--verifier-env SE_CHROMEDRIVER=/usr/bin/chromedriver` to the batch command when using its native ARM64 image. Selenium then uses the Chromium driver installed by the upstream Dockerfile. The registry also captures `/app/filter.py` for review.

Use the report to check coverage, scores and failure evidence. If a verifier writes zero after a dependency fails before the checker can run, preserve that raw reward and exclude the attempt from the local score as an infrastructure failure. A local native pass can include rebuilt environments and unavailable tasks. The published FrontierHarness protocol uses four scored attempts per task. [FrontierHarness scoring protocol](https://frontierharness.org/about/)

## Runta one-attempt batch

The batch controller runs each selectable task once in a separate Runta restore
using the shared launcher. Set `TALOS_BENCH_RUNTA_CHECKPOINT`,
`TALOS_BENCH_RUNTA_SECRET`, `TALOS_BENCH_MODEL`, and
`TALOS_BENCH_REASONING_EFFORT` as in the shared guide, then run:

```bash
uv run --python 3.12 python talos-bench/suites/frontier-harness/runta_batch.py output/frontier-runta/batch --jobs 1
```

`--task` may be repeated to select a subset. Use `--limit 1` to start only
the first pending case while keeping the full batch manifest for later resume;
`--first terminal-bench/regex-log` chooses the first case. The manifest pins the task list,
checkpoint, dataset, model, effort and launcher contents. Each case keeps its
launcher log, runtime name, result directory, archive checksum and raw result.
A case directory is created before launch and is never automatically retried,
including after a controller interruption or transfer failure. Inspect its log
and retained runtime and recover its original job before deciding on a separate
infrastructure retry. Resuming the same batch refuses configuration changes and
skips every started case. The five metadata-only entries remain excluded and are
recorded in the manifest. Preserve the original run directories alongside the
batch directory when moving or submitting evidence.

This controller records a single attempt, not the four-attempt leaderboard
protocol. A Runta runtime alone does not establish equivalence to the official
v1.1 configuration; document the model, resource limits, network policy,
checkpoint, runner versions and any remaining differences with each submission.

After the batch, create a portable evidence bundle with:

```bash
uv run --python 3.12 python talos-bench/suites/frontier-harness/runta_collect.py \
  output/frontier-runta/batch output/frontier-runta/evidence.zip
```

The bundle contains each original archive whose saved SHA-256 still matches and
an index that retains missing, unfinished and unscored cases. It omits the local
secret name and local controller logs. Inspect the archives and add installation,
model and methodology details before sending them to the benchmark maintainers.
