runner=harbor
package=harbor==0.25.0
default_task=terminal-bench/log-summary-date-ranges
fixture_task=
require_atif=1
agent_args=(--agent harbor_talos:AtifTalosAgent)

prepare_suite() {
  local dataset submission submissions
  dataset="$(uv run --python 3.12 python "$bench_dir/summarize.py" dataset-ref "$registry")"
  suite_args=(--dataset "$dataset")
  if [[ "$TALOS_BENCH_PLATFORM" == linux/arm64 ]]; then
    local native_args=(--force-build --extra-docker-compose "$bench_dir/runtime/native.compose.yaml")
    if printf '%s\n' "$selection" | grep -Fxq 'terminal-bench/music-harmony'; then
      tasks="$(uv run --python 3.12 --with "$package" python "$suite_dir/prepare_native.py" "$registry" "$run_dir" "$tasks")"
      registry="$run_dir/native-registry.json"
      suite_args=(--config "$run_dir/native-job.json" "${native_args[@]}")
      return
    fi
    suite_args+=("${native_args[@]}")
  fi
  while IFS= read -r task; do
    suite_args+=(--include-task-name "$task")
    if [[ "$task" == "$default_task" || "$task" == terminal-bench/video-processing ]]; then
      # These published tasks do not declare their outputs as artifacts.
      submissions="$(uv run --python 3.12 python "$bench_dir/summarize.py" submissions "$registry" "$task")"
      while IFS= read -r submission; do
        suite_args+=(--artifact "$submission")
      done <<< "$submissions"
    fi
  done <<< "$selection"
}
