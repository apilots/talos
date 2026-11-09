use std::collections::BTreeMap;

use k8s_openapi::{
    api::{
        apps::v1::{StatefulSet, StatefulSetSpec, StatefulSetUpdateStrategy},
        core::v1::{
            Capabilities, Container, EmptyDirVolumeSource, EnvVar, PersistentVolumeClaim,
            PersistentVolumeClaimSpec, PersistentVolumeClaimVolumeSource, PodSecurityContext,
            PodSpec, PodTemplateSpec, ResourceRequirements, SeccompProfile, SecurityContext,
            Volume, VolumeMount,
        },
    },
    apimachinery::pkg::{
        api::resource::Quantity,
        apis::meta::v1::{LabelSelector, ObjectMeta},
    },
};

use super::{has_revision, Kubernetes, Names};
use crate::{RuntimeError, WorkerSpec, HOME_PATH, REVISION_LABEL};

const HOME_VOLUME: &str = "home";
const SHM_VOLUME: &str = "shm";
const WORKER_ID: i64 = 10001;

impl Kubernetes {
    pub(super) fn home_claim(&self, name: &str, storage: &str) -> PersistentVolumeClaim {
        PersistentVolumeClaim {
            metadata: ObjectMeta {
                name: Some(name.into()),
                ..Default::default()
            },
            spec: Some(PersistentVolumeClaimSpec {
                storage_class_name: Some(self.options.storage_class.clone()),
                access_modes: Some(vec!["ReadWriteOnce".into()]),
                volume_mode: Some("Filesystem".into()),
                resources: Some(ResourceRequirements {
                    requests: Some(BTreeMap::from([(
                        "storage".into(),
                        Quantity(storage.into()),
                    )])),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    pub(super) fn verify_home(
        &self,
        names: &Names,
        home: &PersistentVolumeClaim,
    ) -> Result<(), RuntimeError> {
        names.verify_labels(&home.metadata, "home")?;
        if !home.spec.as_ref().is_some_and(|spec| {
            spec.storage_class_name.as_deref() == Some(&self.options.storage_class)
                && spec
                    .access_modes
                    .as_deref()
                    .is_some_and(|modes| modes == ["ReadWriteOnce"])
                && spec.volume_mode.as_deref() == Some("Filesystem")
        }) || home
            .metadata
            .owner_references
            .as_ref()
            .is_some_and(|owners| !owners.is_empty())
        {
            return Err(RuntimeError::Conflict {
                resource: "Home PVC configuration",
            });
        }
        Ok(())
    }

    pub(super) fn worker_workload(&self, spec: &WorkerSpec, names: &Names) -> StatefulSet {
        let selector_labels = names.labels("worker");
        let mut revision_labels = selector_labels.clone();
        revision_labels.insert(REVISION_LABEL.into(), spec.revision.to_string());

        let mut pod = pod_spec(
            Container {
                name: "worker".into(),
                image: Some(spec.image.clone()),
                image_pull_policy: Some("IfNotPresent".into()),
                env: Some(
                    ["HOME", "TALOS_HOME"]
                        .map(|name| EnvVar {
                            name: name.into(),
                            value: Some(HOME_PATH.into()),
                            ..Default::default()
                        })
                        .into(),
                ),
                resources: Some(worker_resources(spec)),
                volume_mounts: Some(vec![
                    home_mount(),
                    VolumeMount {
                        name: SHM_VOLUME.into(),
                        mount_path: "/dev/shm".into(),
                        ..Default::default()
                    },
                ]),
                security_context: Some(container_security()),
                ..Default::default()
            },
            vec![
                home_volume(names),
                Volume {
                    name: SHM_VOLUME.into(),
                    empty_dir: Some(EmptyDirVolumeSource {
                        medium: Some("Memory".into()),
                        size_limit: Some(Quantity(decimal_quantity(spec.shm_bytes))),
                    }),
                    ..Default::default()
                },
            ],
            SeccompProfile {
                type_: "Localhost".into(),
                localhost_profile: Some(self.options.seccomp_profile.clone()),
            },
        );
        pod.termination_grace_period_seconds = Some(10);

        StatefulSet {
            metadata: ObjectMeta {
                name: Some(names.worker.clone()),
                labels: Some(revision_labels.clone()),
                ..Default::default()
            },
            spec: Some(StatefulSetSpec {
                replicas: Some(0),
                // Worker Pods only initiate outbound connections and do not use a Service.
                service_name: names.worker.clone(),
                selector: LabelSelector {
                    match_labels: Some(selector_labels),
                    ..Default::default()
                },
                template: PodTemplateSpec {
                    metadata: Some(ObjectMeta {
                        labels: Some(revision_labels),
                        ..Default::default()
                    }),
                    spec: Some(pod),
                },
                update_strategy: Some(StatefulSetUpdateStrategy {
                    type_: Some("OnDelete".into()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    pub(super) fn verify_worker(
        &self,
        spec: &WorkerSpec,
        names: &Names,
        worker: &StatefulSet,
    ) -> Result<(), RuntimeError> {
        names.verify_labels(&worker.metadata, "worker")?;
        if !has_revision(&worker.metadata, spec.revision) {
            return Err(RuntimeError::Conflict {
                resource: "Worker revision",
            });
        }
        if !worker.spec.as_ref().is_some_and(|workload| {
            matches!(workload.replicas, Some(0 | 1))
                && workload
                    .ordinals
                    .as_ref()
                    .is_none_or(|ordinals| ordinals.start.unwrap_or_default() == 0)
                && workload
                    .volume_claim_templates
                    .as_ref()
                    .is_none_or(Vec::is_empty)
                && workload
                    .selector
                    .match_expressions
                    .as_ref()
                    .is_none_or(Vec::is_empty)
                && workload.selector.match_labels.as_ref() == Some(&names.labels("worker"))
                && workload
                    .template
                    .metadata
                    .as_ref()
                    .is_some_and(|meta| has_revision(meta, spec.revision))
                && workload.template.spec.as_ref().is_some_and(|pod| {
                    mounts_home(pod, names)
                        && pod.containers.iter().any(|container| {
                            container.name == "worker"
                                && container.volume_mounts.as_ref().is_some_and(|mounts| {
                                    mounts.iter().any(|mount| {
                                        mount.name == HOME_VOLUME
                                            && mount.mount_path == HOME_PATH
                                            && mount.read_only != Some(true)
                                    })
                                })
                        })
                })
        }) {
            return Err(RuntimeError::Conflict {
                resource: "Worker workload configuration",
            });
        }
        Ok(())
    }
}

pub(super) fn mounts_home(pod: &PodSpec, names: &Names) -> bool {
    pod.volumes.as_ref().is_some_and(|volumes| {
        volumes.iter().any(|volume| {
            volume.name == HOME_VOLUME
                && volume
                    .persistent_volume_claim
                    .as_ref()
                    .is_some_and(|claim| {
                        claim.claim_name == names.home && claim.read_only != Some(true)
                    })
        })
    })
}

pub(super) fn home_volume(names: &Names) -> Volume {
    Volume {
        name: HOME_VOLUME.into(),
        persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
            claim_name: names.home.clone(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub(super) fn home_mount() -> VolumeMount {
    VolumeMount {
        name: HOME_VOLUME.into(),
        mount_path: HOME_PATH.into(),
        ..Default::default()
    }
}

pub(super) fn pod_spec(
    container: Container,
    volumes: Vec<Volume>,
    seccomp: SeccompProfile,
) -> PodSpec {
    PodSpec {
        containers: vec![container],
        volumes: Some(volumes),
        automount_service_account_token: Some(false),
        enable_service_links: Some(false),
        security_context: Some(PodSecurityContext {
            run_as_user: Some(WORKER_ID),
            run_as_group: Some(WORKER_ID),
            run_as_non_root: Some(true),
            fs_group: Some(WORKER_ID),
            fs_group_change_policy: Some("OnRootMismatch".into()),
            seccomp_profile: Some(seccomp),
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub(super) fn container_security() -> SecurityContext {
    SecurityContext {
        allow_privilege_escalation: Some(false),
        capabilities: Some(Capabilities {
            drop: Some(vec!["ALL".into()]),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn worker_resources(spec: &WorkerSpec) -> ResourceRequirements {
    let values = BTreeMap::from([
        (
            "cpu".into(),
            Quantity(decimal_quantity(u64::from(spec.cpu_cores))),
        ),
        (
            "memory".into(),
            Quantity(decimal_quantity(spec.memory_bytes)),
        ),
    ]);
    ResourceRequirements {
        requests: Some(values.clone()),
        limits: Some(values),
        ..Default::default()
    }
}

pub(super) fn byte_quantity(bytes: u64) -> Result<String, RuntimeError> {
    if bytes == 0 || i64::try_from(bytes).is_err() {
        return Err(RuntimeError::InvalidSpec(
            "invalid Kubernetes Home capacity",
        ));
    }
    Ok(decimal_quantity(bytes))
}

// Match the API server's canonical DecimalSI quantities when validating existing resources.
fn decimal_quantity(mut amount: u64) -> String {
    let mut suffix = "";
    for unit in ["k", "M", "G", "T", "P", "E"] {
        if amount == 0 || !amount.is_multiple_of(1_000) {
            break;
        }
        amount /= 1_000;
        suffix = unit;
    }
    format!("{amount}{suffix}")
}
