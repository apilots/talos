use serde_json::Value;
use std::path::{Path, PathBuf};

use talos_error::TalosResult;
use talos_fs::FileSystem;

use crate::{env::RuntimeEnv, validate, Config, EnvSecretStore, TalosPaths};

const TALOS_RESOURCE_DIR_ENV: &str = "TALOS_RESOURCE_DIR";
const TALOS_CONFIG_ENV: &str = "TALOS_CONFIG";
const DEFAULT_CONFIG_RELATIVE_PATH: &str = "conf/talos.yaml";

pub(crate) fn load_config() -> TalosResult<Config> {
    load_config_from_path(None)
}

pub(crate) fn load_config_from_path(path: Option<&str>) -> TalosResult<Config> {
    use talos_error::TalosError;

    let paths = TalosPaths::resolve();
    let explicit_path = path
        .map(PathBuf::from)
        .or_else(|| std::env::var_os(TALOS_CONFIG_ENV).map(PathBuf::from));
    let config_path = match explicit_path {
        Some(path) => path,
        None => {
            ensure_first_run_files(&paths)?;
            paths.config_file.clone()
        }
    };

    if !config_path.exists() {
        return Err(TalosError::InvalidConfiguration(format!(
            "no config file found at {}",
            config_path.display()
        )));
    }

    if path.is_some() || std::env::var_os(TALOS_CONFIG_ENV).is_some() {
        ensure_runtime_env_file(&paths)?;
    }
    let runtime_env = RuntimeEnv::load(&paths.env_file)?;
    let raw = std::fs::read_to_string(&config_path).map_err(|err| {
        TalosError::ConfigError(format!(
            "failed to read config file {}: {}",
            config_path.display(),
            err
        ))
    })?;
    let yaml_value: serde_yaml::Value = serde_yaml::from_str(&raw)
        .map_err(|err| TalosError::ConfigError(format!("failed to parse yaml: {}", err)))?;
    let raw_value: Value = serde_json::to_value(yaml_value)
        .map_err(|err| TalosError::ConfigError(format!("failed to convert yaml: {}", err)))?;

    validate::validate_required_paths(&raw_value)?;
    validate::validate_forbidden_paths(&raw_value)?;

    // Deserialize into typed config after strict key presence validation.
    let config: Config =
        serde_json::from_value(raw_value).map_err(|e| TalosError::ConfigError(e.to_string()))?;
    let mut config = resolve_env_placeholders(config, &runtime_env)?;
    config.apply_derived_paths();
    config
        .talos_paths()
        .ensure_agent_workspace(&config.agent.name)?;

    Ok(config)
}

fn ensure_first_run_files(paths: &TalosPaths) -> TalosResult<()> {
    paths.ensure_base_dirs()?;
    if !paths.config_file.exists() {
        copy_default_config_template(paths)?;
    }
    ensure_runtime_env_file(paths)?;
    Ok(())
}

fn copy_default_config_template(paths: &TalosPaths) -> TalosResult<()> {
    copy_default_config_template_from_candidates(paths, default_config_template_candidates())
}

fn copy_default_config_template_from_candidates<I>(
    paths: &TalosPaths,
    candidates: I,
) -> TalosResult<()>
where
    I: IntoIterator<Item = PathBuf>,
{
    use talos_error::TalosError;

    let candidates: Vec<PathBuf> = candidates.into_iter().collect();
    let Some(template_path) = candidates.iter().find(|path| path.is_file()) else {
        let searched = candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(TalosError::InvalidConfiguration(format!(
            "missing default config template: expected {DEFAULT_CONFIG_RELATIVE_PATH} under {TALOS_RESOURCE_DIR_ENV} or installed resource directory; searched: {searched}"
        )));
    };

    talos_fs::copy_file(template_path, &paths.config_file).map_err(|err| {
        TalosError::ConfigError(format!(
            "failed to copy default config template from {} to {}: {}",
            template_path.display(),
            paths.config_file.display(),
            err
        ))
    })?;
    Ok(())
}

fn default_config_template_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(resource_dir) = std::env::var_os(TALOS_RESOURCE_DIR_ENV) {
        candidates.push(default_config_template_path(PathBuf::from(resource_dir)));
    }

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(bin_dir) = current_exe.parent() {
            candidates.push(default_config_template_path(bin_dir));
            if let Some(parent_dir) = bin_dir.parent() {
                candidates.push(default_config_template_path(parent_dir));
            }
        }
    }

    candidates
}

fn default_config_template_path(resource_dir: impl AsRef<Path>) -> PathBuf {
    resource_dir.as_ref().join(DEFAULT_CONFIG_RELATIVE_PATH)
}

fn ensure_runtime_env_file(paths: &TalosPaths) -> TalosResult<()> {
    paths.ensure_base_dirs()?;
    let env_store = EnvSecretStore::new(paths.env_file.clone());
    env_store.ensure_file()?;
    Ok(())
}

pub(crate) fn save_config(config: &Config, path: &str) -> TalosResult<()> {
    use talos_error::TalosError;

    let yaml = serde_yaml::to_string(config)
        .map_err(|e| TalosError::ConfigError(format!("Failed to serialize config: {}", e)))?;

    talos_fs::std_fs()
        .write_file(Path::new(path), &yaml)
        .map_err(|e| TalosError::ConfigError(format!("Failed to write config file: {}", e)))?;

    Ok(())
}

