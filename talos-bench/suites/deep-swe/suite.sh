runner=pier
package=datacurve-pier==0.3.1
default_task=python-statemachine-state-data-scoping
fixture_task=
agent_args=(--agent-import-path talos_agent:TalosAgent)

prepare_suite() {
  while IFS= read -r task; do
    uv run --python 3.12 python "$suite_dir/prepare.py" "$run_dir" "$task" "$repo_root/.tmp"
  done <<< "$selection"
  suite_args=(--path "$run_dir/task" --no-delete)
}
