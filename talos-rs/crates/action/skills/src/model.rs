use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SkillScope {
    Project,
    Plugin,
    External,
    System,
}

impl SkillScope {
    pub fn priority(self) -> u8 {
        match self {
            Self::Project => 0,
            Self::Plugin => 1,
            Self::External => 2,
            Self::System => 3,
        }
    }

    pub fn source_hint(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Plugin => "plugin",
            Self::External => "external",
            Self::System => "system",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillRoot {
    pub path: PathBuf,
    pub scope: SkillScope,
    #[serde(default)]
    pub source_plugin: Option<String>,
    #[serde(default)]
    pub source_install: Option<String>,
    #[serde(default)]
    pub read_only: bool,
}

impl SkillRoot {
    pub fn configured(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            scope: SkillScope::External,
            source_plugin: None,
            source_install: None,
            read_only: true,
        }
    }

    pub fn project(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            scope: SkillScope::Project,
            source_plugin: None,
            source_install: None,
            read_only: true,
        }
    }

    pub fn plugin(
        path: impl Into<PathBuf>,
        plugin_id: impl Into<String>,
        install_id: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            scope: SkillScope::Plugin,
            source_plugin: Some(plugin_id.into()),
            source_install: Some(install_id.into()),
            read_only: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillRoots {
    pub roots: Vec<SkillRoot>,
    pub state_path: PathBuf,
    pub max_skill_md_bytes: usize,
    pub max_available_summary_bytes: usize,
}

impl SkillRoots {
    pub fn from_home(home: impl Into<PathBuf>) -> Self {
        let home = home.into();
        Self {
            roots: vec![SkillRoot::configured(home.join(".agents").join("skills"))],
            state_path: home
                .join(".talos")
                .join("plugins")
                .join("skills")
                .join("skills-state.json"),
            max_skill_md_bytes: 64 * 1024,
            max_available_summary_bytes: 8 * 1024,
        }
    }

    pub fn from_env_home() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        Self::from_home(home)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSource {
    pub root: PathBuf,
    pub scope: SkillScope,
    #[serde(default)]
    pub source_plugin: Option<String>,
    #[serde(default)]
    pub source_install: Option<String>,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillMetadata {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub argument_hint: Option<String>,
}

impl SkillMetadata {
    pub fn is_empty(&self) -> bool {
        self.display_name.is_none()
            && self.icon.is_none()
            && self.author.is_none()
            && self.version.is_none()
            && self.argument_hint.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillSummary {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub short_description: Option<String>,
    pub root: PathBuf,
    pub path: PathBuf,
    pub scope: SkillScope,
    #[serde(default)]
    pub source_plugin: Option<String>,
    #[serde(default)]
    pub source_install: Option<String>,
    pub enabled: bool,
    pub read_only: bool,
    #[serde(default)]
    pub metadata: Option<SkillMetadata>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillSuggestion {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub short_description: Option<String>,
    pub root: PathBuf,
    pub path: PathBuf,
    pub scope: SkillScope,
    #[serde(default)]
    pub source_plugin: Option<String>,
    #[serde(default)]
    pub source_install: Option<String>,
}

impl From<&SkillSummary> for SkillSuggestion {
    fn from(skill: &SkillSummary) -> Self {
        Self {
            name: skill.name.clone(),
            description: skill.description.clone(),
            short_description: skill.short_description.clone(),
            root: skill.root.clone(),
            path: skill.path.clone(),
            scope: skill.scope,
            source_plugin: skill.source_plugin.clone(),
            source_install: skill.source_install.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkillSelector {
    Selection { name: String, path: PathBuf },
    ByPath { path: PathBuf },
    ByName { name: String },
}

impl SkillSelector {
    pub fn selection(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self::Selection {
            name: name.into(),
            path: path.into(),
        }
    }

    pub fn by_path(path: impl Into<PathBuf>) -> Self {
        Self::ByPath { path: path.into() }
    }

    pub fn by_name(name: impl Into<String>) -> Self {
        Self::ByName { name: name.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillDocument {
    pub summary: SkillSummary,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillLoadError {
    pub root: PathBuf,
    #[serde(default)]
    pub path: Option<PathBuf>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillSnapshot {
    pub skills: Vec<SkillSummary>,
    pub errors: Vec<SkillLoadError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillWarning {
    pub code: String,
    pub message: String,
}
