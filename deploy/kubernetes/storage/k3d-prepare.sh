#!/usr/bin/env bash
set -euo pipefail

cluster=${1:?Usage: k3d-prepare.sh K3D_CLUSTER}
context="k3d-$cluster"
directory=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
node_names=$(kubectl --context "$context" get nodes -o jsonpath='{range .items[*]}{.metadata.name}{"\n"}{end}')
node="k3d-$cluster-server-0"
if [[ "$node_names" != "$node" ]]; then
  echo "The disposable quota profile requires a single k3d Server node" >&2
  exit 1
fi

base_path=/var/lib/rancher/k3s/talos-quota
backing_file="$base_path.xfs"
utility_digest=sha256:a3043d266b1c06fb42377ced0c3fc190c66e1e83ae1efda7670ed73feb4ede14
provisioner_digest=sha256:99f5116f5cb83fb9d046ef5ff5b8ea98da90e7da5250d7b101bd650662238a0a
utility_image=docker.io/openebs/linux-utils:4.6.0
profile_dir=/var/lib/kubelet/seccomp/talos
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
rendered="$scratch/chromium-seccomp.json"
"$directory/../render-chromium-seccomp.sh" "$(docker exec "$node" uname -m)" "$(docker exec "$node" uname -r)" > "$rendered"
docker exec "$node" mkdir -p "$profile_dir"
docker cp "$rendered" "$node:$profile_dir/chromium-seccomp.json.new"
docker exec "$node" sh -ec '
  chmod 0644 "$1.new"
  mv -f "$1.new" "$1"
' -- "$profile_dir/chromium-seccomp.json"

cache_image() {
  local repository=$1 version=$2 digest=$3 upstream mirror verified tagged expected cached
  upstream="docker.io/$repository@$digest"
  mirror="mirror.gcr.io/$repository@$digest"
  tagged="docker.io/$repository:$version"
  if docker image inspect "$upstream" >/dev/null 2>&1; then
    verified="$upstream"
  elif docker image inspect "$mirror" >/dev/null 2>&1; then
    verified="$mirror"
  elif docker pull "$upstream"; then
    verified="$upstream"
  else
    verified="$mirror"
    docker pull "$verified"
  fi
  docker tag "$verified" "$tagged"
  expected=$(docker image inspect --format '{{.Id}}' "$verified")
  cached=$(docker exec "$node" crictl inspecti "$tagged" 2>/dev/null | jq -r .status.id) || cached=
  if [[ "$cached" != "$expected" ]]; then
    k3d image import --mode direct --cluster "$cluster" "$tagged"
    [[ $(docker exec "$node" crictl inspecti "$tagged" | jq -r .status.id) == "$expected" ]]
  fi
}
cache_image openebs/linux-utils 4.6.0 "$utility_digest"
cache_image openebs/provisioner-localpv 4.6.0 "$provisioner_digest"
if kubectl --context "$context" --namespace kube-system get configmap local-path-config \
    -o jsonpath='{.data.helperPod\.yaml}' | grep -q 'image: rancher/mirrored-library-busybox:1.36.1$'; then
  cache_image rancher/mirrored-library-busybox 1.36.1 \
    sha256:8a45424ddf949bbe9bb3231b05f9032a45da5cd036eb4867b511b00734756d6f
fi

if ! docker exec "$node" test -e "$backing_file"; then
  docker run --rm --network none --volumes-from "$node" --entrypoint /bin/sh "$utility_image" -ec '
    if [[ ! -e "$1" ]]; then
      staging=$(mktemp "$1.XXXXXX")
      trap "rm -f \"$staging\"" EXIT
      truncate -s 120G "$staging"
      mkfs.xfs -q "$staging"
      mv -n "$staging" "$1"
    fi
  ' -- "$backing_file"
fi
docker exec "$node" sh -ec '
  mkdir -p "$1"
  if ! mountpoint -q "$1"; then
    test -z "$(ls -A "$1")"
    mount -t xfs -o loop,prjquota "$2" "$1"
  fi
  awk -v path="$1" '"'"'$2 == path && $3 == "xfs" && $4 ~ /(^|,)(prjquota|pquota)(,|$)/ { found = 1 }
      END { exit !found }'"'"' /proc/mounts
' -- "$base_path" "$backing_file"

default_classes() {
  kubectl --context "$context" get storageclasses -o json |
    jq -c '[.items[] | select(.metadata.annotations["storageclass.kubernetes.io/is-default-class"] == "true") | .metadata.name] | sort'
}
before=$(default_classes)
chart="$scratch/localpv-provisioner-4.6.0.tgz"
curl -fsSL https://openebs.github.io/dynamic-localpv-provisioner/localpv-provisioner-4.6.0.tgz -o "$chart"
printf '%s  %s\n' 3e6e0661462abbecf944399e2624b0b22249ac386e5d43473f6c1cc8a4337133 "$chart" | shasum -a 256 -c -
helm upgrade --install talos-localpv "$chart" \
  --kube-context "$context" --namespace talos-storage --create-namespace \
  --set hostpathClass.enabled=false --set analytics.enabled=false --wait --timeout 3m
kubectl --context "$context" apply -f "$directory/talos-xfs-quota.yaml"
after=$(default_classes)
[[ "$before" == "$after" ]] || { echo "Default StorageClass changed" >&2; exit 1; }
printf 'Prepared XFS project quotas on %s; default StorageClass remains %s\n' "$node" "$after"
