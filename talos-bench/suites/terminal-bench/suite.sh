runner=harbor
package=harbor==0.25.0

case "$suite_version" in
  2.0)
    default_task=regex-log
    fixture_task=regex-log
    agent_args=(--agent harbor_talos:TalosAgent --agent-kwarg "fixture_file=$suite_dir/regex_log_fixture.py")
    ;;
  4.1.0)
    default_task=production-planning
    fixture_task=production-planning
    fixture_reward=0
    require_atif=1
    agent_args=(--agent harbor_talos:AtifTalosAgent --agent-kwarg "fixture_file=$suite_dir/production_planning_fixture.py")
    ;;
  *) echo "Unsupported Terminal-Bench version: $suite_version" >&2; exit 1 ;;
esac

prepare_suite() {
  if [[ "$suite_version" != 2.0 ]]; then
    job_name="$suite-$suite_version"
  elif [[ "$selection" == regex-log ]]; then
    job_name=regex-log
  fi
  suite_args=(--registry-path "$registry" --dataset "terminal-bench@$suite_version"
    --force-build --extra-docker-compose "$suite_dir/arm64.compose.yaml")
  while IFS= read -r task; do
    suite_args+=(--include-task-name "$task")
  done <<< "$selection"
  if [[ "$suite_version" == 2.0 ]]; then
    local submissions
    submissions="$(uv run --python 3.12 python "$bench_dir/summarize.py" submissions "$registry" "$tasks")"
    while IFS= read -r submission; do
      suite_args+=(--artifact "$submission")
    done <<< "$submissions"
  fi
}
