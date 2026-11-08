use std::{collections::BTreeMap, fmt::Debug, future::Future, time::Duration};

use k8s_openapi::{
    api::{
        apps::v1::StatefulSet,
        batch::v1::Job,
        core::v1::{PersistentVolumeClaim, Pod, Secret},
    },
    apimachinery::pkg::apis::meta::v1::ObjectMeta,
};
use kube::{
    api::{DeleteParams, Patch, PatchParams, PostParams, Preconditions},
    Api, Client,
};
use serde::de::DeserializeOwned;
use serde_json::json;
use tracing::instrument::WithSubscriber;

use crate::{
    seed, ContainerState, DesiredRuntimeState, HomeCapacity, ManagedWorkerRuntime, RuntimeError,
    RuntimeFuture, RuntimeIdentity, RuntimeObservation, SeedFile, SeedMode, WorkerSpec,
    INSTANCE_LABEL, RESOURCE_LABEL, REVISION_LABEL,
};

mod config;
mod objects;
mod seed_job;

/// Deployment-owned Kubernetes target. The operator installs the node-local seccomp profile
/// and verifies that the StorageClass reclaims deleted volumes.
pub struct KubernetesOptions {
    pub namespace: String,
    pub storage_class: String,
    pub seccomp_profile: String,
}

/// Namespace-scoped Kubernetes adapter. Infrastructure state is independent of Worker RPC.
pub struct Kubernetes {
    options: KubernetesOptions,
    client: Client,
    workers: Api<StatefulSet>,
    pods: Api<Pod>,
    homes: Api<PersistentVolumeClaim>,
    jobs: Api<Job>,
    secrets: Api<Secret>,
}

const DELETE_TIMEOUT: Duration = Duration::from_secs(60);
const STOP_TIMEOUT: Duration = Duration::from_secs(60);

impl Kubernetes {
    fn new(options: KubernetesOptions, client: Client) -> Self {
        let namespace = &options.namespace;
        Self {
            workers: Api::namespaced(client.clone(), namespace),
            pods: Api::namespaced(client.clone(), namespace),
            homes: Api::namespaced(client.clone(), namespace),
            jobs: Api::namespaced(client.clone(), namespace),
            secrets: Api::namespaced(client.clone(), namespace),
            client,
            options,
        }
    }

    async fn ensure_home_files(
        &self,
        spec: &WorkerSpec,
        capacity: HomeCapacity,
        files: &[SeedFile],
        mode: SeedMode,
    ) -> Result<(), RuntimeError> {
        let HomeCapacity::LimitedBytes(bytes) = capacity else {
            return Err(RuntimeError::UnsupportedCapacity);
        };
        let storage = objects::byte_quantity(bytes)?;
        let names = Names::new(&spec.identity)?;
        let paths = seed::validate_paths(files)?;
        if !files.is_empty() {
            spec_is_valid(spec)?;
            self.verify_stopped(spec, &names).await?;
        }
        let home = match self.home(&names).await? {
            Some(home) => home,
            None => {
                let mut claim = self.home_claim(&names.home, &storage);
                claim.metadata.labels = Some(names.labels("home"));
                match kube_request(self.homes.create(&PostParams::default(), &claim)).await {
                    Ok(created) => {
                        self.verify_home(&names, &created)?;
                        created
                    }
                    Err(kube::Error::Api(status)) if status.code == 409 => self
                        .home(&names)
                        .await?
                        .ok_or(RuntimeError::Missing { resource: "Home" })?,
                    Err(error) => return Err(api_error("create Worker Home")(error)),
                }
            }
        };
        require_usable_home(&home)?;
        if home
            .spec
            .as_ref()
            .and_then(|spec| spec.resources.as_ref())
            .and_then(|resources| resources.requests.as_ref())
            .and_then(|requests| requests.get("storage"))
            .is_none_or(|capacity| capacity.0 != storage)
        {
            return Err(RuntimeError::Conflict {
                resource: "Home capacity",
            });
        }
        if !files.is_empty() {
            self.seed_home(spec, &names, files, &paths, mode).await?;
        }
        Ok(())
    }

