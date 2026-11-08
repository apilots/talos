use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use talos_error::{ErrorCode, TalosError, TalosResult};
use uuid::Uuid;

use crate::TalosPaths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KubeconfigSource {
    InCluster,
    Mounted,
    Pasted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct KubernetesConfig {
    pub namespace: String,
    pub storage_class: String,
    pub seccomp_profile: String,
    /// Operator assertion for the verified cluster, namespace and StorageClass.
    pub home_quota_enforced: bool,
    pub kubeconfig_source: Option<KubeconfigSource>,
    pub kubeconfig_path: Option<PathBuf>,
    pub kubeconfig_context: Option<String>,
    pub pasted_kubeconfig: Option<Uuid>,
}

impl Default for KubernetesConfig {
    fn default() -> Self {
        Self {
            namespace: "default".into(),
            storage_class: String::new(),
            seccomp_profile: "talos/chromium-seccomp.json".into(),
            home_quota_enforced: false,
            kubeconfig_source: None,
            kubeconfig_path: None,
            kubeconfig_context: None,
            pasted_kubeconfig: None,
        }
    }
}

pub enum KubernetesCredential {
    InCluster,
    Mounted {
        path: PathBuf,
        context: Option<String>,
    },
    Pasted {
        path: PathBuf,
        context: Option<String>,
    },
}

impl KubernetesConfig {
    pub fn credential_source(&self) -> KubeconfigSource {
        self.kubeconfig_source
            .unwrap_or(if self.kubeconfig_path.is_some() {
                KubeconfigSource::Mounted
            } else {
                KubeconfigSource::InCluster
            })
    }

    pub fn credential(&self, paths: &TalosPaths) -> TalosResult<KubernetesCredential> {
        let context = self.kubeconfig_context.clone();
        let invalid = |message| TalosError::new(ErrorCode::InvalidRequest, message);
        match self.credential_source() {
            KubeconfigSource::InCluster => Ok(KubernetesCredential::InCluster),
            KubeconfigSource::Mounted => {
                let path = self
                    .kubeconfig_path
                    .as_ref()
                    .filter(|path| path.is_absolute());
                let path = path.cloned().ok_or_else(|| {
                    invalid("An absolute operator-mounted Kubernetes kubeconfig path is required.")
                })?;
                Ok(KubernetesCredential::Mounted { path, context })
            }
            KubeconfigSource::Pasted => {
                let reference = self.pasted_kubeconfig.ok_or_else(|| {
                    invalid("A pasted Kubernetes kubeconfig must be saved for this runtime.")
                })?;
                Ok(KubernetesCredential::Pasted {
                    path: pasted_path(paths, reference),
                    context,
                })
            }
        }
    }

    pub(super) fn validate(&self, paths: &TalosPaths) -> TalosResult<()> {
        let invalid = |message| TalosError::new(ErrorCode::InvalidRequest, message);
        if self.namespace.len() > 63 || !dns_label(&self.namespace) {
            return Err(invalid(
                "The Kubernetes namespace must be a valid DNS label.",
            ));
        }
        if self.storage_class.len() > 253 || !self.storage_class.split('.').all(dns_label) {
            return Err(invalid(
                "An explicit Kubernetes StorageClass name is required.",
            ));
        }
        if self.seccomp_profile.is_empty()
            || !Path::new(&self.seccomp_profile)
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
        {
            return Err(invalid(
                "The Kubernetes seccomp profile must be a relative kubelet path.",
            ));
        }
        if self.kubeconfig_context.as_ref().is_some_and(|context| {
            context.is_empty()
                || context.len() > 256
                || context.contains("${")
                || context.chars().any(char::is_control)
        }) {
            return Err(invalid("The Kubernetes kubeconfig context is invalid."));
        }
        self.credential(paths)?;
        Ok(())
    }
}

fn dns_label(label: &str) -> bool {
    let ascii_alphanumeric = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    let bytes = label.as_bytes();
    bytes.first().copied().is_some_and(ascii_alphanumeric)
        && bytes.last().copied().is_some_and(ascii_alphanumeric)
        && bytes
            .iter()
            .all(|&byte| ascii_alphanumeric(byte) || byte == b'-')
}

fn pasted_path(paths: &TalosPaths, reference: Uuid) -> PathBuf {
    paths
        .data_dir
        .join("worker-runtime-credentials")
        .join(format!("{reference}.yaml"))
}

/// Write a new immutable credential before publishing its reference in the Server YAML.
#[cfg(unix)]
pub fn write_pasted_kubeconfig(
    paths: &TalosPaths,
    reference: Uuid,
    contents: &str,
) -> TalosResult<()> {
    use std::os::unix::fs::DirBuilderExt;
    use talos_fs::{FilePermissions, WriteOptions};

    let path = pasted_path(paths, reference);
    let directory = path.parent().expect("credential directory");
    match std::fs::DirBuilder::new().mode(0o700).create(directory) {
        Ok(()) => std::fs::File::open(&paths.data_dir)
            .and_then(|directory| directory.sync_all())
            .map_err(credential_io)?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(credential_io(error)),
    }
    verify_private(directory, 0o700)?;
    talos_fs::write_bytes_with(
        &path,
        contents.as_bytes(),
        WriteOptions {
            create_parent: false,
            ..WriteOptions::default().with_permissions(FilePermissions::unix(0o600))
        },
    )
}

/// Read a Server-owned credential only for Runtime authentication and preflight.
#[cfg(unix)]
pub fn read_pasted_kubeconfig(path: &Path) -> TalosResult<String> {
    let directory = path.parent().ok_or_else(|| {
        TalosError::internal("The Server's private kubeconfig has no parent directory.")
    })?;
    verify_private(directory, 0o700)?;
    verify_private(path, 0o600)?;
    std::fs::read_to_string(path).map_err(credential_io)
}

#[cfg(unix)]
fn verify_private(path: &Path, expected_mode: u32) -> TalosResult<()> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::symlink_metadata(path).map_err(credential_io)?;
    let expected_type = if expected_mode == 0o700 {
        metadata.is_dir()
    } else {
        metadata.is_file()
    };
    if !expected_type || metadata.permissions().mode() & 0o777 != expected_mode {
        return Err(TalosError::new(
            ErrorCode::ServiceUnavailable,
            "The Server kubeconfig storage does not have private permissions.",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn credential_io(error: std::io::Error) -> TalosError {
    TalosError::internal_with_source("Access the Server's private kubeconfig storage.", error)
}

#[cfg(not(unix))]
pub fn write_pasted_kubeconfig(_: &TalosPaths, _: Uuid, _: &str) -> TalosResult<()> {
    Err(private_storage_unavailable())
}

#[cfg(not(unix))]
pub fn read_pasted_kubeconfig(_: &Path) -> TalosResult<String> {
    Err(private_storage_unavailable())
}

#[cfg(not(unix))]
fn private_storage_unavailable() -> TalosError {
    TalosError::new(
        ErrorCode::ServiceUnavailable,
        "Private kubeconfig storage requires a Unix Server.",
    )
}

#[cfg(all(test, unix))]
#[path = "kubernetes_worker_tests.rs"]
mod tests;
