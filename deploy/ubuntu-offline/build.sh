#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(git -C "${script_dir}" rev-parse --show-toplevel)"
config="${1:?Usage: build.sh /absolute/path/to/talos-pre.yaml}"
image="${IMAGE:-talos-ubuntu-pre}"
platforms="${PLATFORMS:-arm64 amd64}"
output="${OUTPUT_DIR:-${repo_root}/output/ubuntu-server}"
revision="$(git -C "${repo_root}" rev-parse HEAD)"
mkdir -p "${repo_root}/.tmp"
context="$(mktemp -d "${repo_root}/.tmp/ubuntu-offline.XXXXXX")"
trap 'rm -rf "${context}"' EXIT
mkdir -p "${output}"
cp "${repo_root}/deploy/docker/talos-worker/chromium-seccomp.json" "${output}/chromium-seccomp.json"
cp "${script_dir}/run.sh" "${output}/run.sh"
printf '%s:%s\n' "${image}" "${revision:0:8}" > "${output}/image-prefix.txt"

# Only committed product inputs enter the context. Private configuration stays local.
git -C "${repo_root}" archive HEAD talos-rs talos-app conf/talos-prod.yaml conf/migrations build/worker/browser build/container build/plugins LICENSE NOTICE THIRD_PARTY_NOTICES \
    | tar -x -C "${context}"
cp "${script_dir}/Dockerfile" "${context}/Dockerfile"
cp "${config}" "${context}/pre.yaml"

for arch in ${platforms}; do
    case "${arch}" in arm64|amd64) ;; *) echo "Unsupported architecture: ${arch}" >&2; exit 1 ;; esac
    tag="${image}:${revision:0:8}-${arch}"
    docker buildx build --platform "linux/${arch}" --load --progress=plain \
        --build-arg "REVISION=${revision}" --build-arg "BUILD_JOBS=${BUILD_JOBS:-4}" \
        -t "${tag}" "${context}" 2>&1 | tee "${output}/build-${arch}.log"
    docker image inspect "${tag}" --format '{{.Os}}/{{.Architecture}} {{.Id}}' \
        | tee "${output}/image-${arch}.txt"
    docker save "${tag}" | gzip > "${output}/talos-ubuntu-pre-${arch}.tar.gz"
    (cd "${output}" && shasum -a 256 "talos-ubuntu-pre-${arch}.tar.gz" chromium-seccomp.json run.sh image-prefix.txt > "SHA256SUMS-${arch}")
    tar -cf "${output}/talos-ubuntu-pre-${arch}-offline.tar" -C "${output}" \
        "talos-ubuntu-pre-${arch}.tar.gz" chromium-seccomp.json run.sh image-prefix.txt "SHA256SUMS-${arch}"
done
(cd "${output}" && shasum -a 256 talos-ubuntu-pre-*.tar.gz talos-ubuntu-pre-*-offline.tar chromium-seccomp.json run.sh image-prefix.txt > SHA256SUMS)
printf 'Images and archives: %s\n' "${output}"
