use serde_yaml::{Mapping, Value as YamlValue};
use std::path::{Path, PathBuf};

use talos_error::{TalosError, TalosResult};
use talos_fs::{FileSystem, WriteOptions};

use crate::env::EnvSecretStore;
use crate::loader::default_config_resource_path;
use crate::{validate, Config, TalosPaths};

/// Result of refreshing the product-managed default config template cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigTemplateRefreshReport {
    pub source_path: PathBuf,
    pub target_path: PathBuf,
    pub changed: bool,
}

/// Result of normalizing the product-managed default config file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUpgradeReport {
    pub source_path: PathBuf,
    pub changed: bool,
    pub backup_path: Option<PathBuf>,
    pub added_paths: Vec<String>,
    pub removed_paths: Vec<String>,
    pub defaulted_paths: Vec<String>,
}

impl ConfigUpgradeReport {
    fn unchanged(source_path: PathBuf) -> Self {
        Self {
            source_path,
            changed: false,
            backup_path: None,
            added_paths: Vec::new(),
            removed_paths: Vec::new(),
            defaulted_paths: Vec::new(),
        }
    }
}

#[derive(Debug, Default)]
struct NormalizeReport {
    added_paths: Vec<String>,
    removed_paths: Vec<String>,
    defaulted_paths: Vec<String>,
}

/// Upgrade the default `$TALOS_HOME/conf/talos.yaml` to the current strict config shape.
pub fn upgrade_default_config() -> TalosResult<ConfigUpgradeReport> {
    let paths = TalosPaths::resolve();
    upgrade_default_config_from_template(&paths, &paths.config_template_file)
}

/// Refresh `$TALOS_HOME/conf/talos.yaml.template` from `$TALOS_RESOURCE_DIR/conf/talos.yaml`.
pub fn refresh_default_config_template() -> TalosResult<ConfigTemplateRefreshReport> {
    refresh_default_config_template_from_source(
        &TalosPaths::resolve(),
        &default_config_resource_path()?,
    )
}

pub(crate) fn refresh_default_config_template_from_source(
    paths: &TalosPaths,
    source_path: &Path,
) -> TalosResult<ConfigTemplateRefreshReport> {
    paths.ensure_base_dirs()?;

    if !source_path.is_file() {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!(
                "missing default config template source: {}",
                source_path.display()
            ),
        ));
    }

    let source = std::fs::read_to_string(source_path).map_err(|err| {
        let diagnostic = format!(
            "read default config template source {}: {err}",
            source_path.display()
        );
        TalosError::internal_with_source(
            "Read default config template source; technical details were withheld.",
            err,
        )
        .context(diagnostic)
    })?;
    let changed = std::fs::read_to_string(&paths.config_template_file)
        .map(|existing| existing != source)
        .unwrap_or(true);
    if changed {
        talos_fs::std_fs()
            .write_file_with(
                &paths.config_template_file,
                &source,
                WriteOptions::default(),
            )
            .map_err(|err| {
                err.context(format!(
                    "write default config template cache {}",
                    paths.config_template_file.display()
                ))
            })?;
    }

    Ok(ConfigTemplateRefreshReport {
        source_path: source_path.to_path_buf(),
        target_path: paths.config_template_file.clone(),
        changed,
    })
}

pub(crate) fn upgrade_default_config_from_template(
    paths: &TalosPaths,
    template_path: &Path,
) -> TalosResult<ConfigUpgradeReport> {
    paths.ensure_base_dirs()?;
    EnvSecretStore::new(paths.env_file.clone()).ensure_file()?;

    if !template_path.is_file() {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!(
                "missing managed default config template cache: {}",
                template_path.display()
            ),
        ));
    }

    if !paths.config_file.exists() {
        talos_fs::copy_file(template_path, &paths.config_file).map_err(|err| {
            err.context(format!(
                "copy default config template from {} to {}",
                template_path.display(),
                paths.config_file.display()
            ))
        })?;
        return Ok(ConfigUpgradeReport {
            source_path: paths.config_file.clone(),
            changed: true,
            backup_path: None,
            added_paths: Vec::new(),
            removed_paths: Vec::new(),
            defaulted_paths: Vec::new(),
        });
    }

    let original = read_yaml_file(&paths.config_file)?;
    let template = read_yaml_file(template_path)?;
    validate_config_value(&template).map_err(|err| {
        err.context(format!(
            "validate default config template {}",
            template_path.display()
        ))
    })?;
    let validation_template = build_validation_template(&template, &original);

    let mut normalize_report = NormalizeReport::default();
    let normalized = normalize_value(
        &template,
        &original,
        &validation_template,
        &mut Vec::new(),
        &mut normalize_report,
    );

    validate_config_value(&normalized).map_err(|err| {
        err.context(format!(
            "validate normalized config {}",
            paths.config_file.display()
        ))
    })?;

    if normalized == original {
        return Ok(ConfigUpgradeReport::unchanged(paths.config_file.clone()));
    }

    let backup_path = write_backup(&paths.config_file)?;
    let output = serde_yaml::to_string(&normalized).map_err(|err| {
        let diagnostic = format!(
            "serialize normalized config {}: {err}",
            paths.config_file.display()
        );
        TalosError::internal_with_source(
            "Serialize normalized config; technical details were withheld.",
            err,
        )
        .context(diagnostic)
    })?;
    talos_fs::std_fs()
        .write_file_with(&paths.config_file, &output, WriteOptions::default())
        .map_err(|err| {
            err.context(format!(
                "write normalized config {}",
                paths.config_file.display()
            ))
        })?;

    Ok(ConfigUpgradeReport {
        source_path: paths.config_file.clone(),
        changed: true,
        backup_path: Some(backup_path),
        added_paths: normalize_report.added_paths,
        removed_paths: normalize_report.removed_paths,
        defaulted_paths: normalize_report.defaulted_paths,
    })
}

