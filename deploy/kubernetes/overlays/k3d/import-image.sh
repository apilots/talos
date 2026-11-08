#!/usr/bin/env bash
set -euo pipefail

cluster=${1:?Usage: import-image.sh K3D_CLUSTER LOCAL_IMAGE COMPONENT}
image=${2:?Usage: import-image.sh K3D_CLUSTER LOCAL_IMAGE COMPONENT}
component=${3:?Usage: import-image.sh K3D_CLUSTER LOCAL_IMAGE COMPONENT}
case "$component" in
  server|worker) ;;
  *) echo "Expected component server or worker" >&2; exit 1 ;;
esac
node="k3d-$cluster-server-0"
ctr=(docker exec "$node" ctr --address /run/k3s/containerd/containerd.sock --namespace k8s.io)
id=$(docker image inspect --format '{{.Id}}' "$image")
repository="localhost/talos-k3d/$component"
tag="$repository:${id#sha256:}"
manifest_digest() {
  "${ctr[@]}" images list | awk -v ref="$tag" '$1 == ref { print $3 }'
}
digest=$(manifest_digest)
if [[ -z "$digest" ]]; then
  docker tag "$image" "$tag"
  trap 'docker image rm "$tag" >/dev/null' EXIT
  k3d image import --mode direct "$tag" --cluster "$cluster" >&2
  digest=$(manifest_digest)
fi
[[ "$digest" =~ ^sha256:[0-9a-f]{64}$ ]]
pinned="$repository@$digest"
if ! "${ctr[@]}" images list -q | grep -Fx "$pinned" >/dev/null; then
  "${ctr[@]}" images tag "$tag" "$pinned" >&2
fi
docker exec "$node" crictl inspecti "$pinned" >/dev/null
printf '%s\n' "$pinned"
