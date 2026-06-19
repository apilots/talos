use talos_config::{resolve_runtime_path, SkillsRuntimeConfig};
use talos_error::{TalosError, TalosResult};
use talos_skills::{
    disable_skill, enable_skill, install_skill, read_document, refresh, resolve, suggest,
    uninstall_skill, SkillAdminContext, SkillDocument, SkillInstallSource, SkillRoot, SkillRoots,
    SkillSelector, SkillSnapshot, SkillSuggestion, SkillSummary, SkillWarning,
};

#[derive(Clone)]
pub struct SkillsService {
    enabled: bool,
    roots: SkillRoots,
}

impl SkillsService {
    pub fn new(config: SkillsRuntimeConfig) -> Self {
        Self {
            enabled: config.enabled,
            roots: skill_roots_from_config(&config),
        }
    }

    pub fn roots(&self) -> &SkillRoots {
        &self.roots
    }

    pub fn refresh(&self) -> TalosResult<SkillSnapshot> {
        if !self.enabled {
            return Ok(empty_snapshot());
        }
        refresh(&self.roots)
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
        suggest(&self.roots, query, limit)
    }

    pub fn read_document(&self, selector: &SkillSelector) -> TalosResult<SkillDocument> {
        self.ensure_enabled()?;
        read_document(&self.roots, selector)
    }

    pub fn resolve(
        &self,
        text: &str,
        selected: &[SkillSelector],
    ) -> TalosResult<(Vec<SkillSummary>, Vec<SkillWarning>)> {
        self.ensure_enabled()?;
        resolve(&self.roots, text, selected)
    }

    pub fn install(&self, source_path: impl Into<std::path::PathBuf>) -> TalosResult<SkillSummary> {
        self.ensure_enabled()?;
        let ctx = SkillAdminContext::new(self.roots.clone());
        install_skill(&ctx, SkillInstallSource::LocalPath(source_path.into()))
    }

    pub fn uninstall(&self, selector: &SkillSelector) -> TalosResult<()> {
        self.ensure_enabled()?;
        let ctx = SkillAdminContext::new(self.roots.clone());
        uninstall_skill(&ctx, selector)
    }

    pub fn enable(&self, selector: &SkillSelector) -> TalosResult<SkillSummary> {
        self.ensure_enabled()?;
        let ctx = SkillAdminContext::new(self.roots.clone());
        enable_skill(&ctx, selector)
    }

    pub fn disable(&self, selector: &SkillSelector) -> TalosResult<SkillSummary> {
        self.ensure_enabled()?;
        let ctx = SkillAdminContext::new(self.roots.clone());
        disable_skill(&ctx, selector)
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
}

fn skill_roots_from_config(config: &SkillsRuntimeConfig) -> SkillRoots {
    let install_root = resolve_runtime_path(&config.install_root);
    let roots = config
        .roots
        .iter()
        .map(|path| {
            let path = resolve_runtime_path(path);
            if path == install_root {
                SkillRoot::user(path)
            } else {
                SkillRoot::external(path)
            }
        })
        .collect::<Vec<_>>();
    SkillRoots {
        roots,
        state_path: resolve_runtime_path(&config.state_path),
        install_root,
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
        let user_root = temp.path().join("user-skills");
        let state_path = temp.path().join("skills-state.json");
        let skill_root = user_root.join("review");
        fs::create_dir_all(&skill_root).expect("skill root should be created");
        fs::write(
            skill_root.join("SKILL.md"),
            "---\nname: review\ndescription: Review code changes.\n---\n\n# Review\n\nUse evidence.",
        )
        .expect("skill should be written");

        let service = SkillsService::new(SkillsRuntimeConfig {
            enabled: true,
            roots: vec![user_root.display().to_string()],
            install_root: user_root.display().to_string(),
            state_path: state_path.display().to_string(),
            max_skill_md_bytes: 64 * 1024,
            max_available_summary_bytes: 8 * 1024,
        });

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
    fn skills_service_filters_disabled_skills_from_default_list() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let user_root = temp.path().join("user-skills");
        let state_path = temp.path().join("skills-state.json");
        let skill_root = user_root.join("review");
        fs::create_dir_all(&skill_root).expect("skill root should be created");
        fs::write(
            skill_root.join("SKILL.md"),
            "---\nname: review\ndescription: Review code changes.\n---\n",
        )
        .expect("skill should be written");

        let service = SkillsService::new(SkillsRuntimeConfig {
            enabled: true,
            roots: vec![user_root.display().to_string()],
            install_root: user_root.display().to_string(),
            state_path: state_path.display().to_string(),
            max_skill_md_bytes: 64 * 1024,
            max_available_summary_bytes: 8 * 1024,
        });

        service
            .disable(&SkillSelector::by_path(skill_root.join("SKILL.md")))
            .expect("skill should disable");

        assert!(service
            .list(false)
            .expect("enabled skills should list")
            .skills
            .is_empty());
        assert_eq!(
            service
                .list(true)
                .expect("all skills should list")
                .skills
                .len(),
            1
        );
    }

    #[test]
    fn skills_disabled_returns_empty_catalog_and_rejects_mutations() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let user_root = temp.path().join("user-skills");
        let state_path = temp.path().join("skills-state.json");
        let skill_root = user_root.join("review");
        fs::create_dir_all(&skill_root).expect("skill root should be created");
        fs::write(
            skill_root.join("SKILL.md"),
            "---\nname: review\ndescription: Review code changes.\n---\n",
        )
        .expect("skill should be written");

        let service = SkillsService::new(SkillsRuntimeConfig {
            enabled: false,
            roots: vec![user_root.display().to_string()],
            install_root: user_root.display().to_string(),
            state_path: state_path.display().to_string(),
            max_skill_md_bytes: 64 * 1024,
            max_available_summary_bytes: 8 * 1024,
        });

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