    async fn ensure_worker(&self, spec: &WorkerSpec) -> Result<(), RuntimeError> {
        spec_is_valid(spec)?;
        let names = Names::new(&spec.identity)?;
        self.require_home(&names).await?;
        if let Some(worker) = self.worker(spec, &names).await? {
            return require_usable_worker(&worker);
        }
        self.require_no_pod(&names).await?;
        let workload = self.worker_workload(spec, &names);
        let worker =
            match kube_request(self.workers.create(&PostParams::default(), &workload)).await {
                Ok(worker) => worker,
                Err(kube::Error::Api(status)) if status.code == 409 => {
                    kube_request(self.workers.get(&names.worker))
                        .await
                        .map_err(api_error("read existing Worker workload"))?
                }
                Err(error) => return Err(api_error("create Worker workload")(error)),
            };
        self.verify_worker(spec, &names, &worker)
    }

    async fn change_running(
        &self,
        spec: &WorkerSpec,
        desired: DesiredRuntimeState,
    ) -> Result<(), RuntimeError> {
        let names = Names::new(&spec.identity)?;
        let Some(worker) = self.worker(spec, &names).await? else {
            self.require_no_pod(&names).await?;
            return match desired {
                DesiredRuntimeState::Stopped => Ok(()),
                DesiredRuntimeState::Running => Err(RuntimeError::Missing { resource: "Worker" }),
            };
        };
        let uid = resource_uid(&worker.metadata, "Worker workload")?;
        self.worker_pod(spec, &names, uid).await?;
        if desired == DesiredRuntimeState::Running {
            require_usable_worker(&worker)?;
            self.require_home(&names).await?;
        } else {
            self.home(&names).await?;
        }
        let replicas = i32::from(desired == DesiredRuntimeState::Running);
        if worker_replicas(&worker) != Some(replicas) {
            let resource_version = worker.metadata.resource_version.as_deref().ok_or(
                RuntimeError::UnexpectedState {
                    resource: "Worker workload resource version",
                },
            )?;
            let patch = json!({
                "metadata": { "resourceVersion": resource_version },
                "spec": { "replicas": replicas },
            });
            kube_request(self.workers.patch(
                &names.worker,
                &PatchParams::default(),
                &Patch::Merge(&patch),
            ))
            .await
            .map_err(api_error("scale Worker workload"))?;
        }
        if desired == DesiredRuntimeState::Stopped {
            wait_for("Worker Pod termination", STOP_TIMEOUT, || async {
                let Some(current) = self.worker(spec, &names).await? else {
                    self.require_no_pod(&names).await?;
                    return Ok(true);
                };
                if current.metadata.uid.as_deref() != Some(uid)
                    || worker_replicas(&current) != Some(0)
                {
                    return Err(RuntimeError::Conflict {
                        resource: "Worker workload",
                    });
                }
                Ok(workload_has_scaled_down(&current)
                    && self.worker_pod(spec, &names, uid).await?.is_none())
            })
            .await?;
        }
        Ok(())
    }

    async fn observe(&self, spec: &WorkerSpec) -> Result<RuntimeObservation, RuntimeError> {
        let names = Names::new(&spec.identity)?;
        let home_exists = self.home(&names).await?.is_some();
        let container = if let Some(worker) = self.worker(spec, &names).await? {
            let uid = resource_uid(&worker.metadata, "Worker workload")?;
            match self.worker_pod(spec, &names, uid).await? {
                Some(pod) if worker_container_running(&pod) => ContainerState::Running,
                _ => ContainerState::Stopped,
            }
        } else {
            self.require_no_pod(&names).await?;
            ContainerState::Missing
        };
        Ok(RuntimeObservation {
            container,
            home_exists,
        })
    }

