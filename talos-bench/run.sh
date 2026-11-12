#!/usr/bin/env bash
set -euo pipefail

bench_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$bench_dir/.." && pwd)"
if [[ $# -eq 1 && "$1" == --list ]]; then
  exec uv run --python 3.12 python "$bench_dir/summarize.py" list "$bench_dir/suites"
fi
if [[ $# -gt 1 ]]; then
  echo 'Usage: talos-bench/run.sh [suite[@version] | --list]' >&2
  exit 1
fi
selector="${1:-${TALOS_BENCH_SUITE:-terminal-bench}}"
suite="${selector%%@*}"
suite_dir="$bench_dir/suites/$suite"
if [[ ! "$suite" =~ ^[a-z0-9][a-z0-9-]*$ || ! -r "$suite_dir/suite.sh" ]]; then
  echo "Unknown Talos benchmark suite: $suite" >&2
  exit 1
fi
registry="$suite_dir/registry.json"
if [[ "$selector" == *@* ]]; then
  requested_version="${selector#*@}"
  if [[ ! "$requested_version" =~ ^[0-9]+(\.[0-9]+){1,2}$ ]]; then
    echo "Invalid benchmark version: $requested_version" >&2
    exit 1
  fi
  version_registry="$suite_dir/registry-$requested_version.json"
  if [[ -r "$version_registry" ]]; then
    registry="$version_registry"
  fi
fi
suite_version="$(uv run --python 3.12 python "$bench_dir/summarize.py" version "$registry")"
if [[ -n "${requested_version:-}" && "$requested_version" != "$suite_version" ]]; then
  echo "Unknown Talos benchmark version: $selector" >&2
  exit 1
fi
# shellcheck source-path=SCRIPTDIR
# shellcheck source=suites/terminal-bench/suite.sh
source "$suite_dir/suite.sh"
export DOCKER_DEFAULT_PLATFORM=linux/arm64
export PYTHONPATH="$bench_dir:$bench_dir/runtime:$suite_dir"
export HARBOR_TELEMETRY=0

set --
uv_args=(--python 3.12 --from "$package")
mode=fixture
repeats="${TALOS_BENCH_REPEATS:-1}"
if [[ ! "$repeats" =~ ^[1-9][0-9]*$ ]]; then
  echo 'TALOS_BENCH_REPEATS must be a positive integer.' >&2
  exit 1
fi
case "${TALOS_BENCH_ORACLE:-0}" in
  1)
    if [[ -n "${TALOS_BENCH_MODEL:-}" ]]; then
      echo 'Select either the oracle or a configured model.' >&2
      exit 1
    fi
    mode=oracle
    ;;
  0) ;;
  *) echo 'TALOS_BENCH_ORACLE must be 0 or 1.' >&2; exit 1 ;;
esac
if [[ -n "${TALOS_BENCH_MODEL:-}" ]]; then
  model_config="${TALOS_BENCH_CONFIG:?Set TALOS_BENCH_CONFIG to the existing Talos model configuration.}"
  model_dotenv="${TALOS_BENCH_DOTENV:?Set TALOS_BENCH_DOTENV to the existing Talos dotenv.}"
  if [[ ! -r "$model_config" || ! -r "$model_dotenv" ]]; then
    echo 'The selected Talos config and dotenv must be readable.' >&2
    exit 1
  fi
  set -- --agent-kwarg "talos_model=$TALOS_BENCH_MODEL" \
    --agent-kwarg "talos_config=$(cd "$(dirname "$model_config")" && pwd)/$(basename "$model_config")" \
    --agent-kwarg "talos_dotenv=$(cd "$(dirname "$model_dotenv")" && pwd)/$(basename "$model_dotenv")"
  uv_args+=(--with PyYAML==6.0.3)
  mode=real
fi

tasks="${TALOS_BENCH_TASKS:-$default_task}"
selection="$(uv run --python 3.12 python "$bench_dir/summarize.py" select "$registry" "$tasks")"
if [[ "$mode" == fixture && "$selection" != "$fixture_task" ]]; then
  echo 'The selected tasks have no controlled fixture. Select an oracle or configured model.' >&2
  exit 1
fi
if [[ "$(docker version --format '{{.Server.Os}}/{{.Server.Arch}}')" != linux/arm64 ]]; then
  echo 'A native linux/arm64 Docker daemon is required.' >&2
  exit 1
fi

mkdir -p "$repo_root/.tmp" "$repo_root/output/talos-bench"
run_dir="$(mktemp -d "$repo_root/output/talos-bench/run.XXXXXX")"
job_name="$suite"
prepare_suite
set -- "$@" "${suite_args[@]}"
summary_args=()
if [[ "$mode" == oracle ]]; then
  set -- "$@" --agent oracle
else
  product="${TALOS_BENCH_PRODUCT:-}"
  product_source=provided
  if [[ -z "$product" ]]; then
    product_source=checkout
    product="$(mktemp -d "$repo_root/.tmp/talos-bench-product.XXXXXX")"
    trap 'rm -rf -- "$product"' EXIT
    docker buildx build --platform linux/arm64 \
      -f "$bench_dir/runtime/build.Dockerfile" --output "type=local,dest=$product" "$repo_root"
  fi
  product="$(cd "$product" && pwd)"
  if [[ ! -x "$product/bin/talos" || ! -x "$product/bin/talos-linux-sandbox-launcher" ||
        ! -f "$product/conf/talos.yaml" || ! -d "$product/conf/migrations/sqlite" ]]; then
    echo 'The Talos product must contain the CLI, Linux sandbox launcher, configuration, and SQLite migrations.' >&2
    exit 1
  fi
  set -- "$@" "${agent_args[@]}" --agent-kwarg "product=$product"
  summary_args=(--require-trace)
  if [[ "${require_atif:-0}" == 1 ]]; then
    summary_args+=(--require-atif)
  fi
  if [[ "$mode" == fixture ]]; then
    summary_args+=(--fixture-reward "${fixture_reward:-1}")
  fi
  uv run --python 3.12 python "$bench_dir/summarize.py" build \
    "$run_dir" "$repo_root" "$product/bin/talos" "$product_source" "$mode" "$repeats" --tasks "$tasks"
fi

runner_exit=0
uvx "${uv_args[@]}" "$runner" run --env docker \
  --n-attempts "$repeats" --n-concurrent 1 --job-name "$job_name" --jobs-dir "$run_dir" --yes "$@" \
  || runner_exit=$?

uv run --python 3.12 python "$bench_dir/summarize.py" summary \
  "$run_dir" "$mode" "$repeats" --tasks "$tasks" --job-name "$job_name" \
  --registry "$registry" --suite "$suite" --runner "$runner" --runner-exit "$runner_exit" ${summary_args[@]+"${summary_args[@]}"}
