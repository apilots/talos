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
  if [[ -n "${TALOS_BENCH_CONFIG:-}${TALOS_BENCH_DOTENV:-}" ]]; then
    model_config="${TALOS_BENCH_CONFIG:-}"
    model_dotenv="${TALOS_BENCH_DOTENV:-}"
    if [[ ! -r "$model_config" || ! -r "$model_dotenv" ]]; then
      echo 'Provide both a readable TALOS_BENCH_CONFIG and TALOS_BENCH_DOTENV.' >&2
      exit 1
    fi
    set -- --agent-kwarg "talos_model=$TALOS_BENCH_MODEL" \
      --agent-kwarg "talos_config=$(cd "$(dirname "$model_config")" && pwd)/$(basename "$model_config")" \
      --agent-kwarg "talos_dotenv=$(cd "$(dirname "$model_dotenv")" && pwd)/$(basename "$model_dotenv")"
  else
    set -- --model "${TALOS_BENCH_MODEL%%:*}/${TALOS_BENCH_MODEL#*:}"
  fi
  uv_args+=(--with PyYAML==6.0.3)
  mode=real
fi
if [[ -n "${TALOS_BENCH_AGENT_ENV:-}" ]]; then
  while IFS= read -r entry; do
    if [[ ! "$entry" =~ ^[A-Za-z_][A-Za-z0-9_]*= ]]; then
      echo 'TALOS_BENCH_AGENT_ENV accepts one KEY=VALUE per line.' >&2
      exit 1
    fi
    set -- "$@" --agent-env "$entry"
  done <<< "$TALOS_BENCH_AGENT_ENV"
fi

tasks="${TALOS_BENCH_TASKS:-$default_task}"
selection="$(uv run --python 3.12 python "$bench_dir/summarize.py" select "$registry" "$tasks")"
if [[ "$mode" == fixture && "$selection" != "$fixture_task" ]]; then
  echo 'The selected tasks have no controlled fixture. Select an oracle or configured model.' >&2
  exit 1
fi
native_platform="$(docker version --format '{{.Server.Os}}/{{.Server.Arch}}')"
case "$native_platform" in
  linux/arm64|linux/amd64) ;;
  *) echo "Unsupported Docker platform: $native_platform" >&2; exit 1 ;;
esac
if [[ "${TALOS_BENCH_PLATFORM:-$native_platform}" != "$native_platform" ]]; then
  echo "TALOS_BENCH_PLATFORM must match the native Docker platform ($native_platform)." >&2
  exit 1
fi
export TALOS_BENCH_PLATFORM="$native_platform"
export DOCKER_DEFAULT_PLATFORM="$native_platform"

mkdir -p "$repo_root/.tmp" "$repo_root/output/talos-bench"
run_dir="$(mktemp -d "$repo_root/output/talos-bench/run.XXXXXX")"
printf 'Talos benchmark run: %s\n' "$run_dir"
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
    docker buildx build --platform "$TALOS_BENCH_PLATFORM" \
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
