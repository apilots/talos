use std::{collections::HashMap, path::Path};

use bollard::{
    errors::Error,
    models::{
        ContainerCreateBody, ContainerInspectResponse, ContainerStateStatusEnum, HostConfig,
        HostConfigLogConfig, Mount, MountType, NetworkCreateRequest, RestartPolicy,
        RestartPolicyNameEnum, Volume, VolumeCreateRequest,
    },
    query_parameters::{
        CreateContainerOptions, CreateImageOptions, RemoveContainerOptions, RemoveVolumeOptions,
        StopContainerOptions,
    },
    Docker,
};
use futures_util::TryStreamExt;
use tokio::sync::Mutex;

use crate::{
    ContainerState, DesiredRuntimeState, HomeCapacity, ManagedWorkerRuntime, RuntimeError,
    RuntimeFuture, RuntimeIdentity, RuntimeObservation, SeedFile, SeedMode, WorkerSpec,
};

mod seed;

pub const DEFAULT_SECCOMP_PROFILE: &str =
    include_str!("../../../../../deploy/docker/talos-worker/chromium-seccomp.json");

const INSTANCE_LABEL: &str = "io.talos.managed-worker.instance";
const RESOURCE_LABEL: &str = "io.talos.managed-worker.resource";
const REVISION_LABEL: &str = "io.talos.managed-worker.revision";
const HOME_PATH: &str = "/home/talos/.talos";
const WORKER_USER: &str = "10001:10001";
const SHM_BYTES: i64 = 1 << 30;
const NO_NEW_PRIVILEGES: &str = "no-new-privileges:true";
const WORKER_NETWORK: &str = "talos-managed-workers";
const BRIDGE_ICC: &str = "com.docker.network.bridge.enable_icc";

/// Same-host Docker Engine adapter. Uses an explicit Unix socket path.
pub struct DockerEngine {
    client: Docker,
    seccomp: String,
    network_creation: Mutex<()>,
}

impl DockerEngine {
    pub fn connect(socket_path: &Path, seccomp_profile: &str) -> Result<Self, RuntimeError> {
        let profile: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(seccomp_profile)
                .map_err(|_| RuntimeError::InvalidSpec("seccomp profile must be a JSON object"))?;
        Ok(Self {
            client: connect_socket(socket_path)?,
            seccomp: format!("seccomp={}", serde_json::Value::Object(profile)),
            network_creation: Mutex::new(()),
        })
    }

    /// Negotiate the daemon API version and validate Linux Engine access before use.
    pub async fn check(&self) -> Result<(), RuntimeError> {
        let client = self
            .client
            .clone()
            .negotiate_version()
            .await
            .map_err(engine_error("negotiate Engine API version"))?;
        let info = client
            .version()
            .await
            .map_err(engine_error("read Engine version"))?;
        if info.os.as_deref() != Some("linux") {
            return Err(RuntimeError::InvalidSpec("Docker Engine must run Linux"));
        }
        Ok(())
    }

    async fn ensure_home_files(
        &self,
        spec: &WorkerSpec,
        capacity: HomeCapacity,
        files: &[SeedFile],
        mode: SeedMode,
    ) -> Result<(), RuntimeError> {
        if capacity != HomeCapacity::Unrestricted {
            return Err(RuntimeError::UnsupportedCapacity);
        }
        let names = Names::new(&spec.identity)?;
        let paths = seed::validate_paths(files)?;
        if !files.is_empty()
            && self
                .inspect_worker(spec, &names)
                .await?
                .is_some_and(|worker| worker.state == ContainerState::Running)
        {
            return Err(RuntimeError::Conflict {
                resource: "running Worker",
            });
        }

        if self.home(&names).await?.is_none() {
            let volume = self
                .client
                .create_volume(VolumeCreateRequest {
                    name: Some(names.home()),
                    driver: Some("local".into()),
                    labels: Some(names.labels("home")),
                    ..Default::default()
                })
                .await
                .map_err(engine_error("create Worker Home volume"))?;
            names.verify_home(&volume)?;
        }

        if !files.is_empty() {
            self.seed_home(spec, &names, files, &paths, mode).await?;
        }
        Ok(())
    }

