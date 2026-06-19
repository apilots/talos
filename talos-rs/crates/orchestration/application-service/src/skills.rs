use talos_config::{
    resolve_runtime_path, PluginRootKind, PluginsRuntimeConfig, SkillsRuntimeConfig,
};
use talos_error::{TalosError, TalosResult};
use talos_skills::{
    read_document, refresh, resolve, suggest, SkillDocument, SkillRoot, SkillRoots, SkillSelector,
    SkillSnapshot, SkillSuggestion, SkillSummary, SkillWarning,
};

use crate::PluginManagerService;

#[derive(Clone)]
pub struct SkillsService {
    enabled: bool,
    roots: SkillRoots,
    config: SkillsRuntimeConfig,
    plugins: PluginsRuntimeConfig,
    plugin_manager: Option<PluginManagerService>,
}

impl SkillsService {
    pub fn new(config: SkillsRuntimeConfig, plugins: PluginsRuntimeConfig) -> Self {
        Self {
            enabled: config.enabled,
            roots: skill_roots_from_config(&config, &plugins),
            config,
            plugins,
            plugin_manager: None,
        }
    }

    pub fn with_plugin_manager(mut self, plugin_manager: PluginManagerService) -> Self {
        self.plugin_manager = Some(plugin_manager);
        self
    }

    pub fn roots(&self) -> &SkillRoots {
        &self.roots
    }

    pub fn refresh(&self) -> TalosResult<SkillSnapshot> {
        if !self.enabled {
            return Ok(empty_snapshot());
        }
        refresh(&self.current_roots()?)
    }

    pub fn list(&self, include_disabled: bool) -> TalosResult<SkillSnapshot> {
        let mut snapshot = self.refresh()?;
        if !include_disabled {
            snapshot.skills.retain(|skill| skill.enabled);
        }
        Ok(snapshot)
    }

    pub fn suggest(&self, query: &str, limit: usize) -> TalosResult<Vec<SkillSuggestion>> {
        if !self.enabled {
            return Ok(Vec::new());
        }
        suggest(&self.current_roots()?, query, limit)
    }

    pub fn read_document(&self, selector: &SkillSelector) -> TalosResult<SkillDocument> {
        self.ensure_enabled()?;
        read_document(&self.current_roots()?, selector)
    }

    pub fn resolve(
        &self,
        text: &str,
        selected: &[SkillSelector],
    ) -> TalosResult<(Vec<SkillSummary>, Vec<SkillWarning>)> {
        self.ensure_enabled()?;
        resolve(&self.current_roots()?, text, selected)
    }

    fn ensure_enabled(&self) -> TalosResult<()> {
        if self.enabled {
            Ok(())
        } else {
            Err(TalosError::ValidationError(
                "skills are disabled by configuration".to_string(),
            ))
        }
    }

    fn current_roots(&self) -> TalosResult<SkillRoots> {
        let mut roots = skill_roots_from_config(&self.config, &self.plugins);
        if let Some(plugin_manager) = &self.plugin_manager {
            roots
                .roots
                .extend(plugin_manager.manager().installed_skill_roots()?);
        }
        Ok(roots)
    }
}

fn skill_roots_from_config(
    config: &SkillsRuntimeConfig,
    plugins: &PluginsRuntimeConfig,
) -> SkillRoots {
    let roots = plugins
        .roots
        .iter()
        .filter(|root| root.kind == PluginRootKind::Skill && root.read_only)
        .map(|root| {
            let path = resolve_runtime_path(&root.path);
            SkillRoot::configured(path)
        })
        .collect::<Vec<_>>();
    SkillRoots {
        roots,
        state_path: resolve_runtime_path("${TALOS_HOME}/plugins/skills/skills-state.json"),
        max_skill_md_bytes: config.max_skill_md_bytes,
        max_available_summary_bytes: config.max_available_summary_bytes,
    }
}

fn empty_snapshot() -> SkillSnapshot {
    SkillSnapshot {
        skills: Vec::new(),
        errors: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn skills_service_lists_suggests_reads_and_resolves_from_configured_roots() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let external_root = temp.path().join("external-skills");
        let skill_root = external_root.join("review");
        fs::create_dir_all(&skill_root).expect("skill root should be created");
        fs::write(
            skill_root.join("SKILL.md"),
            "---\nname: review\ndescription: Review code changes.\n---\n\n# Review\n\nUse evidence.",
        )
        .expect("skill should be written");

        let service = SkillsService::new(
            SkillsRuntimeConfig {
                enabled: true,
                max_skill_md_bytes: 64 * 1024,
                max_available_summary_bytes: 8 * 1024,
            },
            PluginsRuntimeConfig {
                sources: Vec::new(),
                roots: vec![talos_config::PluginRootConfig {
                    kind: talos_config::PluginRootKind::Skill,
                    path: external_root.display().to_string(),
                    read_only: true,
                }],
            },
        );

        let listed = service.list(false).expect("skills should list");
        assert_eq!(listed.skills.len(), 1);
        assert_eq!(listed.skills[0].name, "review");

        let suggestions = service
            .suggest("rev", 10)
            .expect("skills should suggest by query");
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].path, skill_root.join("SKILL.md"));

        let document = service
            .read_document(&SkillSelector::by_path(skill_root.join("SKILL.md")))
            .expect("skill document should read");
        assert!(document.contents.contains("Use evidence."));

        let (resolved, warnings) = service
            .resolve(
                "Use $review",
                &[SkillSelector::selection(
                    "review",
                    skill_root.join("SKILL.md"),
                )],
            )
            .expect("skill should resolve");
        assert!(warnings.is_empty());
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].name, "review");
    }

    #[test]
    fn skills_disabled_returns_empty_catalog_and_rejects_mutations() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let external_root = temp.path().join("external-skills");
        let skill_root = external_root.join("review");
        fs::create_dir_all(&skill_root).expect("skill root should be created");
        fs::write(
            skill_root.join("SKILL.md"),
            "---\nname: review\ndescription: Review code changes.\n---\n",
        )
        .expect("skill should be written");

        let service = SkillsService::new(
            SkillsRuntimeConfig {
                enabled: false,
                max_skill_md_bytes: 64 * 1024,
                max_available_summary_bytes: 8 * 1024,
            },
            PluginsRuntimeConfig {
                sources: Vec::new(),
                roots: vec![talos_config::PluginRootConfig {
                    kind: talos_config::PluginRootKind::Skill,
                    path: external_root.display().to_string(),
                    read_only: true,
                }],
            },
        );

        assert!(service
            .list(false)
            .expect("disabled skills list")
            .skills
            .is_empty());
        assert!(service
            .suggest("review", 10)
            .expect("disabled skills suggest")
            .is_empty());
        let error = service
            .read_document(&SkillSelector::by_path(skill_root.join("SKILL.md")))
            .expect_err("disabled skills should reject reads");
        assert!(matches!(error, TalosError::ValidationError(_)));
    }
}
