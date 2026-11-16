# FrontierHarness v1.1 local batch

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
