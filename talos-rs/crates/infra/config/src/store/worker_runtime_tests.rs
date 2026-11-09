use super::*;
use crate::domains::managed_worker::{KubeconfigSource, WorkerConnectionConfig, WorkerRuntimeKind};

#[test]
fn runtime_selection_preserves_operator_fields_and_other_settings() {
    let directory = tempfile::tempdir().unwrap();
    let config_path = directory.path().join("talos.yaml");
    let original: Value = serde_yaml::from_str(
        r#"
gateway:
  port: 8081
talos:
  worker:
    runtime:
      kind: Kubernetes
      image: ${WORKER_IMAGE}
      control_plane_endpoint: ${WORKER_ENDPOINT}
      kubernetes:
        storage_class: old
        kubeconfig_path: ${MOUNTED_KUBECONFIG}
"#,
    )
    .unwrap();
    std::fs::write(&config_path, serde_yaml::to_string(&original).unwrap()).unwrap();
    let mut previous: WorkerRuntimeConfig =
        serde_yaml::from_value(original["talos"]["worker"]["runtime"].clone()).unwrap();
    previous.image = "registry.example/worker@sha256:abc".into();
    previous.control_plane_endpoint = "https://agent.example.test".into();
    previous.kubernetes.kubeconfig_path = Some("/mnt/kubeconfig".into());
    let mut next = previous.clone();
    next.kubernetes.storage_class = "new".into();
    next.kubernetes.kubeconfig_source = Some(KubeconfigSource::Pasted);
    next.kubernetes.pasted_kubeconfig = Some(uuid::Uuid::new_v4());

    next.kubernetes.worker = Some(WorkerConnectionConfig {
        control_plane_endpoint: "https://worker.talos.svc".into(),
        ..previous.connection(WorkerRuntimeKind::Kubernetes)
    });
    let previous_policy = ServerRuntimePolicy::default();
    let next_policy = ServerRuntimePolicy {
        idle_timeout_seconds: 600,
        ..previous_policy.clone()
    };

    YamlConfigStore::new(config_path.clone())
        .save_worker_runtime_selection(&previous, &next, &previous_policy, &next_policy)
        .unwrap();

    let saved: Value =
        serde_yaml::from_str(&std::fs::read_to_string(config_path).unwrap()).unwrap();
    let mut expected = original;
    expected["talos"]["worker"]["runtime"] = serde_yaml::from_str(&format!(
        r#"
kind: Kubernetes
image: ${{WORKER_IMAGE}}
control_plane_endpoint: ${{WORKER_ENDPOINT}}
kubernetes:
  storage_class: new
  worker:
    image: ${{WORKER_IMAGE}}
    control_plane_endpoint: https://worker.talos.svc
  kubeconfig_path: ${{MOUNTED_KUBECONFIG}}
  kubeconfig_source: Pasted
  pasted_kubeconfig: {}
"#,
        next.kubernetes.pasted_kubeconfig.unwrap()
    ))
    .unwrap();
    expected["talos"]["server"]["runtime"]["idle_timeout_seconds"] = 600.into();
    assert_eq!(saved, expected);
}
