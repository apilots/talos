# Kubernetes deployment

This deployment runs one authenticated Talos Server in namespace `talos` with
a persistent Home and a namespace-scoped ServiceAccount. The `talos-server`
ClusterIP Service has HTTP port 8080 and outbound Worker registration port
9443. Managed Talo workloads and their claims are created by the Server.
The separate manual Worker example uses an explicitly issued Computer
credential and has no Service.

Workers use the Server Service DNS inside the cluster. For local k3d access,
forward the Server HTTP Service to the workstation loopback as shown below.
For public access, reuse the cluster's existing ingress with the Server
routing described in the [Cloud deployment guide](../TALOS_CLOUD_DEPLOY.md#local-access-and-public-ingress).

## Quick start with local k3d

Use a disposable single-node k3d cluster to try this deployment locally.
Install Docker, [k3d](https://k3d.io/v5.6.3/#installation), and [kubectl
compatible with Kubernetes 1.28](https://kubernetes.io/releases/version-skew-policy/#kubectl). Run from the repository root in a dedicated
terminal and choose a cluster name that is unused in `k3d cluster list`. The
image below matches the local acceptance environment.

```bash
export K3D_CLUSTER=talos-local
export KUBE_CONTEXT="k3d-$K3D_CLUSTER"
k3d cluster create "$K3D_CLUSTER" --servers 1 --agents 0 \
  --image rancher/k3s:v1.28.8-k3s1 --api-port 127.0.0.1:6550 --no-lb \
  --k3s-arg '--disable=traefik@server:0' \
  --kubeconfig-update-default=false --kubeconfig-switch-context=false
```

After the cluster is created successfully, save a dedicated kubeconfig and
create the `talos` namespace for the storage probe and Server.

```bash
export KUBECONFIG="$(mktemp "${TMPDIR:-/tmp}/talos-kubeconfig.XXXXXX")"
k3d kubeconfig get "$K3D_CLUSTER" > "$KUBECONFIG"
kubectl --context "$KUBE_CONTEXT" wait --for=condition=Ready nodes --all --timeout=120s
kubectl --context "$KUBE_CONTEXT" apply -f deploy/kubernetes/namespace.yaml
```

The dedicated kubeconfig keeps your existing default context intact. If
`6550` is in use, choose another loopback port for the Kubernetes API. The
Talos HTTP endpoint remains `127.0.0.1:8080`.
Follow the [disposable k3d profile](#disposable-k3d-profile) to prepare the
node's seccomp profile and quota-backed storage, then [deploy Server](#deploy-server).
Workers use `https://talos-server.talos.svc:9443`. Forward the Server HTTP
Service to `127.0.0.1:8080` for Web and Electron as shown in the deployment
steps. These addresses remain the same across Server Pod restarts. If a
restart ends the port-forward process, rerun the same command.

When the cluster's data is disposable, run `k3d cluster delete "$K3D_CLUSTER"`
in the same terminal with the dedicated kubeconfig still set. Then remove
that kubeconfig.

## Cluster preparation

Use a cluster with NetworkPolicy enforcement and filesystem storage writable
by UID/GID 10001. Deploy Talos into a dedicated namespace. The Server Role
creates and reads bootstrap Secrets in that namespace, so keep unrelated
credentials and workloads outside it. The managed Worker policy denies
ingress to managed Worker Pods, and seed Pods have neither ingress nor
egress. Keep this ingress policy effective when adding other network policies.
Workers can reach the Server registration Service and their own Provider
and Browser endpoints.
After applying the policies, run
`deploy/kubernetes/check-network-policy.sh "$KUBE_CONTEXT"` on a disposable
deployment to check the actual packet path between Pods.

On every node eligible to run Talos, render the Chromium seccomp profile for
that node's architecture and kernel. The renderer translates the existing
Docker profile to OCI format for workloads that drop all capabilities.
Chromium can use `chroot` inside its private user namespace, where the
kernel applies its capability check.

```bash
deploy/kubernetes/render-chromium-seccomp.sh > chromium-seccomp.json
sudo install -D -m 0644 chromium-seccomp.json /var/lib/kubelet/seccomp/talos/chromium-seccomp.json
```

Use the actual kubelet root if it differs from `/var/lib/kubelet`. If rendering
off-node, pass the node architecture and kernel version as arguments.
Linux AMD64 and ARM64 are supported.

Choose a named dynamic StorageClass whose `Delete` policy removes the volume
and its backing data. Each managed Talo requests `100G` on that class.
The [OpenEBS profile](storage/talos-xfs-quota.yaml) is an example of a
quota-enforcing class. It requires the OpenEBS Dynamic LocalPV Hostpath
provisioner 4.6.0 and XFS with project quotas enabled on every eligible node
at its configured BasePath. Local volumes remain attached to the node
holding their data. Use a production-managed filesystem and node placement
appropriate for that profile. If OpenEBS is already installed, use the
existing compatible provisioner.

Set `home_quota_enforced: true` only after a real write-over-limit probe
passes on the configured class. With a capacity-only provisioner such as K3s
`local-path`, leave it false. Server and manual Worker Home claims use the
cluster default unless an operator overlay explicitly names another class.

## Deploy Server

1. Copy [server.talos.yaml.example](server.talos.yaml.example) to a private
   working directory as `talos.yaml`. Set the Worker image digest and
   StorageClass. Add deployment-specific Cloud settings as required.
2. Issue a TLS certificate and key with DNS SAN `talos-server.talos.svc`.
   Use the same name in the runtime endpoint and supply the issuing CA bundle.
3. Create the namespace and operator-owned config and TLS resources. Set an
   explicit kubectl context and the private input directory. Run from the
   repository root.

   ```bash
   kube=(kubectl --context "$KUBE_CONTEXT" --namespace talos)
   "${kube[@]}" apply -f deploy/kubernetes/namespace.yaml
   "${kube[@]}" create configmap talos-server-bootstrap --from-file=talos.yaml="$INPUT_DIR/talos.yaml" \
     --dry-run=client -o yaml | "${kube[@]}" apply -f -
   "${kube[@]}" create secret generic talos-server-tls --type=kubernetes.io/tls \
     --from-file=tls.crt="$INPUT_DIR/tls.crt" --from-file=tls.key="$INPUT_DIR/tls.key" \
     --from-file=ca.crt="$INPUT_DIR/ca.crt" \
     --dry-run=client -o yaml | "${kube[@]}" apply -f -
   ```

4. Build the standard Server/Worker OCI images from the same version or
   select a published version containing Kubernetes Runtime support.
   Create an operator Kustomize overlay that references
   [the base](base/kustomization.yaml) and sets an immutable Server image.

   ```yaml
   apiVersion: kustomize.config.k8s.io/v1beta1
   kind: Kustomization
   resources:
     - ../base
   images:
     - name: ghcr.io/apilots/talos-server
       digest: sha256:<server-image-digest>
   ```

   Place this overlay next to `base` or adjust its relative reference.
   Set the Server Home StorageClass and capacity in the overlay if needed.
   Apply it with `"${kube[@]}" apply -k "$OVERLAY"`.

5. Verify namespace permissions, wait for rollout, and forward HTTP locally.

   ```bash
   deploy/kubernetes/check-runtime-access.sh "$KUBE_CONTEXT" talos-xfs-quota
   "${kube[@]}" rollout status deployment/talos-server --timeout=180s
   "${kube[@]}" port-forward --address 127.0.0.1 service/talos-server 8080:8080
   ```

   Check `http://127.0.0.1:8080/health` and open the Web UI or start native
   Desktop with `TALOS_DESKTOP_SERVER_URL=http://127.0.0.1:8080`. Sign in,
   initialize Talo, and verify the Computer becomes available. HTTP health
   only establishes Server liveness. The product readiness boundary is
   outbound registration and publication.

The Server init container copies `talos.yaml` into its Home once. Subsequent
changes belong to the persistent config, including **Settings → Runtime**.
Keep Provider credentials in the existing private runtime environment or in
operator-managed Secrets. Avoid putting keys in the bootstrap ConfigMap.

### Mounted kubeconfig

In-cluster ServiceAccount authentication is the default. To use an
operator-managed kubeconfig, add a Secret volume mounted read-only at
`/var/run/secrets/talos-kubernetes` in the Server container. Keep it separate
from the Worker TLS Secret.

```bash
"${kube[@]}" create secret generic talos-server-kubeconfig --from-file=kubeconfig
```

Add this Deployment patch to the operator overlay.

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: talos-server
spec:
  template:
    spec:
      containers:
        - name: server
          volumeMounts:
            - name: kubeconfig
              mountPath: /var/run/secrets/talos-kubernetes
              readOnly: true
      volumes:
        - name: kubeconfig
          secret:
            secretName: talos-server-kubeconfig
            defaultMode: 0440
```

Set the following under
`talos.worker.runtime.kubernetes` in the initial or persisted configuration.

```yaml
kubeconfig_path: /var/run/secrets/talos-kubernetes/kubeconfig
# kubeconfig_context: talos
```

Set `kubeconfig_source: InCluster` if staging the mounted reference for
selection from Runtime Settings later.
Use a scoped credential with the same namespace permissions as the Role and
an API endpoint reachable and trusted from the Server Pod. The local
operator's kubectl context is for deployment and preflight. Administrators
can also paste a self-contained kubeconfig into Runtime Settings. The Server
persists the credential privately in its Home and the ordinary settings
store retains its reference. Follow the controlled Server restart indicated
in Settings.

## Deploy a manually registered Worker

Sign in and register a personal Computer using the authenticated Server API.
Put its exact UUID in `node-id` and its credential in `node.credential` in
a private working directory. Copy
[the Worker config](manual-worker/talos.yaml.example) as `talos.yaml`.
Retain the `${TALOS_WORKER_NODE_ID}` placeholder and use the same CA bundle
as for the Server endpoint.

```bash
chmod 0600 node.credential
"${kube[@]}" create secret generic talos-personal-worker-bootstrap \
  --from-file=node-id --from-file=node.credential --from-file=ca.crt \
  --from-file=talos.yaml --dry-run=client -o yaml | "${kube[@]}" apply -f -
```

Use [the manual Worker base](manual-worker/kustomization.yaml) in an operator
overlay with `ghcr.io/apilots/talos-worker` pinned by digest, then apply it.
Wait for the Worker to appear available in the Computer list. For a replacement
credential, update the Secret and restart the manual Worker deployment.
Its init container refreshes the credential and CA while preserving the
existing Home and its configuration. Ordinary Computer removal affects the
registration. The deployment and Home remain under operator control.

## Disposable k3d profile

For a single-node k3d cluster, explicitly prepare the node and install the
pinned upstream storage provisioner. The command creates a 120 GiB sparse
backing file and mounts its XFS filesystem with project quotas. It adds only
`talos-xfs-quota` and preserves existing default StorageClasses.
The preparation command uses Docker, k3d, kubectl, Helm, jq, curl and shasum.

```bash
deploy/kubernetes/storage/k3d-prepare.sh "$K3D_CLUSTER"
deploy/kubernetes/storage/check-quota.sh "k3d-$K3D_CLUSTER" talos-xfs-quota
```

The probe writes 4 MiB, checks that the filesystem rejects writing 16 MiB
on an 8 MiB claim, and waits for the dynamically provisioned PV to disappear
after deleting the claim. Rerun node preparation after a k3d node restart
before using the quota volume. Its sparse file persists in the k3d node's
storage volume. Back up real data before removing any storage infrastructure.

The [k3d overlay](overlays/k3d/kustomization.yaml) uses `local-path` for the
small disposable Server Home, with the Talo class still set in Server YAML.
For local builds, import each exact image into the node and use the resulting
digest reference in the Server overlay and Worker runtime configuration.

```bash
deploy/kubernetes/overlays/k3d/import-image.sh "$K3D_CLUSTER" "$SERVER_IMAGE" server
deploy/kubernetes/overlays/k3d/import-image.sh "$K3D_CLUSTER" "$WORKER_IMAGE" worker
```

See the [local managed Talo E2E group](../../talos-e2e/scenarios/25_managed_talo/README.md)
for fixtures and reusable acceptance commands.

## Cleanup

Delete owned Talo instances through the product with data-loss confirmation
and verify their Pods and claims are gone before removing the Server.

```bash
"${kube[@]}" delete deployment talos-server talos-personal-worker --ignore-not-found
"${kube[@]}" delete service talos-server
"${kube[@]}" delete networkpolicy talos-worker-no-ingress talos-seeder-isolation
"${kube[@]}" delete role,rolebinding,serviceaccount talos-server
"${kube[@]}" delete configmap talos-server-bootstrap
"${kube[@]}" delete secret talos-server-tls talos-server-kubeconfig talos-personal-worker-bootstrap --ignore-not-found
```

Delete the named Home PVCs only when their persisted data is intentionally
disposable. Keep the namespace and other workloads.

To remove the local storage profile after all of its volumes are deleted,
delete the `talos-xfs-quota` StorageClass and uninstall Helm release
`talos-localpv` from `talos-storage` using the same context. Unmount the
dedicated node path `/var/lib/rancher/k3s/talos-quota` before removing the
sparse backing file at `/var/lib/rancher/k3s/talos-quota.xfs`.

[OpenEBS quota configuration](https://openebs.io/docs/main/user-guides/local-storage-user-guide/local-pv-hostpath/configuration/hostpath-storageclass-parameters) ·
[Kubernetes seccomp profiles](https://kubernetes.io/docs/reference/node/seccomp/)
