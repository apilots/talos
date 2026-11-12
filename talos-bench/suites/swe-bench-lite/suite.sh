runner=harbor
package=harbor==0.25.0
default_task=sympy__sympy-20590
fixture_task=sympy__sympy-20590
agent_args=(--agent harbor_talos:TalosAgent --agent-kwarg "capture=patch")

prepare_suite() {
  if [[ "$mode" == fixture ]]; then
    agent_args+=(--agent-kwarg "fixture_file=$suite_dir/cases/$selection/fixture_model.py")
  fi
  uv run --python 3.12 --script "$suite_dir/prepare.py" "$run_dir" "$selection" "$mode"
  suite_args=(--path "$run_dir/task/$selection")
}
