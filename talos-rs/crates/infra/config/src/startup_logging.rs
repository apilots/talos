//! Partial startup settings share Config's environment resolution before logging exists.
use crate::{default_timezone, env::RuntimeEnv, loader, TalosPaths};
use talos_error::TalosResult;
use talos_logging::LoggingConfig;

/// Read only startup logging settings before upgrades and strict config loading.
/// Root timezone and logging strings use the same dotenv/process environment
/// resolver as the full Config loader. This function never initializes a sink,
/// mutates the environment, runs upgrades or contacts remote services.
/// Missing or malformed YAML retains startup defaults and a deferred warning;
/// full configuration validation remains the normal loader's responsibility.
pub fn load_startup_logging(
    paths: &TalosPaths,
) -> TalosResult<(LoggingConfig, String, Option<String>)> {
    let runtime_env = RuntimeEnv::load(&paths.env_file)?;
    Ok(startup_logging_config(paths, &runtime_env))
}

fn startup_logging_config(
    paths: &TalosPaths,
    runtime_env: &RuntimeEnv,
) -> (LoggingConfig, String, Option<String>) {
    let config = LoggingConfig {
        file: Some(paths.log_file.to_string_lossy().to_string()),
        ..LoggingConfig::default()
    };
    let raw = match std::fs::read_to_string(&paths.config_file) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (config, default_timezone(), None);
        }
        Err(error) => {
            return (
                config,
                default_timezone(),
                Some(format!(
                    "failed to read logging config {}: {error}",
                    paths.config_file.display()
                )),
            )
        }
    };
    let mut root = match serde_yaml::from_str::<serde_json::Value>(&raw) {
        Ok(root) => root,
        Err(error) => {
            return (
                config,
                default_timezone(),
                Some(format!(
                    "failed to parse logging config yaml {}: {error}",
                    paths.config_file.display()
                )),
            )
        }
    };
    loader::resolve_value_placeholders(&mut root, runtime_env);
    let timezone = root
        .get("timezone")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(default_timezone);
    let Some(logging) = root.get("logging") else {
        return (config, timezone, None);
    };
    match serde_json::from_value::<LoggingConfig>(logging.clone()) {
        Ok(mut parsed) => {
            if parsed.file.is_none() {
                parsed.file = config.file;
            }
            (parsed, timezone, None)
        }
        Err(error) => (
            config,
            timezone,
            Some(format!(
                "failed to decode logging config {}: {error}",
                paths.config_file.display()
            )),
        ),
    }
}

#[cfg(test)]
#[path = "startup_logging_tests.rs"]
mod tests;
