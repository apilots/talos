use serde_json::Value;
use std::path::{Path, PathBuf};

use talos_error::TalosResult;
use talos_fs::FileSystem;

use crate::{env::RuntimeEnv, validate, Config, DatabaseType, EnvSecretStore, TalosPaths};

pub(crate) const TALOS_RESOURCE_DIR_ENV: &str = "TALOS_RESOURCE_DIR";
const TALOS_CONFIG_ENV: &str = "TALOS_CONFIG";
const DEFAULT_CONFIG_RELATIVE_PATH: &str = "conf/talos.yaml";
const DEFAULT_MIGRATIONS_RELATIVE_PATH: &str = "conf/migrations";

/// Result of installing product-managed database migration resources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseMigrationsInstallReport {
    pub source_dir: PathBuf,
    pub target_dir: PathBuf,
    pub backend_dir: PathBuf,
    pub database_type: DatabaseType,
    pub changed: bool,
}

/// Replace the runtime migration resources and return the selected backend directory.
///
/// The release resource directory owns migration files. This operation replaces only
/// `<talos_home>/conf/migrations` and never modifies the user's `talos.yaml`.
pub fn install_database_migrations(
    config: &Config,
) -> TalosResult<DatabaseMigrationsInstallReport> {
    let paths = config.talos_paths();
    replace_migration_resources_from_candidates(
        &paths,
        config.storage.database_type,
        vec![database_migration_resource_dir()?],
    )
}

pub(crate) fn load_config() -> TalosResult<Config> {
    load_config_from_path(None)
}

pub(crate) fn load_config_from_path(path: Option<&str>) -> TalosResult<Config> {
    use talos_error::TalosError;

    let paths = TalosPaths::resolve();
    let explicit_path = explicit_config_path(path);
    let config_path = match explicit_path {
        Some(path) => path,
        None => {
            ensure_first_run_files(&paths)?;
            paths.config_file.clone()
        }
    };

    if !config_path.exists() {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!("no config file found at {}", config_path.display()),
        ));
    }

    if path.is_some() || std::env::var_os(TALOS_CONFIG_ENV).is_some() {
        ensure_runtime_env_file(&paths)?;
    }
    load_config_at(config_path, paths)
}

pub(crate) fn load_config_at(config_path: PathBuf, paths: TalosPaths) -> TalosResult<Config> {
    use talos_error::TalosError;
    let runtime_env = RuntimeEnv::load(&paths.env_file)?;
    let raw = std::fs::read_to_string(&config_path).map_err(|err| {
        TalosError::internal_with_source(
            format!("failed to read config file {}", config_path.display()),
            err,
        )
    })?;
    let yaml_value: serde_yaml::Value = serde_yaml::from_str(&raw).map_err(|err| {
        TalosError::from_source(
            talos_error::ErrorCode::InvalidRequest,
            "failed to parse configuration YAML",
            err,
        )
    })?;
    let raw_value: Value = serde_json::to_value(yaml_value).map_err(|err| {
        TalosError::internal_with_source("failed to convert configuration YAML", err)
    })?;

    validate::validate_required_paths(&raw_value)?;
    validate::validate_forbidden_paths(&raw_value)?;

    // Deserialize into typed config after strict key presence validation.
    let config: Config = serde_json::from_value(raw_value).map_err(|error| {
        TalosError::from_source(
            talos_error::ErrorCode::InvalidRequest,
            "invalid configuration value",
            error,
        )
    })?;
    let mut config = resolve_env_placeholders(config, &runtime_env)?.with_runtime_paths(paths);
    config.source_path = Some(config_path);
    config.apply_derived_paths();
    config.attach_runtime_env(runtime_env);
    Ok(config)
}

fn explicit_config_path(path: Option<&str>) -> Option<PathBuf> {
    path.map(PathBuf::from)
        .or_else(|| std::env::var_os(TALOS_CONFIG_ENV).map(PathBuf::from))
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
    copy_default_config_template_from_candidates(paths, vec![default_config_resource_path()?])
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
        return Err(TalosError::new(talos_error::ErrorCode::InvalidRequest, format!(
            "missing default config template: expected {DEFAULT_CONFIG_RELATIVE_PATH} under {TALOS_RESOURCE_DIR_ENV}; searched: {searched}"
        )));
    };

    talos_fs::copy_file(template_path, &paths.config_file).map_err(|err| {
        err.context(format!(
            "copy default config template from {} to {}",
            template_path.display(),
            paths.config_file.display()
        ))
    })?;
    Ok(())
}

pub(crate) fn default_config_resource_path() -> TalosResult<PathBuf> {
    Ok(default_config_template_path(required_resource_dir()?))
}

pub(crate) fn default_config_template_path(resource_dir: impl AsRef<Path>) -> PathBuf {
    resource_dir.as_ref().join(DEFAULT_CONFIG_RELATIVE_PATH)
}

fn database_migration_resource_dir() -> TalosResult<PathBuf> {
    Ok(required_resource_dir()?.join(DEFAULT_MIGRATIONS_RELATIVE_PATH))
}

pub(crate) fn required_resource_dir() -> TalosResult<PathBuf> {
    use talos_error::TalosError;

    std::env::var_os(TALOS_RESOURCE_DIR_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                format!(
                    "{TALOS_RESOURCE_DIR_ENV} must be set to the Talos installation resource root"
                ),
            )
        })
}