fn read_yaml_file(path: &Path) -> TalosResult<YamlValue> {
    let raw = std::fs::read_to_string(path).map_err(|err| {
        let diagnostic = format!("read config yaml {}: {err}", path.display());
        TalosError::internal_with_source("Read config yaml; technical details were withheld.", err)
            .context(diagnostic)
    })?;
    serde_yaml::from_str(&raw).map_err(|err| {
        let diagnostic = format!("parse config yaml {}: {err}", path.display());
        TalosError::internal_with_source("Parse config yaml; technical details were withheld.", err)
            .context(diagnostic)
    })
}

fn build_validation_template(template: &YamlValue, original: &YamlValue) -> YamlValue {
    let mut validation_template = template.clone();
    let backends_path = ["storage", "files", "backends"];
    let default_path = ["storage", "files", "default"];
    if let Some(original_backends) = get_path(original, &backends_path) {
        let backends_path = backends_path
            .iter()
            .map(|segment| (*segment).to_string())
            .collect::<Vec<_>>();
        let default_path = default_path
            .iter()
            .map(|segment| (*segment).to_string())
            .collect::<Vec<_>>();
        let mut candidate = template.clone();
        if set_path(&mut candidate, &backends_path, original_backends.clone()).is_ok() {
            if let Some(original_default) = get_path(original, &["storage", "files", "default"]) {
                let mut candidate_with_default = candidate.clone();
                if set_path(
                    &mut candidate_with_default,
                    &default_path,
                    original_default.clone(),
                )
                .is_ok()
                    && validate_config_value(&candidate_with_default).is_ok()
                {
                    validation_template = candidate_with_default;
                    return validation_template;
                }
            }
            if validate_config_value(&candidate).is_ok() {
                validation_template = candidate;
            }
        }
    }
    validation_template
}

fn get_path<'a>(root: &'a YamlValue, path: &[&str]) -> Option<&'a YamlValue> {
    let mut cursor = root;
    for segment in path {
        let YamlValue::Mapping(map) = cursor else {
            return None;
        };
        cursor = map.get(YamlValue::String((*segment).to_string()))?;
    }
    Some(cursor)
}

fn normalize_value(
    template: &YamlValue,
    original: &YamlValue,
    root_template: &YamlValue,
    path: &mut Vec<String>,
    report: &mut NormalizeReport,
) -> YamlValue {
    if is_dynamic_map_path(path) && validates_at_path(root_template, path, original) {
        return original.clone();
    }

    match (template, original) {
        (YamlValue::Mapping(template_map), YamlValue::Mapping(original_map)) => {
            normalize_mapping(template_map, original_map, root_template, path, report)
        }
        _ => {
            if validates_at_path(root_template, path, original) {
                original.clone()
            } else {
                report.defaulted_paths.push(format_path(path));
                template.clone()
            }
        }
    }
}

