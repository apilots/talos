use crate::catalog::{refresh, resolve};
use crate::loader::read_skill_contents;
use crate::model::{SkillSelector, SkillWarning, SkillWarningCode};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use talos_error::{TalosError, TalosResult};

pub trait SkillProvider: Send + Sync {
    fn prompt_context(&self, request: SkillPromptRequest) -> TalosResult<SkillPromptContext>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillPromptRequest {
    pub text: String,
    #[serde(default)]
    pub selected: Vec<SkillSelector>,
    pub budget: SkillPromptBudget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillPromptBudget {
    pub max_available_summary_bytes: usize,
    pub max_skill_md_bytes: usize,
}

impl Default for SkillPromptBudget {
    fn default() -> Self {
        Self {
            max_available_summary_bytes: 8 * 1024,
            max_skill_md_bytes: 64 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillPromptContext {
    pub available_summary: String,
    pub injections: Vec<SkillPromptInjection>,
    pub warnings: Vec<SkillPromptWarning>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillPromptInjection {
    pub name: String,
    pub root: PathBuf,
    pub path: PathBuf,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillPromptWarning {
    pub code: SkillWarningCode,
    pub message: String,
}

impl From<SkillWarning> for SkillPromptWarning {
    fn from(warning: SkillWarning) -> Self {
        Self {
            code: warning.code,
            message: warning.message,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LocalSkillProvider {
    roots: crate::model::SkillRoots,
}

impl LocalSkillProvider {
    pub fn new(roots: crate::model::SkillRoots) -> Self {
        Self { roots }
    }

    pub fn roots(&self) -> &crate::model::SkillRoots {
        &self.roots
    }
}

impl SkillProvider for LocalSkillProvider {
    fn prompt_context(&self, request: SkillPromptRequest) -> TalosResult<SkillPromptContext> {
        let mut roots = self.roots.clone();
        roots.max_available_summary_bytes = request.budget.max_available_summary_bytes;
        roots.max_skill_md_bytes = request.budget.max_skill_md_bytes;
        let snapshot = refresh(&roots)?;
        let available_summary = render_available_summary(
            snapshot.skills.iter().filter(|skill| skill.enabled),
            roots.max_available_summary_bytes,
        );
        let (selected, warnings) = resolve(&roots, &request.text, &request.selected)?;
        let mut prompt_warnings = warnings
            .into_iter()
            .map(SkillPromptWarning::from)
            .collect::<Vec<_>>();
        let mut injections = Vec::new();
        for skill in selected {
            match read_skill_contents(&skill.path, roots.max_skill_md_bytes) {
                Ok(contents) => injections.push(SkillPromptInjection {
                    name: skill.name,
                    root: skill.root,
                    path: skill.path,
                    contents,
                }),
                Err(err) => prompt_warnings.push(SkillPromptWarning {
                    code: SkillWarningCode::SkillInjectionFailed,
                    message: format!(
                        "failed to read skill `{}` at `{}`: {err}",
                        skill.name,
                        skill.path.display()
                    ),
                }),
            }
        }
        Ok(SkillPromptContext {
            available_summary,
            injections,
            warnings: prompt_warnings,
        })
    }
}

fn render_available_summary<'a>(
    skills: impl Iterator<Item = &'a crate::model::SkillSummary>,
    max_bytes: usize,
) -> String {
    let mut output = String::from("Available skills:\n");
    for skill in skills {
        let source = skill
            .source_plugin
            .as_deref()
            .map(|plugin| format!("plugin:{plugin}"))
            .unwrap_or_else(|| skill.scope.source_hint().to_string());
        let line = format!("- {} [{}]: {}\n", skill.name, source, skill.description);
        if output.len() + line.len() > max_bytes {
            output.push_str("- ...\n");
            break;
        }
        output.push_str(&line);
    }
    output
}

pub fn ensure_prompt_context(context: SkillPromptContext) -> TalosResult<SkillPromptContext> {
    if context.available_summary.len() > 1024 * 1024 {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "skill prompt context summary is unexpectedly large".to_string(),
        ));
    }
    Ok(context)
}
