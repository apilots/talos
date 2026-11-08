use serde::Serialize;
use serde_yaml::{Mapping, Value};
use talos_error::{TalosError, TalosResult};
use talos_fs::FileSystem;

use super::{nested_mapping, parse_yaml_root, YamlConfigStore};
use crate::WorkerRuntimeConfig;

impl YamlConfigStore {
    /// Patch the admin-editable keys, preserving deployment fields and environment placeholders.
    pub fn save_worker_runtime_selection(
        &self,
        previous: &WorkerRuntimeConfig,
        next: &WorkerRuntimeConfig,
    ) -> TalosResult<()> {
        let _lock = talos_fs::std_fs().with_file_lock(&self.lock_path())?;
        let mut root = parse_yaml_root(&self.read_config_text()?)?;
        let root_map = root
            .as_mapping_mut()
            .ok_or_else(|| TalosError::internal("Config root is not a YAML mapping."))?;
        let talos = nested_mapping(root_map, "talos")?;
        let worker = nested_mapping(talos, "worker")?;
        let runtime = worker
            .get_mut("runtime")
            .and_then(Value::as_mapping_mut)
            .ok_or_else(|| TalosError::internal("The Server runtime configuration is missing."))?;
        set_changed(runtime, "kind", &previous.kind, &next.kind)?;
        if previous.kubernetes != next.kubernetes {
            let kubernetes = nested_mapping(runtime, "kubernetes")?;
            let previous = &previous.kubernetes;
            let next = &next.kubernetes;
            set_changed(
                kubernetes,
                "namespace",
                &previous.namespace,
                &next.namespace,
            )?;
            set_changed(
                kubernetes,
                "storage_class",
                &previous.storage_class,
                &next.storage_class,
            )?;
            set_changed(
                kubernetes,
                "kubeconfig_source",
                &previous.kubeconfig_source,
                &next.kubeconfig_source,
            )?;
            set_changed(
                kubernetes,
                "kubeconfig_context",
                &previous.kubeconfig_context,
                &next.kubeconfig_context,
            )?;
            set_changed(
                kubernetes,
                "pasted_kubeconfig",
                &previous.pasted_kubeconfig,
                &next.pasted_kubeconfig,
            )?;
            set_changed(
                kubernetes,
                "home_quota_enforced",
                &previous.home_quota_enforced,
                &next.home_quota_enforced,
            )?;
        }
        let text = serde_yaml::to_string(&root).map_err(|error| {
            TalosError::internal_with_source("Serialize the Server runtime configuration.", error)
        })?;
        talos_fs::std_fs().write_file(&self.config_path, &text)
    }
}

fn set_changed<T: PartialEq + Serialize>(
    map: &mut Mapping,
    key: &str,
    previous: &T,
    next: &T,
) -> TalosResult<()> {
    if previous != next {
        let value = serde_yaml::to_value(next).map_err(|error| {
            TalosError::internal_with_source("Serialize a Server runtime setting.", error)
        })?;
        map.insert(Value::String(key.into()), value);
    }
    Ok(())
}

#[cfg(test)]
#[path = "worker_runtime_tests.rs"]
mod tests;
