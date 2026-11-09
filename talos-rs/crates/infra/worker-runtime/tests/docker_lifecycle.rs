//! Run with a disposable Linux Engine and a Worker image running a long-lived child under init.

#![cfg(unix)]

use std::{env, path::PathBuf, time::Duration};

use bollard::{exec::StartExecResults, models::ExecConfig, Docker, API_DEFAULT_VERSION};
use futures_util::TryStreamExt;
use talos_worker_runtime::{
    ContainerState, DesiredRuntimeState, DockerEngine, HomeCapacity, ManagedWorkerRuntime,
    RuntimeError, RuntimeIdentity, RuntimeObservation, SeedFile, SeedMode, WorkerSpec,
    DEFAULT_SECCOMP_PROFILE,
};
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires a disposable Linux Docker Engine and an installed Worker fixture image"]
async fn managed_lifecycle_preserves_home_and_rejects_conflicts() {
    let socket = PathBuf::from(env::var_os("TALO_TEST_DOCKER_SOCKET").expect("Docker test socket"));
    let image = env::var("TALO_TEST_WORKER_IMAGE").expect("locally installed Worker fixture image");
    let runtime = DockerEngine::connect(&socket, DEFAULT_SECCOMP_PROFILE).unwrap();
    runtime.check().await.unwrap();
    let docker = Docker::connect_with_unix(socket.to_str().unwrap(), 20, API_DEFAULT_VERSION)
        .unwrap()
        .negotiate_version()
        .await
        .unwrap();
    let instance_id = Uuid::new_v4();
    let identity = RuntimeIdentity {
        instance_id,
        short_id: instance_id.simple().to_string()[..8].to_owned(),
    };
    let spec = WorkerSpec {
        identity,
        revision: 1,
        image,
        cpu_cores: 4,
        memory_bytes: 8 << 30,
        shm_bytes: 1 << 30,
    };
    let worker_name = format!("talos-worker-talo-{}", spec.identity.short_id);
    let test_result = tokio::spawn({
        let runtime = DockerEngine::connect(&socket, DEFAULT_SECCOMP_PROFILE).unwrap();
        let spec = spec.clone();
        let worker_name = worker_name.clone();
        async move {
            runtime.check().await.unwrap();
            exercise_lifecycle(&runtime, &docker, &spec, &worker_name).await;
        }
    })
    .await;
    // Cleanup by verified identity even after a regression assertion fails.
    for revision in [spec.revision, spec.revision + 1] {
        let spec = WorkerSpec {
            revision,
            ..spec.clone()
        };
        let _ = runtime
            .set_running(&spec, DesiredRuntimeState::Stopped)
            .await;
        let _ = runtime.remove_runtime(&spec).await;
    }
    runtime.remove_home(&spec.identity).await.unwrap();
    test_result.unwrap();
}

