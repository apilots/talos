#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(git -C "${script_dir}" rev-parse --show-toplevel)"
if [[ $# -ne 0 ]]; then
    echo "Usage: build.sh (production template only; no private configuration input)" >&2
    exit 2
fi
image="${IMAGE:-talos-ubuntu-pre}"
read -r -a platforms <<< "${PLATFORMS:-arm64 amd64}"
output="${OUTPUT_DIR:-${repo_root}/output/ubuntu-offline}"
revision="$(git -C "${repo_root}" rev-parse HEAD)"
mkdir -p "${repo_root}/.tmp"
context="$(mktemp -d "${repo_root}/.tmp/ubuntu-offline.XXXXXX")"
trap 'rm -rf "${context}"' EXIT
mkdir -p "${output}"
cp "${repo_root}/deploy/docker/talos-worker/chromium-seccomp.json" "${output}/chromium-seccomp.json"
cp "${script_dir}/run.sh" "${output}/run.sh"
printf '%s:%s\n' "${image}" "${revision:0:8}" > "${output}/image-prefix.txt"

# Only committed product inputs enter the context; runtime configuration stays in the data volume.
git -C "${repo_root}" archive HEAD talos-rs talos-app conf/talos-prod.yaml conf/migrations build/worker/browser build/container build/plugins deploy/ubuntu-offline/Dockerfile LICENSE NOTICE THIRD_PARTY_NOTICES \
    | tar -x -C "${context}"

for arch in "${platforms[@]}"; do
    case "${arch}" in arm64|amd64) ;; *) echo "Unsupported architecture: ${arch}" >&2; exit 1 ;; esac
    tag="${image}:${revision:0:8}-${arch}"
    docker buildx build --platform "linux/${arch}" --load --progress=plain \
        --file "${context}/deploy/ubuntu-offline/Dockerfile" \
        --build-arg "REVISION=${revision}" --build-arg "BUILD_JOBS=${BUILD_JOBS:-4}" \
        -t "${tag}" "${context}" 2>&1 | tee "${output}/build-${arch}.log"
    docker image inspect "${tag}" --format '{{.Os}}/{{.Architecture}} {{.Id}}' \
        | tee "${output}/image-${arch}.txt"
    docker save "${tag}" | gzip > "${output}/talos-ubuntu-pre-${arch}.tar.gz"
done
python3 "${script_dir}/package.py" "${output}" "${revision}" "${platforms[@]}"
printf 'Images and archives: %s\n' "${output}"
