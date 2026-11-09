use super::*;

fn options() -> KubernetesOptions {
    KubernetesOptions {
        namespace: "talos".into(),
        storage_class: "local-path".into(),
        seccomp_profile: "talos/chromium-seccomp.json".into(),
    }
}

fn kubeconfig(endpoint: &str, cluster: &str, auth: &str) -> String {
    format!(
        "apiVersion: v1\nkind: Config\ncurrent-context: fixture\n\
         contexts:\n- name: fixture\n  context: {{ cluster: target, user: worker }}\n\
         clusters:\n- name: target\n  cluster:\n    server: {endpoint}\n{cluster}\n\
         users:\n- name: worker\n  user:\n    token: fixture-token\n{auth}\n"
    )
}

#[tokio::test]
async fn inline_kubeconfig_rejects_untrusted_local_and_insecure_inputs() {
    let https = "https://kubernetes.invalid";
    let cases = [
        ("http://kubernetes.invalid", "", ""),
        (https, "    insecure-skip-tls-verify: true", ""),
        (https, "    certificate-authority: /private/ca.pem", ""),
        (https, "    proxy-url: https://proxy.invalid", ""),
        (https, "", "    tokenFile: /private/token"),
        (https, "", "    client-certificate: /private/cert.pem"),
        (https, "", "    client-key: /private/key.pem"),
        (https, "", "    exec: { command: /private/helper }"),
        (https, "", "    auth-provider: { name: fixture }"),
        (https, "", "    as: system:admin"),
    ];

    for (endpoint, cluster, auth) in cases {
        let result = Kubernetes::from_inline_kubeconfig(
            options(),
            &kubeconfig(endpoint, cluster, auth),
            None,
        )
        .await;
        let Err(error @ RuntimeError::InvalidSpec(_)) = result else {
            panic!("unsafe inline kubeconfig was accepted");
        };
        let diagnostic = format!("{error:?}");
        assert!(!diagnostic.contains("fixture-token"));
        assert!(!diagnostic.contains("/private/"));
    }
}

#[tokio::test]
async fn inline_kubeconfig_accepts_a_self_contained_bearer_token() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let config = kubeconfig("https://kubernetes.invalid", "", "");
    let runtime = Kubernetes::from_inline_kubeconfig(options(), &config, Some("fixture")).await;
    assert!(runtime.is_ok(), "self-contained kubeconfig was rejected");
}

#[tokio::test]
async fn accepts_admission_changes_to_owned_worker_template() {
    use k8s_openapi::api::core::v1::Container;

    use super::super::Names;
    use crate::{RuntimeIdentity, WorkerSpec};

    let _ = rustls::crypto::ring::default_provider().install_default();
    let config = kubeconfig("https://kubernetes.invalid", "", "");
    let runtime = Kubernetes::from_inline_kubeconfig(options(), &config, None)
        .await
        .unwrap();
    let spec = WorkerSpec {
        identity: RuntimeIdentity {
            instance_id: uuid::Uuid::nil(),
            short_id: "fixture1".into(),
        },
        revision: 1,
        image: "registry.example/talos-worker:fixture".into(),
        cpu_cores: 2,
        memory_bytes: 2 << 30,
        shm_bytes: 1 << 30,
    };
    let names = Names::new(&spec.identity).unwrap();
    let mut worker = runtime.worker_workload(&spec, &names);
    let pod = worker
        .spec
        .as_mut()
        .unwrap()
        .template
        .spec
        .as_mut()
        .unwrap();
    pod.containers[0].image = Some("mirror.example/talos-worker:fixture".into());
    pod.containers[0].resources.as_mut().unwrap().requests = None;
    pod.containers.push(Container {
        name: "sidecar".into(),
        image: Some("registry.example/sidecar:fixture".into()),
        ..Default::default()
    });

    runtime.verify_worker(&spec, &names, &worker).unwrap();
}
