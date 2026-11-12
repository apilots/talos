runner=harbor
package=harbor==0.25.0
default_task=barber-shop-first-word
fixture_task=barber-shop-first-word
agent_args=(--agent harbor_talos:TalosAgent
  --agent-kwarg "fixture_file=$suite_dir/atlas_fixture.py"
  --agent-kwarg "mcp_config=$suite_dir/mcp.json"
  --agent-kwarg capture=answer)

prepare_suite() {
  if [[ "$mode" == real ]]; then
    echo 'The MCP-Atlas case uses an isolated network and a controlled judge. Choose fixture or oracle mode.' >&2
    return 1
  fi
  suite_args=(--path "$suite_dir/cases/$selection" --force-build)
}