    async fn verify_stopped(
        &self,
        spec: &WorkerSpec,
        names: &Names,
    ) -> Result<Option<StatefulSet>, RuntimeError> {
        let worker = self.worker(spec, names).await?;
        if let Some(worker) = &worker {
            if worker_replicas(worker) != Some(0)
                || !workload_has_scaled_down(worker)
                || self
                    .worker_pod(
                        spec,
                        names,
                        resource_uid(&worker.metadata, "Worker workload")?,
                    )
                    .await?
                    .is_some()
            {
                return Err(RuntimeError::Conflict {
                    resource: "active Worker workload",
                });
            }
        } else {
            self.require_no_pod(names).await?;
        }
        Ok(worker)
    }

    async fn remove_worker(&self, spec: &WorkerSpec) -> Result<(), RuntimeError> {
        let names = Names::new(&spec.identity)?;
        if let Some(worker) = self.verify_stopped(spec, &names).await? {
            delete_owned(
                &self.workers,
                &names.worker,
                &worker.metadata,
                "Worker workload",
            )
            .await?;
            self.require_no_pod(&names).await?;
        }
        Ok(())
    }

    async fn delete_home(&self, identity: &RuntimeIdentity) -> Result<(), RuntimeError> {
        let names = Names::new(identity)?;
        if kube_request(self.workers.get_metadata_opt(&names.worker))
            .await
            .map_err(api_error("inspect Worker workload"))?
            .is_some()
        {
            return Err(RuntimeError::Conflict {
                resource: "Worker workload",
            });
        }
        self.require_no_pod(&names).await?;
        let home = self.home(&names).await?;
        self.remove_seeder(&names).await?;
        if let Some(home) = home {
            delete_owned(&self.homes, &names.home, &home.metadata, "Worker Home PVC").await?;
        }
        Ok(())
    }

    async fn home(&self, names: &Names) -> Result<Option<PersistentVolumeClaim>, RuntimeError> {
        let home = kube_request(self.homes.get_opt(&names.home))
            .await
            .map_err(api_error("inspect Worker Home"))?;
        if let Some(home) = &home {
            self.verify_home(names, home)?;
        }
        Ok(home)
    }

    async fn require_home(&self, names: &Names) -> Result<(), RuntimeError> {
        let home = self
            .home(names)
            .await?
            .ok_or(RuntimeError::Missing { resource: "Home" })?;
        require_usable_home(&home)
    }

    async fn worker(
        &self,
        spec: &WorkerSpec,
        names: &Names,
    ) -> Result<Option<StatefulSet>, RuntimeError> {
        let worker = kube_request(self.workers.get_opt(&names.worker))
            .await
            .map_err(api_error("inspect Worker workload"))?;
        if let Some(worker) = &worker {
            self.verify_worker(spec, names, worker)?;
        }
        Ok(worker)
    }

    async fn worker_pod(
        &self,
        spec: &WorkerSpec,
        names: &Names,
        worker_uid: &str,
    ) -> Result<Option<Pod>, RuntimeError> {
        let pod = kube_request(self.pods.get_opt(&names.pod))
            .await
            .map_err(api_error("inspect Worker Pod"))?;
        if let Some(pod) = &pod {
            names.verify_labels(&pod.metadata, "worker")?;
            if !pod
                .metadata
                .owner_references
                .as_ref()
                .is_some_and(|owners| {
                    owners
                        .iter()
                        .any(|owner| owner.controller == Some(true) && owner.uid == worker_uid)
                })
                || !has_revision(&pod.metadata, spec.revision)
            {
                return Err(RuntimeError::Conflict {
                    resource: "Worker Pod ownership",
                });
            }
        }
        Ok(pod)
    }

    async fn require_no_pod(&self, names: &Names) -> Result<(), RuntimeError> {
        if kube_request(self.pods.get_metadata_opt(&names.pod))
            .await
            .map_err(api_error("inspect Worker Pod"))?
            .is_some()
        {
            return Err(RuntimeError::Conflict {
                resource: "Worker Pod",
            });
        }
        Ok(())
    }
}

