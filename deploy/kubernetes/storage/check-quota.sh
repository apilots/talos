#!/usr/bin/env bash
set -euo pipefail

context=${1:?Usage: check-quota.sh KUBE_CONTEXT STORAGE_CLASS}
storage_class=${2:?Usage: check-quota.sh KUBE_CONTEXT STORAGE_CLASS}
[[ "$storage_class" =~ ^[a-z0-9][a-z0-9.-]*$ ]]
kube=(kubectl --context "$context" --namespace talos)
probe="talos-quota-probe-$(openssl rand -hex 5)"
image=docker.io/openebs/linux-utils:4.6.0
cleanup() {
  "${kube[@]}" delete pod "$probe" --ignore-not-found --timeout=120s
  "${kube[@]}" delete pvc "$probe" --ignore-not-found --timeout=120s
}
trap cleanup EXIT

"${kube[@]}" create -f - <<MANIFEST
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: $probe
spec:
  accessModes: [ReadWriteOnce]
  storageClassName: $storage_class
  resources:
    requests:
      storage: 8Mi
---
apiVersion: v1
kind: Pod
metadata:
  name: $probe
spec:
  restartPolicy: Never
  automountServiceAccountToken: false
  securityContext:
    runAsNonRoot: true
    runAsUser: 10001
    runAsGroup: 10001
    fsGroup: 10001
    seccompProfile:
      type: RuntimeDefault
  containers:
    - name: write-probe
      image: $image
      command: [/bin/sh, -ec]
      args:
        - |
          export LC_ALL=C
          dd if=/dev/zero of=/data/below bs=1M count=4 conv=fsync
          if dd if=/dev/zero of=/data/above bs=1M count=16 conv=fsync 2>/tmp/quota-error; then
            echo "16 MiB write succeeded on an 8 MiB claim" >&2
            exit 1
          fi
          cat /tmp/quota-error
          grep -Eq 'Disk quota exceeded|No space left on device' /tmp/quota-error
          test "\$(stat -c %s /data/below)" -eq 4194304
          test "\$(stat -c %s /data/above)" -eq 4194304
          echo 'UID 10001 wrote 4 MiB and hit the 8 MiB filesystem hard quota'
      securityContext:
        allowPrivilegeEscalation: false
        capabilities:
          drop: [ALL]
      volumeMounts:
        - name: quota
          mountPath: /data
  volumes:
    - name: quota
      persistentVolumeClaim:
        claimName: $probe
MANIFEST

for ((attempt=0; attempt<90; attempt++)); do
  phase=$("${kube[@]}" get pod "$probe" -o jsonpath='{.status.phase}')
  case "$phase" in
    Succeeded|Failed) break ;;
  esac
  sleep 2
done
if [[ "$phase" != Succeeded ]]; then
  "${kube[@]}" describe pod "$probe" >&2
  "${kube[@]}" logs "$probe" >&2 || true
  exit 1
fi
"${kube[@]}" logs "$probe"
pv=$("${kube[@]}" get pvc "$probe" -o jsonpath='{.spec.volumeName}')
[[ -n "$pv" ]]
"${kube[@]}" get pv "$pv" -o custom-columns=NAME:.metadata.name,CAPACITY:.spec.capacity.storage,RECLAIM:.spec.persistentVolumeReclaimPolicy
[[ $("${kube[@]}" get pv "$pv" -o jsonpath='{.spec.persistentVolumeReclaimPolicy}') == Delete ]]
cleanup
trap - EXIT
"${kube[@]}" wait --for=delete "pv/$pv" --timeout=120s
if [[ "$context" == k3d-* && "$storage_class" == talos-xfs-quota ]]; then
  docker exec "$context-server-0" test ! -e "/var/lib/rancher/k3s/talos-quota/$pv"
  printf 'Backing directory removed from the XFS quota filesystem\n'
fi
printf 'Deleted disposable quota claim and its dynamically provisioned PV %s\n' "$pv"
