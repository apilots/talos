#!/usr/bin/env bash
set -euo pipefail

context=${1:?Usage: check-network-policy.sh KUBE_CONTEXT}
kube=(kubectl --context "$context" --namespace talos)
image=docker.io/rancher/mirrored-library-busybox:1.36.1
prefix="talos-netpolicy-$(openssl rand -hex 5)"
source="$prefix-source"
worker="$prefix-worker"
seeder="$prefix-seeder"
cleanup() {
  "${kube[@]}" delete pod "$source" "$worker" "$seeder" --ignore-not-found --timeout=90s
}
trap cleanup EXIT

for name in "$source" "$worker" "$seeder"; do
  "${kube[@]}" create -f - <<MANIFEST
apiVersion: v1
kind: Pod
metadata:
  name: $name
spec:
  restartPolicy: Never
  automountServiceAccountToken: false
  securityContext:
    runAsNonRoot: true
    runAsUser: 10001
    seccompProfile:
      type: RuntimeDefault
  containers:
    - name: listener
      image: $image
      command: [sh, -ec]
      args:
        - |
          mkdir /tmp/probe
          printf live >/tmp/probe/index.html
          exec httpd -f -p 18081 -h /tmp/probe
      readinessProbe:
        httpGet:
          path: /
          port: 18081
      securityContext:
        allowPrivilegeEscalation: false
        capabilities:
          drop: [ALL]
MANIFEST
done
"${kube[@]}" wait --for=condition=Ready "pod/$source" "pod/$worker" "pod/$seeder" --timeout=90s

ip_of() {
  "${kube[@]}" get pod "$1" -o jsonpath='{.status.podIP}'
}

read_listener() {
  "${kube[@]}" exec "$1" -- wget -T 2 -O - "http://$2:18081/"
}

wait_for_denial() {
  local result
  for ((attempt=0; attempt<20; attempt++)); do
    if ! result=$(read_listener "$1" "$2" 2>&1); then
      if grep -Eqi 'wget:.*(timed out|connection refused|connection reset)' <<<"$result"; then
        return 0
      fi
      printf '%s\n' "$result" >&2
      return 1
    fi
    sleep 2
  done
  echo "NetworkPolicy allowed access from $1 to $2" >&2
  return 1
}

for role in worker seeder; do
  if [[ "$role" == worker ]]; then target=$worker; else target=$seeder; fi
  ip=$(ip_of "$target")
  read_listener "$source" "$ip" | grep -Fx live
  "${kube[@]}" label "pod/$target" "io.talos.managed-worker.resource=$role"
  wait_for_denial "$source" "$ip"
  read_listener "$target" 127.0.0.1 | grep -Fx live
  printf 'Peer access denied to live %s listener\n' "$role"
done
wait_for_denial "$seeder" "$(ip_of "$source")"
echo 'Seed Pod egress denied to ordinary peer listener'
