use std::{path::Path, time::Duration};

use futures_util::future::try_join_all;
use k8s_openapi::api::authorization::v1::{
    ResourceAttributes, SelfSubjectAccessReview, SelfSubjectAccessReviewSpec,
};
use kube::{
    api::PostParams,
    config::{KubeConfigOptions, Kubeconfig},
    Api, Client, Config,
};

use super::{api_error, kube_request, Kubernetes, KubernetesOptions};
use crate::{seed::valid_relative_path, RuntimeError};

const INLINE_KUBECONFIG_ERROR: RuntimeError =
    RuntimeError::InvalidSpec("invalid self-contained Kubernetes kubeconfig");

impl Kubernetes {
    /// Use the Server Pod's projected ServiceAccount credentials.
    pub fn in_cluster(options: KubernetesOptions) -> Result<Self, RuntimeError> {
        let config = Config::incluster().map_err(|source| RuntimeError::Kubernetes {
            operation: "load in-cluster configuration",
            source: Box::new(source),
        })?;
        Self::connect(options, config)
    }

    /// Load an explicit operator-managed kubeconfig. The caller trusts the mounted file.
    pub async fn from_kubeconfig(
        options: KubernetesOptions,
        path: &Path,
        context: Option<&str>,
    ) -> Result<Self, RuntimeError> {
        if !path.is_absolute() {
            return Err(RuntimeError::InvalidSpec(
                "Kubernetes kubeconfig path must be absolute",
            ));
        }
        let kubeconfig = Kubeconfig::read_from(path)
            .map_err(|_| RuntimeError::InvalidSpec("cannot read the mounted kubeconfig"))?;
        let config = Config::from_custom_kubeconfig(kubeconfig, &config_options(context))
            .await
            .map_err(|_| RuntimeError::InvalidSpec("invalid mounted kubeconfig"))?;
        Self::connect(options, config)
    }

    /// Load a self-contained kubeconfig supplied by an administrator. Local file references,
    /// executable authentication, impersonation and insecure transport are rejected.
    pub async fn from_inline_kubeconfig(
        options: KubernetesOptions,
        contents: &str,
        context: Option<&str>,
    ) -> Result<Self, RuntimeError> {
        let kubeconfig = Kubeconfig::from_yaml(contents).map_err(|_| INLINE_KUBECONFIG_ERROR)?;
        validate_inline(&kubeconfig)?;
        let config = Config::from_custom_kubeconfig(kubeconfig, &config_options(context))
            .await
            .map_err(|_| INLINE_KUBECONFIG_ERROR)?;
        let auth = &config.auth_info;
        if auth.token.is_none()
            && (auth.client_certificate_data.is_none() || auth.client_key_data.is_none())
        {
            return Err(INLINE_KUBECONFIG_ERROR);
        }
        Self::connect(options, config)
    }

    fn connect(options: KubernetesOptions, mut config: Config) -> Result<Self, RuntimeError> {
        let namespace = &options.namespace;
        if !valid_dns_label(namespace) {
            return Err(RuntimeError::InvalidSpec("invalid Kubernetes namespace"));
        }
        let storage_class = &options.storage_class;
        if storage_class.len() > 253 || !storage_class.split('.').all(valid_dns_label) {
            return Err(RuntimeError::InvalidSpec(
                "invalid Kubernetes StorageClass name",
            ));
        }
        if !valid_relative_path(&options.seccomp_profile) {
            return Err(RuntimeError::InvalidSpec(
                "invalid Kubernetes localhost seccomp profile",
            ));
        }
        if config.accept_invalid_certs
            || config.cluster_url.scheme_str() != Some("https")
            || config
                .cluster_url
                .authority()
                .is_none_or(|authority| authority.as_str().contains('@'))
        {
            return Err(RuntimeError::InvalidSpec(
                "Kubernetes requires verified HTTPS",
            ));
        }
        config.connect_timeout = Some(Duration::from_secs(5));
        config.read_timeout = Some(Duration::from_secs(15));
        config.write_timeout = Some(Duration::from_secs(15));
        let client = tracing::dispatcher::with_default(&tracing::Dispatch::none(), || {
            Client::try_from(config)
        })
        .map_err(|_| {
            RuntimeError::InvalidSpec(
                "invalid Kubernetes client TLS or authentication configuration",
            )
        })?;
        Ok(Self::new(options, client))
    }