    async fn ensure_worker(&self, spec: &WorkerSpec) -> Result<(), RuntimeError> {
        let names = Names::new(&spec.identity)?;
        let memory = i64::try_from(spec.memory_bytes)
            .ok()
            .filter(|memory| *memory > 0)
            .ok_or(RuntimeError::InvalidSpec("invalid memory limit"))?;
        if spec.cpu_cores == 0 {
            return Err(RuntimeError::InvalidSpec("invalid CPU limit"));
        }

        if self.home(&names).await?.is_none() {
            return Err(RuntimeError::Missing { resource: "Home" });
        }
        if self.inspect_worker(spec, &names).await?.is_some() {
            return Ok(());
        }

        self.ensure_image(&spec.image).await?;
        let network = self.ensure_network().await?;

        let mut labels = names.labels("worker");
        labels.insert(REVISION_LABEL.into(), spec.revision.to_string());
        self.client
            .create_container(
                Some(CreateContainerOptions {
                    name: Some(names.container.clone()),
                    ..Default::default()
                }),
                ContainerCreateBody {
                    image: Some(spec.image.clone()),
                    user: Some(WORKER_USER.into()),
                    env: Some(vec![
                        format!("HOME={HOME_PATH}"),
                        format!("TALOS_HOME={HOME_PATH}"),
                    ]),
                    labels: Some(labels),
                    host_config: Some(HostConfig {
                        mounts: Some(vec![names.home_mount()]),
                        network_mode: Some(network),
                        memory: Some(memory),
                        memory_swap: Some(memory),
                        nano_cpus: Some(i64::from(spec.cpu_cores) * 1_000_000_000),
                        shm_size: Some(SHM_BYTES),
                        restart_policy: Some(RestartPolicy {
                            name: Some(RestartPolicyNameEnum::UNLESS_STOPPED),
                            ..Default::default()
                        }),
                        security_opt: Some(vec![self.seccomp.clone(), NO_NEW_PRIVILEGES.into()]),
                        log_config: Some(HostConfigLogConfig {
                            typ: Some("json-file".into()),
                            config: Some(HashMap::from([
                                ("max-size".into(), "20m".into()),
                                ("max-file".into(), "5".into()),
                            ])),
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )
            .await
            .map_err(engine_error("create Worker container"))?;
        Ok(())
    }

    async fn change_running(
        &self,
        spec: &WorkerSpec,
        desired: DesiredRuntimeState,
    ) -> Result<(), RuntimeError> {
        let names = Names::new(&spec.identity)?;
        if desired == DesiredRuntimeState::Stopped {
            self.home(&names).await?;
        }
        let Some(worker) = self.inspect_worker(spec, &names).await? else {
            return match desired {
                DesiredRuntimeState::Stopped => Ok(()),
                DesiredRuntimeState::Running => Err(RuntimeError::Missing { resource: "Worker" }),
            };
        };

        let target = match desired {
            DesiredRuntimeState::Running => ContainerState::Running,
            DesiredRuntimeState::Stopped => ContainerState::Stopped,
        };
        if worker.state == target {
            return Ok(());
        }
        match desired {
            DesiredRuntimeState::Running => {
                if self.home(&names).await?.is_none() {
                    return Err(RuntimeError::Missing { resource: "Home" });
                }
                self.client
                    .start_container(&worker.id, None)
                    .await
                    .map_err(engine_error("start Worker container"))?;
            }
            DesiredRuntimeState::Stopped => {
                self.client
                    .stop_container(
                        &worker.id,
                        Some(StopContainerOptions {
                            t: Some(10),
                            ..Default::default()
                        }),
                    )
                    .await
                    .map_err(engine_error("stop Worker container"))?;
            }
        }
        let observed = self.inspect_worker(spec, &names).await?;
        if observed.map(|worker| worker.state) != Some(target) {
            return Err(RuntimeError::UnexpectedState { resource: "Worker" });
        }
        Ok(())
    }

    async fn observe(&self, spec: &WorkerSpec) -> Result<RuntimeObservation, RuntimeError> {
        let names = Names::new(&spec.identity)?;
        let home_exists = self.home(&names).await?.is_some();
        let container = self
            .inspect_worker(spec, &names)
            .await?
            .map_or(ContainerState::Missing, |worker| worker.state);
        Ok(RuntimeObservation {
            container,
            home_exists,
        })
    }

    async fn remove_worker(&self, spec: &WorkerSpec) -> Result<(), RuntimeError> {
        let names = Names::new(&spec.identity)?;
        if let Some(worker) = self.inspect_worker(spec, &names).await? {
            if worker.state == ContainerState::Running {
                return Err(RuntimeError::Conflict {
                    resource: "running Worker",
                });
            }
            self.remove_container(&worker.id, None).await?;
        }
        Ok(())
    }

    async fn delete_home(&self, identity: &RuntimeIdentity) -> Result<(), RuntimeError> {
        let names = Names::new(identity)?;
        if self.container(&names.container).await?.is_some() {
            return Err(RuntimeError::Conflict {
                resource: "Worker container",
            });
        }
        if self.home(&names).await?.is_some() {
            self.remove_seeder(&names).await?;
            optional(
                self.client
                    .remove_volume(&names.home(), None::<RemoveVolumeOptions>)
                    .await,
                "remove Worker Home volume",
            )?;
            if self.home(&names).await?.is_some() {
                return Err(RuntimeError::UnexpectedState { resource: "Home" });
            }
        }
        Ok(())
    }

    async fn ensure_image(&self, image: &str) -> Result<(), RuntimeError> {
        if image.is_empty() {
            return Err(RuntimeError::InvalidSpec("missing Worker image"));
        }
        if optional(
            self.client.inspect_image(image).await,
            "inspect Worker image",
        )?
        .is_none()
        {
            self.client
                .create_image(
                    Some(CreateImageOptions {
                        from_image: Some(image.to_owned()),
                        ..Default::default()
                    }),
                    None,
                    None,
                )
                .try_for_each(|_| async { Ok(()) })
                .await
                .map_err(engine_error("pull Worker image"))?;
        }
        Ok(())
    }

    async fn home(&self, names: &Names) -> Result<Option<Volume>, RuntimeError> {
        let volume = optional(
            self.client.inspect_volume(&names.home()).await,
            "inspect Worker Home volume",
        )?;
        if let Some(volume) = &volume {
            names.verify_home(volume)?;
        }
        Ok(volume)
    }

    async fn ensure_network(&self) -> Result<String, RuntimeError> {
        let _guard = self.network_creation.lock().await;
        if let Some(network) = optional(
            self.client.inspect_network(WORKER_NETWORK, None).await,
            "inspect managed Worker network",
        )? {
            let label = network
                .labels
                .as_ref()
                .and_then(|labels| labels.get(RESOURCE_LABEL));
            let icc = network
                .options
                .as_ref()
                .and_then(|options| options.get(BRIDGE_ICC));
            if network.driver.as_deref() != Some("bridge")
                || label.map(String::as_str) != Some("network")
                || icc.map(String::as_str) != Some("false")
                || network.internal == Some(true)
            {
                return Err(RuntimeError::Conflict {
                    resource: "managed Worker network configuration",
                });
            }
            return network.id.ok_or(RuntimeError::UnexpectedState {
                resource: "network ID",
            });
        }
        let network = self
            .client
            .create_network(NetworkCreateRequest {
                name: WORKER_NETWORK.into(),
                driver: Some("bridge".into()),
                labels: Some(HashMap::from([(RESOURCE_LABEL.into(), "network".into())])),
                options: Some(HashMap::from([(BRIDGE_ICC.into(), "false".into())])),
                ..Default::default()
            })
            .await
            .map_err(engine_error("create managed Worker network"))?;
        Ok(network.id)
    }

    async fn container(
        &self,
        name: &str,
    ) -> Result<Option<ContainerInspectResponse>, RuntimeError> {
        optional(
            self.client.inspect_container(name, None).await,
            "inspect Worker container",
        )
    }

    async fn inspect_worker(
        &self,
        spec: &WorkerSpec,
        names: &Names,
    ) -> Result<Option<ObservedWorker>, RuntimeError> {
        let Some(container) = self.container(&names.container).await? else {
            return Ok(None);
        };
        let config = container.config.as_ref();
        let labels = config.and_then(|config| config.labels.as_ref());
        names.verify_labels(labels, "worker")?;
        if labels.and_then(|labels| labels.get(REVISION_LABEL)) != Some(&spec.revision.to_string())
            || config.and_then(|config| config.image.as_deref()) != Some(&spec.image)
        {
            return Err(RuntimeError::Conflict {
                resource: "Worker revision",
            });
        }
        names.verify_mount(&container)?;
        let id = container.id.ok_or(RuntimeError::UnexpectedState {
            resource: "Worker ID",
        })?;
        let state = match container.state.and_then(|state| state.status) {
            Some(ContainerStateStatusEnum::RUNNING | ContainerStateStatusEnum::RESTARTING) => {
                ContainerState::Running
            }
            Some(
                ContainerStateStatusEnum::CREATED
                | ContainerStateStatusEnum::EXITED
                | ContainerStateStatusEnum::DEAD,
            ) => ContainerState::Stopped,
            _ => {
                return Err(RuntimeError::UnexpectedState {
                    resource: "Worker state",
                })
            }
        };
        Ok(Some(ObservedWorker { id, state }))
    }

    async fn remove_container(
        &self,
        id: &str,
        options: Option<RemoveContainerOptions>,
    ) -> Result<(), RuntimeError> {
        optional(
            self.client.remove_container(id, options).await,
            "remove Worker container",
        )?;
        Ok(())
    }
}

impl ManagedWorkerRuntime for DockerEngine {
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

struct ObservedWorker {
    id: String,
    state: ContainerState,
}

struct Names {
    container: String,
    instance: String,
}

impl Names {
    fn new(identity: &RuntimeIdentity) -> Result<Self, RuntimeError> {
        if identity.short_id.len() != 8
            || !identity
                .short_id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        {
            return Err(RuntimeError::InvalidSpec(
                "Worker short ID must use eight lowercase letters or digits",
            ));
        }
        Ok(Self {
            container: format!("talos-worker-talo-{}", identity.short_id),
            instance: identity.instance_id.to_string(),
        })
    }

    fn home(&self) -> String {
        format!("{}-home", self.container)
    }

    fn seeder(&self) -> String {
        format!("{}-seed", self.container)
    }

    fn labels(&self, resource: &str) -> HashMap<String, String> {
        HashMap::from([
            (INSTANCE_LABEL.into(), self.instance.clone()),
            (RESOURCE_LABEL.into(), resource.into()),
        ])
    }

    fn verify_labels(
        &self,
        labels: Option<&HashMap<String, String>>,
        resource: &'static str,
    ) -> Result<(), RuntimeError> {
        if labels.is_some_and(|labels| {
            labels.get(INSTANCE_LABEL) == Some(&self.instance)
                && labels
                    .get(RESOURCE_LABEL)
                    .is_some_and(|value| value == resource)
        }) {
            return Ok(());
        }
        Err(RuntimeError::Conflict { resource })
    }

    fn verify_home(&self, volume: &Volume) -> Result<(), RuntimeError> {
        self.verify_labels(Some(&volume.labels), "home")?;
        if volume.name != self.home() || volume.driver != "local" || !volume.options.is_empty() {
            return Err(RuntimeError::Conflict {
                resource: "Home configuration",
            });
        }
        Ok(())
    }

    fn home_mount(&self) -> Mount {
        Mount {
            source: Some(self.home()),
            target: Some(HOME_PATH.into()),
            typ: Some(MountType::VOLUME),
            ..Default::default()
        }
    }

    fn verify_mount(&self, container: &ContainerInspectResponse) -> Result<(), RuntimeError> {
        if let Some([mount]) = container.mounts.as_deref() {
            if mount.typ.as_deref() == Some("volume")
                && mount.name.as_deref() == Some(&self.home())
                && mount.destination.as_deref() == Some(HOME_PATH)
                && mount.rw == Some(true)
            {
                return Ok(());
            }
        }
        Err(RuntimeError::Conflict {
            resource: "Home mount",
        })
    }
}

#[cfg(unix)]
fn connect_socket(path: &Path) -> Result<Docker, RuntimeError> {
    use std::os::unix::fs::FileTypeExt;

    let socket = path
        .to_str()
        .filter(|_| path.is_absolute())
        .ok_or(RuntimeError::InvalidSpec(
            "Docker socket must be an absolute path",
        ))?;
    let metadata = path.metadata().map_err(|source| RuntimeError::Docker {
        operation: "inspect Unix socket",
        source: Box::new(source),
    })?;
    if !metadata.file_type().is_socket() {
        return Err(RuntimeError::InvalidSpec(
            "Docker endpoint must be a Unix socket",
        ));
    }
    Docker::connect_with_unix(socket, 120, bollard::API_DEFAULT_VERSION)
        .map_err(engine_error("connect to Unix socket"))
}

#[cfg(not(unix))]
fn connect_socket(_path: &Path) -> Result<Docker, RuntimeError> {
    Err(RuntimeError::InvalidSpec(
        "DockerEngine requires a Unix socket",
    ))
}

fn optional<T>(
    result: Result<T, Error>,
    operation: &'static str,
) -> Result<Option<T>, RuntimeError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(Error::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(None),
        Err(source) => Err(engine_error(operation)(source)),
    }
}

fn engine_error(operation: &'static str) -> impl FnOnce(Error) -> RuntimeError {
    move |source| RuntimeError::Docker {
        operation,
        source: Box::new(source),
    }
}
