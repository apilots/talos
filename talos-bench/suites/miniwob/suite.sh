runner=harbor
package=harbor==0.25.0
default_task=click-test-2
fixture_task=click-test-2
require_atif=1
agent_args=(--agent browser_talos:BrowserTalosAgent
  --agent-kwarg "fixture_file=$suite_dir/browser_fixture.py")

prepare_suite() {
  if [[ "$mode" == oracle ]]; then
    echo 'Use the controlled Browser MCP trial without TALOS_BENCH_ORACLE.' >&2
    exit 1
  fi
  local -a selected=()
  local task
  while IFS= read -r task; do selected+=("$task"); done <<< "$selection"
  uv run --python 3.12 python "$suite_dir/prepare.py" "$run_dir" "$repo_root/.tmp" "${selected[@]}"
  suite_args=(--path "$run_dir/task" --force-build)
}
