#!/usr/bin/env bash
set -euo pipefail

context=${1:?Usage: check-runtime-access.sh KUBE_CONTEXT STORAGE_CLASS}
storage_class=${2:?Usage: check-runtime-access.sh KUBE_CONTEXT STORAGE_CLASS}
namespace=talos
identity=system:serviceaccount:talos:talos-server
kube=(kubectl --context "$context")
as_server=("${kube[@]}" --as "$identity" --as-group system:serviceaccounts
  --as-group system:serviceaccounts:talos --as-group system:authenticated)

if [[ $("${kube[@]}" get storageclass "$storage_class" -o jsonpath='{.reclaimPolicy}') != Delete ]]; then
  echo "Talo storage class $storage_class must reclaim volumes with Delete" >&2
  exit 1
fi

for resource in statefulsets.apps persistentvolumeclaims jobs.batch secrets; do
  for verb in get create delete; do
    "${as_server[@]}" auth can-i "$verb" "$resource" --namespace "$namespace" --quiet
  done
done
"${as_server[@]}" auth can-i patch statefulsets.apps --namespace "$namespace" --quiet
"${as_server[@]}" auth can-i get pods --namespace "$namespace" --quiet
"${as_server[@]}" auth can-i create selfsubjectaccessreviews.authorization.k8s.io --quiet

deny() {
  local result
  result=$("${as_server[@]}" auth can-i "$@") || [[ "$result" == no ]]
  if [[ "$result" != no ]]; then
    echo "Unexpected permission for $identity: $*" >&2
    exit 1
  fi
}
deny create pods --namespace "$namespace"
deny list secrets --namespace "$namespace"
deny get secrets --namespace default
deny get nodes
deny get persistentvolumes
deny get storageclasses.storage.k8s.io

printf 'Namespace runtime permissions and Delete reclaim policy verified for %s\n' "$storage_class"