fn replace_migration_resources_from_candidates<I>(
    paths: &TalosPaths,
    database_type: DatabaseType,
    candidates: I,
) -> TalosResult<DatabaseMigrationsInstallReport>
where
    I: IntoIterator<Item = PathBuf>,
{
    use talos_error::TalosError;

    let candidates: Vec<PathBuf> = candidates.into_iter().collect();
    let Some(source_dir) = candidates.iter().find(|path| path.is_dir()) else {
        let searched = candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(TalosError::new(talos_error::ErrorCode::InvalidRequest, format!(
            "missing database migrations: expected {DEFAULT_MIGRATIONS_RELATIVE_PATH} under {TALOS_RESOURCE_DIR_ENV}; searched: {searched}"
        )));
    };

    let target_dir = &paths.conf_dir.join("migrations");
    let changed = source_dir.canonicalize().ok() != target_dir.canonicalize().ok();
    if changed {
        replace_directory(source_dir, target_dir)?;
    }

    let backend_dir = paths.database_migrations_dir(database_type);
    if !backend_dir.is_dir() {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!(
                "missing {} database migrations at {}",
                database_type,
                backend_dir.display()
            ),
        ));
    }
    Ok(DatabaseMigrationsInstallReport {
        source_dir: source_dir.clone(),
        target_dir: target_dir.clone(),
        backend_dir,
        database_type,
        changed,
    })
}

fn replace_directory(source: &Path, target: &Path) -> TalosResult<()> {
    use talos_error::TalosError;

    let parent = target.parent().ok_or_else(|| {
        TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!(
                "database migration target has no parent: {}",
                target.display()
            ),
        )
    })?;
    std::fs::create_dir_all(parent).map_err(|error| {
        let diagnostic = format!(
            "create database migration parent {}: {error}",
            parent.display()
        );
        TalosError::internal_with_source(
            "Create database migration parent; technical details were withheld.",
            error,
        )
        .context(diagnostic)
    })?;

    let staging = parent.join(".migrations-installing");
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(|error| {
            { let diagnostic = format!(
                "remove stale database migration staging directory {}: {error}",
                staging.display()
            ); TalosError::internal_with_source("Remove stale database migration staging directory; technical details were withheld.", error).context(diagnostic) }
        })?;
    }
    copy_directory(source, &staging)?;

    if target.exists() {
        std::fs::remove_dir_all(target).map_err(|error| {
            let diagnostic = format!(
                "remove existing database migrations {}: {error}",
                target.display()
            );
            TalosError::internal_with_source(
                "Remove existing database migrations; technical details were withheld.",
                error,
            )
            .context(diagnostic)
        })?;
    }
    std::fs::rename(&staging, target).map_err(|error| {
        let diagnostic = format!(
            "install database migrations from {} to {}: {error}",
            source.display(),
            target.display()
        );
        TalosError::internal_with_source(
            "Install database migrations; technical details were withheld.",
            error,
        )
        .context(diagnostic)
    })
}

fn copy_directory(source: &Path, target: &Path) -> TalosResult<()> {
    use talos_error::TalosError;

    std::fs::create_dir_all(target).map_err(|error| {
        let diagnostic = format!(
            "create database migration directory {}: {error}",
            target.display()
        );
        TalosError::internal_with_source(
            "Create database migration directory; technical details were withheld.",
            error,
        )
        .context(diagnostic)
    })?;
    for entry in std::fs::read_dir(source).map_err(|error| {
        let diagnostic = format!(
            "read database migration directory {}: {error}",
            source.display()
        );
        TalosError::internal_with_source(
            "Read database migration directory; technical details were withheld.",
            error,
        )
        .context(diagnostic)
    })? {
        let entry = entry.map_err(|error| {
            let diagnostic = format!(
                "read database migration entry under {}: {error}",
                source.display()
            );
            TalosError::internal_with_source(
                "Read database migration entry; technical details were withheld.",
                error,
            )
            .context(diagnostic)
        })?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let file_type = entry.file_type().map_err(|error| {
            let diagnostic = format!(
                "inspect database migration resource {}: {error}",
                source_path.display()
            );
            TalosError::internal_with_source(
                "Inspect database migration resource; technical details were withheld.",
                error,
            )
            .context(diagnostic)
        })?;
        if file_type.is_dir() {
            copy_directory(&source_path, &target_path)?;
        } else if file_type.is_file() {
            std::fs::copy(&source_path, &target_path).map_err(|error| {
                let diagnostic = format!(
                    "copy database migration resource {} to {}: {error}",
                    source_path.display(),
                    target_path.display()
                );
                TalosError::internal_with_source(
                    "Copy database migration resource; technical details were withheld.",
                    error,
                )
                .context(diagnostic)
            })?;
        }
    }
    Ok(())
}

fn ensure_runtime_env_file(paths: &TalosPaths) -> TalosResult<()> {
    paths.ensure_base_dirs()?;
    let env_store = EnvSecretStore::new(paths.env_file.clone());
    env_store.ensure_file()?;
    Ok(())
}

pub(crate) fn save_config(config: &Config, path: &str) -> TalosResult<()> {
    use talos_error::TalosError;

    let yaml = serde_yaml::to_string(config).map_err(|e| {
        TalosError::internal_with_source(
            "Failed to serialize config; technical details were withheld.",
            e,
        )
    })?;

    talos_fs::std_fs()
        .write_file(Path::new(path), &yaml)
        .map_err(|error| error.context("write configuration file"))?;

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

    let mut value = serde_json::to_value(config).map_err(|e| {
        TalosError::internal_with_source(
            "Failed to encode config; technical details were withheld.",
            e,
        )
    })?;
    resolve_value_placeholders(&mut value, runtime_env);
    serde_json::from_value(value).map_err(|e| {
        TalosError::internal_with_source(
            "Failed to decode config; technical details were withheld.",
            e,
        )
    })
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
#[path = "loader_tests.rs"]
mod tests;