fn normalize_mapping(
    template_map: &Mapping,
    original_map: &Mapping,
    root_template: &YamlValue,
    path: &mut Vec<String>,
    report: &mut NormalizeReport,
) -> YamlValue {
    let mut normalized = Mapping::new();
    for (key, template_value) in template_map {
        let Some(key_name) = key.as_str() else {
            normalized.insert(key.clone(), template_value.clone());
            continue;
        };
        path.push(key_name.to_string());
        if let Some(original_value) = original_map.get(key) {
            let value =
                normalize_value(template_value, original_value, root_template, path, report);
            normalized.insert(key.clone(), value);
        } else {
            report.added_paths.push(format_path(path));
            normalized.insert(key.clone(), template_value.clone());
        }
        path.pop();
    }

    for (key, _) in original_map {
        if template_map.contains_key(key) {
            continue;
        }
        if let Some(key_name) = key.as_str() {
            path.push(key_name.to_string());
            report.removed_paths.push(format_path(path));
            path.pop();
        }
    }

    YamlValue::Mapping(normalized)
}

fn validates_at_path(root_template: &YamlValue, path: &[String], value: &YamlValue) -> bool {
    let mut candidate = root_template.clone();
    if set_path(&mut candidate, path, value.clone()).is_err() {
        return false;
    }
    validate_config_value(&candidate).is_ok()
}

fn set_path(root: &mut YamlValue, path: &[String], value: YamlValue) -> TalosResult<()> {
    if path.is_empty() {
        *root = value;
        return Ok(());
    }

    let mut cursor = root;
    for segment in &path[..path.len() - 1] {
        let key = YamlValue::String(segment.clone());
        let YamlValue::Mapping(map) = cursor else {
            return Err(
                TalosError::internal("Config path is not a mapping.").context(format!(
                    "config path {} is not a mapping",
                    format_path(path)
                )),
            );
        };
        cursor = map.get_mut(&key).ok_or_else(|| {
            TalosError::internal("Config path is missing segment; technical details were withheld.")
                .context(format!(
                    "config path {} is missing segment {segment}",
                    format_path(path)
                ))
        })?;
    }

    let key = YamlValue::String(path[path.len() - 1].clone());
    let YamlValue::Mapping(map) = cursor else {
        return Err(
            TalosError::internal("Config path is not a mapping.").context(format!(
                "config path {} is not a mapping",
                format_path(path)
            )),
        );
    };
    map.insert(key, value);
    Ok(())
}

fn validate_config_value(value: &YamlValue) -> TalosResult<()> {
    let raw_value = serde_json::to_value(value).map_err(|err| {
        TalosError::internal_with_source(
            "convert config yaml; technical details were withheld.",
            err,
        )
    })?;
    validate::validate_required_paths(&raw_value)?;
    validate::validate_forbidden_paths(&raw_value)?;
    let config = serde_json::from_value::<Config>(raw_value).map_err(|err| {
        TalosError::internal_with_source(
            "Failed to inspect configuration for backup; technical details were withheld.",
            err,
        )
    })?;
    config.validate_runtime()
}

fn write_backup(config_path: &Path) -> TalosResult<PathBuf> {
    let parent = config_path.parent().ok_or_else(|| {
        TalosError::internal(
            "config path has no parent directory; technical details were withheld.",
        )
        .context(format!(
            "config path has no parent directory: {}",
            config_path.display()
        ))
    })?;
    let stem = config_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("talos.yaml");
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|err| {
            TalosError::internal_with_source(
                "read system time for backup; technical details were withheld.",
                err,
            )
        })?
        .as_secs();
    for index in 0..100 {
        let suffix = if index == 0 {
            format!("{timestamp}")
        } else {
            format!("{timestamp}.{index}")
        };
        let backup_path = parent.join(format!("{stem}.bak.{suffix}"));
        match talos_fs::std_fs().write_file_with(
            &backup_path,
            &std::fs::read_to_string(config_path).map_err(|err| {
                let diagnostic =
                    format!("read config before backup {}: {err}", config_path.display());
                TalosError::internal_with_source(
                    "Read config before backup; technical details were withheld.",
                    err,
                )
                .context(diagnostic)
            })?,
            WriteOptions::create_new(),
        ) {
            Ok(()) => return Ok(backup_path),
            Err(err) if err.is_code(talos_error::ErrorCode::Conflict) => continue,
            Err(err) => {
                return Err(err.context(format!("write config backup {}", backup_path.display())))
            }
        }
    }
    Err(TalosError::internal(
        "could not allocate unique config backup path under; technical details were withheld.",
    )
    .context(format!(
        "could not allocate unique config backup path under {}",
        parent.display()
    )))
}

fn is_dynamic_map_path(path: &[String]) -> bool {
    path == ["storage", "files", "backends"].as_slice()
}

fn format_path(path: &[String]) -> String {
    if path.is_empty() {
        "<root>".to_string()
    } else {
        path.join(".")
    }
}

#[cfg(test)]
#[path = "upgrade_tests.rs"]
mod tests;
