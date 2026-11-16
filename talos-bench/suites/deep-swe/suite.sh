runner=pier
package=datacurve-pier==0.3.1
default_task=python-statemachine-state-data-scoping
fixture_task="$default_task"
fixture_reward=0
agent_args=(--agent-import-path talos_agent:TalosAgent)

prepare_suite() {
  if [[ "$mode" == fixture ]]; then
    agent_args+=(--agent-kwarg "fixture_file=$suite_dir/fixture_model.py")
  fi
  while IFS= read -r task; do
    uv run --python 3.12 python "$suite_dir/prepare.py" "$run_dir" "$task" "$repo_root/.tmp"
  done <<< "$selection"
  suite_args=(--path "$run_dir/task" --no-delete)
}
