use crate::model::{SkillLoadError, SkillMetadata, SkillRoot, SkillSnapshot, SkillSummary};
use crate::state::{plugin_skill_state_key, SkillState};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
    #[serde(default)]
    short_description: Option<String>,
    #[serde(default)]
    metadata: Option<serde_yaml::Value>,
    #[serde(flatten)]
    unknown: serde_yaml::Mapping,
}

#[derive(Debug, Default)]
struct HostMetadata {
    display_name: Option<String>,
    short_description: Option<String>,
    icon: Option<String>,
}

pub fn load_skills_from_roots(
    roots: &[SkillRoot],
    state: &SkillState,
    max_skill_md_bytes: usize,
) -> SkillSnapshot {
    let mut skills = Vec::new();
    let mut errors = Vec::new();
    for root in roots {
        if !root.path.exists() {
            continue;
        }
        let entries = match fs::read_dir(&root.path) {
            Ok(entries) => entries,
            Err(err) => {
                errors.push(SkillLoadError {
                    root: root.path.clone(),
                    path: None,
                    message: format!("failed to read skill root: {err}"),
                });
                continue;
            }
        };
        for entry in entries.flatten() {
            let skill_root = entry.path();
            if !skill_root.is_dir() {
                continue;
            }
            match parse_skill_dir(&skill_root, root, state, max_skill_md_bytes) {
                Ok(skill) => skills.push(skill),
                Err(error) => errors.push(error),
            }
        }
    }
    sort_skills(&mut skills);
    SkillSnapshot { skills, errors }
}

pub fn parse_skill_dir(
    skill_root: &Path,
    source: &SkillRoot,
    state: &SkillState,
    max_skill_md_bytes: usize,
) -> Result<SkillSummary, SkillLoadError> {
    let skill_path = skill_root.join("SKILL.md");
    let root = skill_root.to_path_buf();
    if !skill_path.exists() {
        return Err(SkillLoadError {
            root,
            path: Some(skill_path),
            message: "missing SKILL.md".to_string(),
        });
    }
    let metadata = fs::metadata(&skill_path).map_err(|err| SkillLoadError {
        root: root.clone(),
        path: Some(skill_path.clone()),
        message: format!("failed to read SKILL.md metadata: {err}"),
    })?;
    if metadata.len() as usize > max_skill_md_bytes {
        return Err(SkillLoadError {
            root,
            path: Some(skill_path),
            message: format!("SKILL.md exceeds {max_skill_md_bytes} bytes"),
        });
    }
    let contents = fs::read_to_string(&skill_path).map_err(|err| SkillLoadError {
        root: root.clone(),
        path: Some(skill_path.clone()),
        message: format!("failed to read SKILL.md: {err}"),
    })?;
    let frontmatter = parse_frontmatter(&contents).map_err(|message| SkillLoadError {
        root: root.clone(),
        path: Some(skill_path.clone()),
        message,
    })?;
    let name = required_field(frontmatter.name, "name").map_err(|message| SkillLoadError {
        root: root.clone(),
        path: Some(skill_path.clone()),
        message,
    })?;
    let description =
        required_field(frontmatter.description, "description").map_err(|message| {
            SkillLoadError {
                root: root.clone(),
                path: Some(skill_path.clone()),
                message,
            }
        })?;
    let metadata = parse_extra_metadata(frontmatter.metadata, &frontmatter.unknown);
    let host_metadata = parse_host_metadata(skill_root);
    let mut metadata = metadata.unwrap_or(SkillMetadata {
        display_name: None,
        icon: None,
        author: None,
        version: None,
        argument_hint: None,
    });
    if metadata.display_name.is_none() {
        metadata.display_name = host_metadata.display_name;
    }
    if metadata.icon.is_none() {
        metadata.icon = host_metadata.icon;
    }
    let metadata = (!metadata.is_empty()).then_some(metadata);
    Ok(SkillSummary {
        name,
        description,
        short_description: frontmatter
            .short_description
            .or(host_metadata.short_description),
        root: root.clone(),
        path: skill_path.clone(),
        scope: source.scope,
        source_plugin: source.source_plugin.clone(),
        source_install: source.source_install.clone(),
        enabled: !state.is_disabled(
            plugin_skill_state_key(source.source_install.as_deref(), &root, &skill_path).as_ref(),
        ),
        read_only: source.read_only,
        metadata,
    })
}

