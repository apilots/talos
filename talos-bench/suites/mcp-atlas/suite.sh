runner=harbor
package=harbor==0.25.0
default_task=barber-shop-first-word
fixture_task=barber-shop-first-word
agent_args=(--agent harbor_talos:TalosAgent
  --agent-kwarg "fixture_file=$suite_dir/atlas_fixture.py"
  --agent-kwarg include_content=true
  --agent-kwarg capture=answer)

prepare_suite() {
  local case_path="$suite_dir/cases/$selection" mcp="$suite_dir/mcp.json"
  if [[ "$mode" == real ]]; then
    local catalog="${TALOS_BENCH_PRODUCT:-$repo_root}" task
    local -a selected=()
    while IFS= read -r task; do selected+=("$task"); done <<< "$selection"
    if [[ "$selection" != "$fixture_task" ]]; then
      TALOS_BENCH_ATLAS_JUDGE_API_KEY="$(uv run --python 3.12 --with PyYAML==6.0.3 \
        --with python-dotenv==1.2.4 python -c '
from io import StringIO
from pathlib import Path
import sys
from dotenv import dotenv_values
from prepare import model_connection

connection, assignment = model_connection(Path(sys.argv[1]))
if connection["provider"] != "openai":
    raise ValueError("The Atlas public trials use the configured OpenAI credential for judging")
key = dotenv_values(stream=StringIO(assignment), interpolate=False)["TALOS_LLM_OPENAI_API_KEY"]
if not key:
    raise ValueError("The configured OpenAI judge credential is empty")
print(key)
' "$catalog")"
      export TALOS_BENCH_ATLAS_JUDGE_API_KEY
    fi
    uv run --python 3.12 --with PyYAML==6.0.3 python "$suite_dir/prepare.py" \
      "$suite_dir" "$run_dir/task" "$catalog" "${selected[@]}"
    case_path="$run_dir/task"
    if [[ ${#selected[@]} == 1 ]]; then
      case_path="$case_path/$selection"
    fi
    mcp="$run_dir/task/mcp.json"
  elif [[ "$selection" != "$fixture_task" ]]; then
    echo 'Published Atlas tasks require a configured model and judge.' >&2
    exit 1
  fi
  agent_args+=(--agent-kwarg "mcp_config=$mcp")
  suite_args=(--path "$case_path" --force-build)
}