impl ManagedWorkerRuntime for Kubernetes {
    fn ensure_home<'a>(
        &'a self,
        spec: &'a WorkerSpec,
        capacity: HomeCapacity,
        files: &'a [SeedFile],
        mode: SeedMode,
    ) -> RuntimeFuture<'a, ()> {
        Box::pin(self.ensure_home_files(spec, capacity, files, mode))
    }

    fn ensure_runtime<'a>(&'a self, spec: &'a WorkerSpec) -> RuntimeFuture<'a, ()> {
        Box::pin(self.ensure_worker(spec))
    }

    fn set_running<'a>(
        &'a self,
        spec: &'a WorkerSpec,
        desired: DesiredRuntimeState,
    ) -> RuntimeFuture<'a, ()> {
        Box::pin(self.change_running(spec, desired))
    }

    fn inspect<'a>(&'a self, spec: &'a WorkerSpec) -> RuntimeFuture<'a, RuntimeObservation> {
        Box::pin(self.observe(spec))
    }

    fn remove_runtime<'a>(&'a self, spec: &'a WorkerSpec) -> RuntimeFuture<'a, ()> {
        Box::pin(self.remove_worker(spec))
    }

    fn remove_home<'a>(&'a self, identity: &'a RuntimeIdentity) -> RuntimeFuture<'a, ()> {
        Box::pin(self.delete_home(identity))
    }
}

struct Names {
    instance: String,
    worker: String,
    pod: String,
    home: String,
    job: String,
    secret: String,
}

impl Names {
    fn new(identity: &RuntimeIdentity) -> Result<Self, RuntimeError> {
        let worker = identity.resource_name()?;
        Ok(Self {
            instance: identity.instance_id.to_string(),
            pod: format!("{worker}-0"),
            home: format!("{worker}-home"),
            job: format!("{worker}-seed"),
            secret: format!("{worker}-seed-files"),
            worker,
        })
    }

    fn labels(&self, resource: &str) -> BTreeMap<String, String> {
        BTreeMap::from([
            (INSTANCE_LABEL.into(), self.instance.clone()),
            (RESOURCE_LABEL.into(), resource.into()),
        ])
    }

    fn verify_labels(
        &self,
        metadata: &ObjectMeta,
        resource: &'static str,
    ) -> Result<(), RuntimeError> {
        if metadata.labels.as_ref().is_some_and(|labels| {
            labels.get(INSTANCE_LABEL) == Some(&self.instance)
                && labels
                    .get(RESOURCE_LABEL)
                    .is_some_and(|value| value == resource)
        }) {
            Ok(())
        } else {
            Err(RuntimeError::Conflict { resource })
        }
    }
}

fn has_revision(metadata: &ObjectMeta, revision: u64) -> bool {
    metadata
        .labels
        .as_ref()
        .and_then(|labels| labels.get(REVISION_LABEL))
        == Some(&revision.to_string())
}

fn spec_is_valid(spec: &WorkerSpec) -> Result<(), RuntimeError> {
    if spec.image.is_empty() {
        return Err(RuntimeError::InvalidSpec("missing Worker image"));
    }
    if spec.cpu_cores == 0 || spec.memory_bytes == 0 || i64::try_from(spec.memory_bytes).is_err() {
        return Err(RuntimeError::InvalidSpec("invalid Worker resource limits"));
    }
    Ok(())
}

fn require_usable_worker(worker: &StatefulSet) -> Result<(), RuntimeError> {
    if worker.metadata.deletion_timestamp.is_some() {
        return Err(RuntimeError::UnexpectedState {
            resource: "Worker workload",
        });
    }
    Ok(())
}

fn worker_replicas(worker: &StatefulSet) -> Option<i32> {
    worker.spec.as_ref().and_then(|spec| spec.replicas)
}

