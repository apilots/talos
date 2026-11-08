# Managed Talo local acceptance

This group runs against a disposable local Docker Engine or a designated k3d
cluster and an installed native Electron shell. The hosted PR workflow retains
the representative Docker case in `23_browser_container`; the full topology
and fault matrix runs here.

| Case | Coverage |
| --- | --- |
| `01_docker_lifecycle` | Docker Talo ownership, initialization, ordinary Home, lifecycle, desktop control and real Worker-local Turn. Personal Worker desktop and Turn, registration-only removal and retained Home. |
| `02_docker_recovery_desktop` | Native Desktop over the Server relay, Worker loss and reconnect, delayed queue dispatch, both Wake deadlines, offline Reset and data retention. |
| `03_kubernetes_preflight` | Checked-in deployment, namespace permissions, admin Runtime configuration and storage quota enforcement. |
| `04_kubernetes_lifecycle` | Kubernetes Talo lifecycle, same PVC across compute changes, real desktop and Turn, and the same personal Worker capability and registration checks. |
| `05_kubernetes_recovery_desktop` | The same native Desktop and fault matrix against a Kubernetes Server and Talo. |

Prepare the current Server and Worker OCI images from the existing Dockerfiles.
Use the image variables from [the packaged Browser group](../23_browser_container/README.md).
Install the pinned Browser fixture dependencies and Python Playwright. Native
cases use the Electron binary installed by the Desktop workspace, or an
explicit `TALOS_E2E_ELECTRON` path. Playwright attaches to a disposable Electron
profile using CDP and does not install a separate browser.

For Kubernetes prepare the designated `talos` namespace and StorageClass using
[the Kubernetes deployment instructions](../../../deploy/kubernetes/README.md).
Use the name returned by `k3d cluster list`, without the `k3d-` context prefix.

```sh
TALOS_E2E_K3D_CLUSTER=mycluster just e2e --group 25_managed_talo --check
TALOS_E2E_K3D_CLUSTER=mycluster just e2e-group 25_managed_talo
just e2e-case 25_managed_talo/02_docker_recovery_desktop
```

The runner creates per-case evidence and checks owned-resource cleanup. The
fault cases deliberately hold registration through the real operation timeout.