pub fn read_skill_contents(path: &Path, max_skill_md_bytes: usize) -> Result<String, String> {
    let metadata =
        fs::metadata(path).map_err(|err| format!("failed to read SKILL.md metadata: {err}"))?;
    if metadata.len() as usize > max_skill_md_bytes {
        return Err(format!("SKILL.md exceeds {max_skill_md_bytes} bytes"));
    }
    fs::read_to_string(path).map_err(|err| format!("failed to read SKILL.md: {err}"))
}

fn parse_frontmatter(contents: &str) -> Result<SkillFrontmatter, String> {
    let mut lines = contents.lines();
    if lines.next() != Some("---") {
        return Err("missing YAML frontmatter".to_string());
    }
    let mut yaml = String::new();
    for line in lines {
        if line == "---" {
            return serde_yaml::from_str::<SkillFrontmatter>(&yaml)
                .map_err(|err| format!("invalid YAML frontmatter: {err}"));
        }
        yaml.push_str(line);
        yaml.push('\n');
    }
    Err("unterminated YAML frontmatter".to_string())
}

fn required_field(value: Option<String>, name: &str) -> Result<String, String> {
    let value = value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    value.ok_or_else(|| format!("missing required frontmatter field `{name}`"))
}

fn parse_extra_metadata(
    metadata_value: Option<serde_yaml::Value>,
    unknown: &serde_yaml::Mapping,
) -> Option<SkillMetadata> {
    let mut metadata = SkillMetadata {
        display_name: None,
        icon: None,
        author: None,
        version: None,
        argument_hint: None,
    };
    if let Some(serde_yaml::Value::Mapping(map)) = metadata_value {
        metadata.author = string_field(&map, "author");
        metadata.version = string_field(&map, "version");
        metadata.argument_hint =
            string_field(&map, "argument-hint").or_else(|| string_field(&map, "argument_hint"));
        metadata.display_name =
            string_field(&map, "display-name").or_else(|| string_field(&map, "display_name"));
        metadata.icon = string_field(&map, "icon");
    }
    if metadata.argument_hint.is_none() {
        if let Some(value) = unknown.get(serde_yaml::Value::String("argument-hint".to_string())) {
            metadata.argument_hint = yaml_string(value);
        }
    }
    if metadata.is_empty() {
        None
    } else {
        Some(metadata)
    }
}

fn parse_host_metadata(skill_root: &Path) -> HostMetadata {
    let path = skill_root.join("agents").join("talos.yaml");
    let Ok(contents) = fs::read_to_string(path) else {
        return HostMetadata::default();
    };
    let Ok(serde_yaml::Value::Mapping(map)) = serde_yaml::from_str::<serde_yaml::Value>(&contents)
    else {
        return HostMetadata::default();
    };
    HostMetadata {
        display_name: string_field(&map, "display_name")
            .or_else(|| string_field(&map, "display-name"))
            .or_else(|| string_field(&map, "name")),
        short_description: string_field(&map, "short_description")
            .or_else(|| string_field(&map, "short-description")),
        icon: string_field(&map, "icon"),
    }
}

fn string_field(map: &serde_yaml::Mapping, key: &str) -> Option<String> {
    map.get(serde_yaml::Value::String(key.to_string()))
        .and_then(yaml_string)
}

fn yaml_string(value: &serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::String(value) => {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        serde_yaml::Value::Number(value) => Some(value.to_string()),
        serde_yaml::Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn sort_skills(skills: &mut [SkillSummary]) {
    skills.sort_by(|left, right| {
        left.scope
            .priority()
            .cmp(&right.scope.priority())
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.path.cmp(&right.path))
    });
}

pub(crate) fn same_path(left: &Path, right: &Path) -> bool {
    normalized_path(left) == normalized_path(right)
}

fn normalized_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