    /// Verify the identity's namespace permissions and confirm the namespace admits a PVC.
    /// The operator verifies the StorageClass and its reclaim and quota semantics separately.
    pub async fn check(&self, home_bytes: u64) -> Result<(), RuntimeError> {
        let reviews: Api<SelfSubjectAccessReview> = Api::all(self.client.clone());
        let resources: &[(&str, &str, &[&str])] = &[
            (
                "apps",
                "statefulsets",
                &["get", "create", "patch", "delete"],
            ),
            ("", "pods", &["get"]),
            ("", "persistentvolumeclaims", &["get", "create", "delete"]),
            ("batch", "jobs", &["get", "create", "delete"]),
            ("", "secrets", &["get", "create", "delete"]),
        ];
        try_join_all(
            resources
                .iter()
                .flat_map(|&(group, resource, verbs)| {
                    verbs.iter().map(move |&verb| (group, resource, verb))
                })
                .map(|(group, resource, verb)| {
                    let reviews = &reviews;
                    async move {
                        let review = SelfSubjectAccessReview {
                            spec: SelfSubjectAccessReviewSpec {
                                resource_attributes: Some(ResourceAttributes {
                                    group: Some(group.into()),
                                    namespace: Some(self.options.namespace.clone()),
                                    resource: Some(resource.into()),
                                    verb: Some(verb.into()),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            },
                            ..Default::default()
                        };
                        let result = kube_request(reviews.create(&PostParams::default(), &review))
                            .await
                            .map_err(api_error("verify Worker namespace permissions"))?;
                        if result.status.is_none_or(|status| !status.allowed) {
                            return Err(RuntimeError::Kubernetes {
                                operation: "verify Worker namespace permissions",
                                source: Box::new(PermissionDenied { resource, verb }),
                            });
                        }
                        Ok(())
                    }
                }),
        )
        .await?;

        let storage = super::objects::byte_quantity(home_bytes)?;
        let mut probe = self.home_claim("talos-worker-runtime-check", &storage);
        probe.metadata.name = None;
        probe.metadata.generate_name = Some("talos-worker-runtime-check-".into());
        kube_request(self.homes.create(
            &PostParams {
                dry_run: true,
                ..Default::default()
            },
            &probe,
        ))
        .await
        .map_err(api_error("verify Worker namespace admission"))?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Kubernetes identity cannot {verb} {resource} in the configured namespace")]
struct PermissionDenied {
    resource: &'static str,
    verb: &'static str,
}

fn config_options(context: Option<&str>) -> KubeConfigOptions {
    KubeConfigOptions {
        context: context.map(str::to_owned),
        ..Default::default()
    }
}

fn validate_inline(config: &Kubeconfig) -> Result<(), RuntimeError> {
    if config
        .clusters
        .iter()
        .filter_map(|named| named.cluster.as_ref())
        .any(|cluster| {
            cluster.insecure_skip_tls_verify == Some(true)
                || cluster.certificate_authority.is_some()
                || cluster.proxy_url.is_some()
        })
        || config
            .auth_infos
            .iter()
            .filter_map(|named| named.auth_info.as_ref())
            .any(|auth| {
                auth.exec.is_some()
                    || auth.auth_provider.is_some()
                    || auth.token_file.is_some()
                    || auth.client_certificate.is_some()
                    || auth.client_key.is_some()
                    || auth.impersonate.is_some()
                    || auth.impersonate_groups.is_some()
                    || auth.username.is_some()
                    || auth.password.is_some()
            })
    {
        return Err(INLINE_KUBECONFIG_ERROR);
    }
    Ok(())
}

fn valid_dns_label(name: &str) -> bool {
    let alphanumeric = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    let bytes = name.as_bytes();
    bytes.len() <= 63
        && bytes.first().copied().is_some_and(alphanumeric)
        && bytes.last().copied().is_some_and(alphanumeric)
        && bytes
            .iter()
            .copied()
            .all(|byte| alphanumeric(byte) || byte == b'-')
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
