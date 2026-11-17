runner=harbor
package=harbor==0.25.0
default_task=terminal-bench/log-summary-date-ranges
fixture_task=
require_atif=1
agent_args=(--agent harbor_talos:AtifTalosAgent)

prepare_suite() {
  local dataset task submission submissions native_config
  suite_args=()
  if [[ -n "${TALOS_BENCH_EXTRA_DOCKER_COMPOSE:-}" ]]; then
    suite_args+=(--extra-docker-compose "$TALOS_BENCH_EXTRA_DOCKER_COMPOSE")
  fi
  if [[ "$TALOS_BENCH_PLATFORM" == linux/arm64 ]]; then
    suite_args+=(--force-build --extra-docker-compose "$bench_dir/runtime/native.compose.yaml")
    native_config="$(uv run --python 3.12 --with "$package" python "$suite_dir/prepare_native.py" "$registry" "$run_dir" "$tasks")"
    if [[ -n "$native_config" ]]; then
      tasks="$native_config"
      registry="$run_dir/native-registry.json"
      suite_args+=(--config "$run_dir/native-job.json")
      return
    fi
  fi
  dataset="$(uv run --python 3.12 python "$bench_dir/summarize.py" dataset-ref "$registry")"
  suite_args+=(--dataset "$dataset")
  while IFS= read -r task; do
    suite_args+=(--include-task-name "$task")
  done <<< "$selection"
  submissions="$(uv run --python 3.12 python "$bench_dir/summarize.py" submissions "$registry" "$tasks" --extra-only)"
  while IFS= read -r submission; do
    if [[ -n "$submission" ]]; then
      suite_args+=(--artifact "$submission")
    fi
  done <<< "$submissions"
}