pub(crate) fn default_config_path() -> PathBuf {
    TalosPaths::resolve().config_file
}

pub(crate) fn resolve_env_placeholders(
    config: Config,
    runtime_env: &RuntimeEnv,
) -> TalosResult<Config> {
    use talos_error::TalosError;

    let mut value = serde_json::to_value(config)
        .map_err(|e| TalosError::ConfigError(format!("Failed to encode config: {}", e)))?;
    resolve_value_placeholders(&mut value, runtime_env);
    serde_json::from_value(value)
        .map_err(|e| TalosError::ConfigError(format!("Failed to decode config: {}", e)))
}

pub(crate) fn resolve_value_placeholders(value: &mut Value, runtime_env: &RuntimeEnv) {
    match value {
        Value::String(text) => {
            *text = expand_env_in_string_with_env(text, runtime_env);
        }
        Value::Array(items) => {
            for item in items {
                resolve_value_placeholders(item, runtime_env);
            }
        }
        Value::Object(map) => {
            for v in map.values_mut() {
                resolve_value_placeholders(v, runtime_env);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
pub(crate) fn expand_env_in_string(input: &str) -> String {
    let runtime_env = RuntimeEnv::load_from_iter(
        std::path::Path::new("__talos_missing_env_file__"),
        std::env::vars(),
    )
    .unwrap_or_default();
    expand_env_in_string_with_env(input, &runtime_env)
}

fn expand_env_in_string_with_env(input: &str, runtime_env: &RuntimeEnv) -> String {
    let mut result = String::with_capacity(input.len());
    let mut cursor = 0usize;
    while let Some(start_rel) = input[cursor..].find("${") {
        let start = cursor + start_rel;
        result.push_str(&input[cursor..start]);
        let after_start = start + 2;
        if let Some(end_rel) = input[after_start..].find('}') {
            let end = after_start + end_rel;
            let var_name = &input[after_start..end];
            let replacement = runtime_env
                .get(var_name)
                .map(str::to_string)
                .unwrap_or_else(|| format!("${{{}}}", var_name));
            result.push_str(&replacement);
            cursor = end + 1;
        } else {
            result.push_str(&input[start..]);
            cursor = input.len();
        }
    }
    if cursor < input.len() {
        result.push_str(&input[cursor..]);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_resource_template(root: &Path, content: &str) -> PathBuf {
        let conf_dir = root.join("conf");
        std::fs::create_dir_all(&conf_dir).expect("resource conf dir should be created");
        let template_path = conf_dir.join("talos.yaml");
        std::fs::write(&template_path, content).expect("resource template should be written");
        root.to_path_buf()
    }

    #[test]
    fn first_run_files_create_config_and_env_secret() {
        let dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = TalosPaths::from_home(dir.path().join(".talos"));
        let resource_dir = write_resource_template(
            dir.path(),
            "gateway:\n  auth: true\nllm:\n  anthropic:\n    api_key: ${TALOS_LLM_ANTHROPIC_API_KEY}\n",
        );

        paths
            .ensure_base_dirs()
            .expect("base dirs should be created");
        copy_default_config_template_from_candidates(
            &paths,
            vec![default_config_template_path(resource_dir)],
        )
        .expect("default config should copy");
        ensure_runtime_env_file(&paths).expect("env file should be created");

        let config =
            std::fs::read_to_string(&paths.config_file).expect("default config should be written");
        let env = std::fs::read_to_string(&paths.env_file).expect("env file should be written");
        assert!(config.contains("gateway:"));
        assert!(config.contains("auth: true"));
        assert!(config.contains("${TALOS_LLM_ANTHROPIC_API_KEY}"));
        assert!(env.is_empty());
    }

    #[test]
    fn first_run_files_do_not_overwrite_existing_config_or_secret() {
        let dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = TalosPaths::from_home(dir.path().join(".talos"));
        let resource_dir = write_resource_template(dir.path(), "gateway:\n  auth: true\n");
        paths
            .ensure_base_dirs()
            .expect("base dirs should be created");
        std::fs::write(&paths.config_file, "gateway:\n  host: custom\n")
            .expect("config should write");
        std::fs::write(&paths.env_file, "TALOS_CLOUD_API_TOKEN=existing\n")
            .expect("env should write");

        if !paths.config_file.exists() {
            copy_default_config_template_from_candidates(
                &paths,
                vec![default_config_template_path(resource_dir)],
            )
            .expect("default config should copy");
        }
        ensure_runtime_env_file(&paths).expect("env should stay idempotent");

        let config = std::fs::read_to_string(&paths.config_file).expect("config should read");
        let env = std::fs::read_to_string(&paths.env_file).expect("env should read");
        assert_eq!(config, "gateway:\n  host: custom\n");
        assert_eq!(env, "TALOS_CLOUD_API_TOKEN=existing\n");
    }

    #[test]
    fn first_run_config_copy_fails_when_resource_template_is_missing() {
        let dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = TalosPaths::from_home(dir.path().join(".talos"));
        paths
            .ensure_base_dirs()
            .expect("base dirs should be created");

        let err = copy_default_config_template_from_candidates(
            &paths,
            vec![dir.path().join("missing/conf/talos.yaml")],
        )
        .expect_err("missing resource template should fail");

        assert!(err.to_string().contains("missing default config template"));
        assert!(err.to_string().contains("TALOS_RESOURCE_DIR"));
    }
}
