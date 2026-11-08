use std::time::Duration;

use k8s_openapi::{
    api::{
        batch::v1::{Job, JobSpec},
        core::v1::{
            Container, PodTemplateSpec, SeccompProfile, Secret, SecretVolumeSource, Volume,
            VolumeMount,
        },
    },
    apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference},
    ByteString,
};
use kube::api::PostParams;

use super::{
    api_error, delete_owned, kube_request, objects, resource_uid, wait_for, Kubernetes, Names,
};
use crate::{seed::WRITE_FILE, RuntimeError, SeedFile, SeedMode, WorkerSpec, HOME_PATH};

const SEED_DEADLINE_SECONDS: i64 = 90;
const SEED_TIMEOUT: Duration = Duration::from_secs(105);
const SEED_VOLUME: &str = "seed-files";
const SEED_PATH: &str = "/run/talos-seed";

// File content reaches the shared writer over stdin. Job arguments contain paths and sizes.
const WRITE_FILES: &str = r#"
set -eu
writer=$1
home=$2
mode=$3
source=$4
shift 4
index=0
while [ "$#" -ne 0 ]; do
    /bin/bash -c "$writer" talos-seed "$home" "$1" "$mode" "$2" < "$source/$index"
    shift 2
    index=$((index + 1))
done
"#;

impl Kubernetes {
    pub(super) async fn seed_home(
        &self,
        spec: &WorkerSpec,
        names: &Names,
        files: &[SeedFile],
        paths: &[&str],
        mode: SeedMode,
    ) -> Result<(), RuntimeError> {
        self.remove_seeder(names).await?;

        // The Job owns the Secret so TTL cleanup also removes credential bytes after a
        // Server interruption. Its Pod cannot start before the required Secret exists.
        let job = kube_request(self.jobs.create(
            &PostParams::default(),
            &seed_job(spec, names, files, paths, mode),
        ))
        .await
        .map_err(api_error("create Worker Home seeder"))?;
        let seeded = async {
            let uid = resource_uid(&job.metadata, "Worker Home seeder")?;
            let secret = Secret {
                metadata: ObjectMeta {
                    name: Some(names.secret.clone()),
                    labels: Some(names.labels("seed-files")),
                    owner_references: Some(vec![OwnerReference {
                        api_version: "batch/v1".into(),
                        kind: "Job".into(),
                        name: names.job.clone(),
                        uid: uid.into(),
                        ..Default::default()
                    }]),
                    ..Default::default()
                },
                data: Some(
                    files
                        .iter()
                        .enumerate()
                        .map(|(index, file)| (index.to_string(), ByteString(file.contents.clone())))
                        .collect(),
                ),
                immutable: Some(true),
                ..Default::default()
            };
            kube_request(self.secrets.create(&PostParams::default(), &secret))
                .await
                .map_err(api_error("supply Worker Home seed files"))?;
            wait_for("Worker Home seeder", SEED_TIMEOUT, || async {
                let job = kube_request(self.jobs.get(&names.job))
                    .await
                    .map_err(api_error("read Worker Home seeder result"))?;
                if job.metadata.uid.as_deref() != Some(uid) {
                    return Err(RuntimeError::Conflict {
                        resource: "Worker Home seeder",
                    });
                }
                let conditions = job
                    .status
                    .as_ref()
                    .and_then(|status| status.conditions.as_deref())
                    .unwrap_or_default();
                if conditions
                    .iter()
                    .any(|condition| condition.type_ == "Failed" && condition.status == "True")
                {
                    return Err(RuntimeError::UnexpectedState {
                        resource: "Worker Home file write",
                    });
                }
                Ok(conditions
                    .iter()
                    .any(|condition| condition.type_ == "Complete" && condition.status == "True"))
            })
            .await
        }
        .await;
        let cleanup = self.remove_seeder(names).await;
        seeded.and(cleanup)
    }

    pub(super) async fn remove_seeder(&self, names: &Names) -> Result<(), RuntimeError> {
        let job = kube_request(self.jobs.get_opt(&names.job))
            .await
            .map_err(api_error("inspect Worker Home seeder"))?;
        let secret = kube_request(self.secrets.get_metadata_opt(&names.secret))
            .await
            .map_err(api_error("inspect Worker Home seed files"))?;
        if let Some(secret) = &secret {
            names.verify_labels(&secret.metadata, "seed-files")?;
        }
        if let Some(job) = &job {
            names.verify_labels(&job.metadata, "seeder")?;
            if !job
                .spec
                .as_ref()
                .and_then(|spec| spec.template.spec.as_ref())
                .is_some_and(|pod| objects::mounts_home(pod, names))
            {
                return Err(RuntimeError::Conflict {
                    resource: "Worker Home seeder mount",
                });
            }
            delete_owned(&self.jobs, &names.job, &job.metadata, "Worker Home seeder").await?;
        }
        if let Some(secret) = secret {
            delete_owned(
                &self.secrets,
                &names.secret,
                &secret.metadata,
                "Worker Home seed files",
            )
            .await?;
        }
        Ok(())
    }
}

fn seed_job(
    spec: &WorkerSpec,
    names: &Names,
    files: &[SeedFile],
    paths: &[&str],
    mode: SeedMode,
) -> Job {
    let mode = match mode {
        SeedMode::CreateMissing => "create",
        SeedMode::Replace => "replace",
    };
    let mut arguments = [
        WRITE_FILES,
        "talos-seed",
        WRITE_FILE,
        HOME_PATH,
        mode,
        SEED_PATH,
    ]
    .map(str::to_owned)
    .to_vec();
    for (file, path) in files.iter().zip(paths) {
        arguments.extend([(*path).into(), file.contents.len().to_string()]);
    }
    let mut security = objects::container_security();
    security.read_only_root_filesystem = Some(true);
    let mut pod = objects::pod_spec(
        Container {
            name: "seeder".into(),
            image: Some(spec.image.clone()),
            image_pull_policy: Some("IfNotPresent".into()),
            command: Some(vec!["/bin/bash".into(), "-c".into()]),
            args: Some(arguments),
            volume_mounts: Some(vec![
                objects::home_mount(),
                VolumeMount {
                    name: SEED_VOLUME.into(),
                    mount_path: SEED_PATH.into(),
                    read_only: Some(true),
                    ..Default::default()
                },
            ]),
            security_context: Some(security),
            ..Default::default()
        },
        vec![
            objects::home_volume(names),
            Volume {
                name: SEED_VOLUME.into(),
                secret: Some(SecretVolumeSource {
                    secret_name: Some(names.secret.clone()),
                    default_mode: Some(0o440),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ],
        SeccompProfile {
            type_: "RuntimeDefault".into(),
            localhost_profile: None,
        },
    );
    pod.restart_policy = Some("Never".into());
    pod.termination_grace_period_seconds = Some(5);

    Job {
        metadata: ObjectMeta {
            name: Some(names.job.clone()),
            labels: Some(names.labels("seeder")),
            ..Default::default()
        },
        spec: Some(JobSpec {
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(names.labels("seeder")),
                    ..Default::default()
                }),
                spec: Some(pod),
            },
            backoff_limit: Some(0),
            active_deadline_seconds: Some(SEED_DEADLINE_SECONDS),
            ttl_seconds_after_finished: Some(120),
            ..Default::default()
        }),
        ..Default::default()
    }
}
