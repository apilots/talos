#!/usr/bin/env bash
set -euo pipefail
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
arch="${1:-$(uname -m)}"
case "${arch}" in x86_64|amd64) arch=amd64 ;; aarch64|arm64) arch=arm64 ;; *) echo "Unsupported architecture: ${arch}" >&2; exit 1 ;; esac
prefix="${IMAGE_PREFIX:-$(cat "${script_dir}/image-prefix.txt")}"
image="${prefix}-${arch}"
if ! docker image inspect "${image}" >/dev/null 2>&1; then
    docker load -i "${script_dir}/talos-ubuntu-pre-${arch}.tar.gz"
fi
docker volume create "${DATA_VOLUME:-talos-pre-data}" >/dev/null
docker run -d --name "${CONTAINER_NAME:-talos-pre}" --platform "linux/${arch}" \
    --restart unless-stopped --shm-size=1g \
    --security-opt no-new-privileges:true \
    --security-opt "seccomp=${script_dir}/chromium-seccomp.json" \
    -p "${HTTP_PORT:-19091}:19091" \
    -v "${DATA_VOLUME:-talos-pre-data}:/home/talos/.talos" "${image}"
printf 'WebUI: http://localhost:%s\nTUI: docker exec -it %s talos\n' "${HTTP_PORT:-19091}" "${CONTAINER_NAME:-talos-pre}"