async fn exercise_lifecycle(
    runtime: &DockerEngine,
    docker: &Docker,
    spec: &WorkerSpec,
    worker_name: &str,
) {
    assert_eq!(
        runtime.inspect(spec).await.unwrap(),
        RuntimeObservation {
            container: ContainerState::Missing,
            home_exists: false
        }
    );
    assert!(matches!(
        runtime
            .ensure_home(
                spec,
                HomeCapacity::LimitedBytes(1024),
                &[],
                SeedMode::CreateMissing
            )
            .await,
        Err(RuntimeError::UnsupportedCapacity)
    ));
    let original = b"fixture credential".to_vec();
    let original_config = b"configured by the Worker".to_vec();
    let new_credential = b"replacement credential".to_vec();
    runtime
        .ensure_home(
            spec,
            HomeCapacity::Unrestricted,
            &[
                SeedFile::new("conf/node.credential", original.clone()),
                SeedFile::new("conf/talos.yaml", original_config.clone()),
            ],
            SeedMode::CreateMissing,
        )
        .await
        .unwrap();
    runtime
        .ensure_home(
            spec,
            HomeCapacity::Unrestricted,
            &[SeedFile::new(
                "conf/talos.yaml",
                b"bootstrap retry".to_vec(),
            )],
            SeedMode::CreateMissing,
        )
        .await
        .unwrap();

    let mut conflicting = spec.clone();
    conflicting.identity.instance_id = Uuid::new_v4();
    assert!(matches!(
        runtime.remove_home(&conflicting.identity).await,
        Err(RuntimeError::Conflict { .. })
    ));

    runtime.ensure_runtime(spec).await.unwrap();
    let first = docker.inspect_container(worker_name, None).await.unwrap();
    let first_id = first.id.clone().unwrap();
    runtime.ensure_runtime(spec).await.unwrap();
    let existing = docker.inspect_container(worker_name, None).await.unwrap();
    assert_eq!(existing.id.as_deref(), Some(first_id.as_str()));
    let config = existing.host_config.unwrap();
    assert_eq!(config.nano_cpus, Some(4_000_000_000));
    assert_eq!(config.memory, Some(8 << 30));
    assert_eq!(
        config.restart_policy.unwrap().name,
        Some(bollard::models::RestartPolicyNameEnum::UNLESS_STOPPED)
    );
    assert!(config
        .port_bindings
        .is_none_or(|bindings| bindings.is_empty()));
    let network = config.network_mode.unwrap();
    let network_config = docker.inspect_network(&network, None).await.unwrap();
    assert_eq!(
        network_config.name.as_deref(),
        Some("talos-managed-workers")
    );
    assert_eq!(
        network_config.options.unwrap()["com.docker.network.bridge.enable_icc"],
        "false"
    );
    assert_eq!(existing.mounts.unwrap().len(), 1);
    assert_eq!(
        existing.config.unwrap().user.as_deref(),
        Some("10001:10001")
    );
    assert!(matches!(
        runtime
            .ensure_runtime(&WorkerSpec {
                revision: spec.revision + 1,
                ..spec.clone()
            })
            .await,
        Err(RuntimeError::Conflict { .. })
    ));

    runtime
        .set_running(spec, DesiredRuntimeState::Running)
        .await
        .unwrap();
    assert_eq!(
        runtime.inspect(spec).await.unwrap().container,
        ContainerState::Running
    );
    let credential = read_file(docker, &first_id, "conf/node.credential").await;
    assert_eq!(credential, original);
    assert_eq!(
        read_file(docker, &first_id, "conf/talos.yaml").await,
        original_config
    );
    assert!(matches!(
        runtime.remove_runtime(spec).await,
        Err(RuntimeError::Conflict { .. })
    ));
    assert!(matches!(
        runtime.remove_home(&spec.identity).await,
        Err(RuntimeError::Conflict { .. })
    ));

    // Let Docker monitor a successfully started container before killing its child.
    tokio::time::sleep(Duration::from_secs(11)).await;
    let crash = docker
        .create_exec(
            &first_id,
            ExecConfig {
                cmd: Some(vec![
                    "/bin/sh".into(),
                    "-c".into(),
                    "kill -KILL $(cat /proc/1/task/1/children)".into(),
                ]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    docker
        .start_exec(
            &crash.id,
            Some(bollard::exec::StartExecOptions {
                detach: true,
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let observed = docker.inspect_container(&first_id, None).await.unwrap();
            if observed.restart_count.unwrap_or_default() > 0
                && observed.state.unwrap().status
                    == Some(bollard::models::ContainerStateStatusEnum::RUNNING)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .expect("Docker did not restart the Worker after its child crashed");

    runtime
        .set_running(spec, DesiredRuntimeState::Stopped)
        .await
        .unwrap();
    assert_eq!(
        runtime.inspect(spec).await.unwrap().container,
        ContainerState::Stopped
    );
    runtime.remove_runtime(spec).await.unwrap();
    runtime.remove_runtime(spec).await.unwrap();
    let reset = WorkerSpec {
        revision: spec.revision + 1,
        ..spec.clone()
    };
    runtime
        .ensure_home(
            &reset,
            HomeCapacity::Unrestricted,
            &[SeedFile::new(
                "conf/node.credential",
                new_credential.clone(),
            )],
            SeedMode::Replace,
        )
        .await
        .unwrap();
    runtime.ensure_runtime(&reset).await.unwrap();
    runtime
        .set_running(&reset, DesiredRuntimeState::Running)
        .await
        .unwrap();
    let replacement = docker.inspect_container(worker_name, None).await.unwrap();
    assert_eq!(
        replacement.host_config.unwrap().network_mode.as_deref(),
        Some(network.as_str())
    );
    let reset_id = replacement.id.unwrap();
    assert_ne!(reset_id, first_id);
    assert_eq!(
        read_file(docker, &reset_id, "conf/node.credential").await,
        new_credential
    );
    assert_eq!(
        read_file(docker, &reset_id, "conf/talos.yaml").await,
        original_config
    );

    runtime
        .set_running(&reset, DesiredRuntimeState::Stopped)
        .await
        .unwrap();
    runtime.remove_runtime(&reset).await.unwrap();
    runtime.remove_home(&reset.identity).await.unwrap();
    runtime.remove_home(&reset.identity).await.unwrap();
    assert_eq!(
        runtime.inspect(&reset).await.unwrap(),
        RuntimeObservation {
            container: ContainerState::Missing,
            home_exists: false
        }
    );
}

async fn read_file(docker: &Docker, container: &str, path: &str) -> Vec<u8> {
    let exec = docker
        .create_exec(
            container,
            ExecConfig {
                attach_stdout: Some(true),
                cmd: Some(vec![
                    "/bin/bash".into(),
                    "-ec".into(),
                    "test \"$(stat -c %a -- \"$1\")\" = 600 && cat -- \"$1\"".into(),
                    "test".into(),
                    format!("/home/talos/.talos/{path}"),
                ]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let StartExecResults::Attached { output, .. } =
        docker.start_exec(&exec.id, None).await.unwrap()
    else {
        panic!("expected attached exec output");
    };
    let bytes = output
        .try_fold(Vec::new(), |mut bytes, chunk| async move {
            bytes.extend_from_slice(&chunk.into_bytes());
            Ok(bytes)
        })
        .await
        .unwrap();
    assert_eq!(
        docker.inspect_exec(&exec.id).await.unwrap().exit_code,
        Some(0)
    );
    bytes
}
