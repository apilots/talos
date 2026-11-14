#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 2 ]]; then
  echo 'Usage: talos-bench/install.sh <Talos git revision> <new output directory>' >&2
  exit 1
fi

bench_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$bench_dir/.." && pwd)"
revision="$(git -C "$repo_root" rev-parse --verify --end-of-options "$1^{commit}")"
destination="$2"
native_platform="$(docker version --format '{{.Server.Os}}/{{.Server.Arch}}')"
case "$native_platform" in
  linux/arm64|linux/amd64) ;;
  *) echo "Unsupported Docker platform: $native_platform" >&2; exit 1 ;;
esac
if [[ "${TALOS_BENCH_PLATFORM:-$native_platform}" != "$native_platform" ]]; then
  echo "TALOS_BENCH_PLATFORM must match the native Docker platform ($native_platform)." >&2
  exit 1
fi
if [[ -e "$destination" || -L "$destination" ]]; then
  echo 'Choose a new output directory.' >&2
  exit 1
fi

mkdir -p -- "$(dirname "$destination")"
staging="$(mktemp -d "$(dirname "$destination")/.talos-bench-install.XXXXXX")"
trap 'rm -rf -- "$staging"' EXIT
mkdir "$staging/source" "$staging/install"
git -C "$repo_root" archive "$revision" -- talos-rs conf deploy/docker/talos-worker talos-bench/runtime \
  | tar -xf - -C "$staging/source"
docker buildx build --platform "$native_platform" \
  -f "$staging/source/talos-bench/runtime/build.Dockerfile" \
  --output "type=local,dest=$staging/install/product" "$staging/source"
mkdir "$staging/install/adapter"
cp "$bench_dir"/runtime/{atif,delegation_watch,fixture,harbor_talos,payload,run_talos}.py "$staging/install/adapter/"
cp "$bench_dir/runtime/native.compose.yaml" "$staging/install/adapter/"
printf '%s\n' "$revision" > "$staging/install/product-revision.txt"
mv -- "$staging/install" "$destination"
printf 'Installed Talos product and Harbor adapter at %s\n' "$destination"