fn workload_has_scaled_down(worker: &StatefulSet) -> bool {
    worker.status.as_ref().is_some_and(|status| {
        status
            .observed_generation
            .zip(worker.metadata.generation)
            .is_some_and(|(observed, desired)| observed >= desired)
            && status.replicas == 0
    })
}

fn worker_container_running(pod: &Pod) -> bool {
    pod.metadata.deletion_timestamp.is_none()
        && pod
            .status
            .as_ref()
            .and_then(|status| status.container_statuses.as_ref())
            .and_then(|containers| {
                containers
                    .iter()
                    .find(|container| container.name == "worker")
            })
            .and_then(|container| container.state.as_ref())
            .is_some_and(|state| state.running.is_some())
}

fn require_usable_home(home: &PersistentVolumeClaim) -> Result<(), RuntimeError> {
    if home.metadata.deletion_timestamp.is_some()
        || home
            .status
            .as_ref()
            .and_then(|status| status.phase.as_deref())
            == Some("Lost")
    {
        return Err(RuntimeError::UnexpectedState { resource: "Home" });
    }
    Ok(())
}

fn resource_uid<'a>(
    metadata: &'a ObjectMeta,
    resource: &'static str,
) -> Result<&'a str, RuntimeError> {
    metadata
        .uid
        .as_deref()
        .ok_or(RuntimeError::UnexpectedState { resource })
}

async fn delete_owned<K>(
    api: &Api<K>,
    name: &str,
    metadata: &ObjectMeta,
    resource: &'static str,
) -> Result<(), RuntimeError>
where
    K: Clone + DeserializeOwned + Debug,
{
    let uid = resource_uid(metadata, resource)?;
    if metadata.deletion_timestamp.is_none() {
        let delete = DeleteParams {
            preconditions: Some(Preconditions {
                uid: Some(uid.into()),
                resource_version: None,
            }),
            ..DeleteParams::foreground()
        };
        match kube_request(api.delete(name, &delete)).await {
            Ok(_) => {}
            Err(kube::Error::Api(status)) if status.code == 404 => return Ok(()),
            Err(error) => return Err(api_error("delete owned Worker resource")(error)),
        }
    }
    wait_for(resource, DELETE_TIMEOUT, || async {
        let current = kube_request(api.get_metadata_opt(name))
            .await
            .map_err(api_error("confirm owned Worker resource deletion"))?;
        match current {
            Some(current) if current.metadata.uid.as_deref() != Some(uid) => {
                Err(RuntimeError::Conflict { resource })
            }
            Some(_) => Ok(false),
            None => Ok(true),
        }
    })
    .await
}

async fn wait_for<F, Fut>(
    resource: &'static str,
    timeout: Duration,
    mut check: F,
) -> Result<(), RuntimeError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<bool, RuntimeError>>,
{
    tokio::time::timeout(timeout, async {
        while !check().await? {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Ok(())
    })
    .await
    .map_err(|_| RuntimeError::UnexpectedState { resource })?
}

#[derive(Debug, thiserror::Error)]
enum ApiFailure {
    #[error("Kubernetes API returned HTTP {0}")]
    Status(u16),
    #[error("Kubernetes API request failed")]
    Request,
}

// API responses can echo input. Keep only safe diagnostics, including for Secret operations.
fn api_error(operation: &'static str) -> impl FnOnce(kube::Error) -> RuntimeError {
    move |error| RuntimeError::Kubernetes {
        operation,
        source: Box::new(match error {
            kube::Error::Api(status) => ApiFailure::Status(status.code),
            _ => ApiFailure::Request,
        }),
    }
}

// kube-client can log undecodable response bodies before returning an error. Disable its
// subscriber while polling requests that may contain credential or seed-file material.
fn kube_request<F: Future>(future: F) -> impl Future<Output = F::Output> {
    future.with_subscriber(tracing::Dispatch::none())
}
