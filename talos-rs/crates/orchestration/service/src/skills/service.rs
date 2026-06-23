use talos_config::{PluginsRuntimeConfig, SkillsRuntimeConfig};
use talos_error::TalosResult;
use talos_skills::{
    read_document, refresh, resolve, suggest, SkillDocument, SkillRoots, SkillSelector,
    SkillSnapshot, SkillSuggestion, SkillSummary, SkillWarning,
};
use talos_types::{
    SkillsListRequest, SkillsListResponse, SkillsReadRequest, SkillsRefreshRequest,
    SkillsResolveRequest, SkillsResolveResponse, SkillsSuggestRequest, SkillsSuggestResponse,
};

use crate::PluginManagerService;

#[derive(Clone)]
pub struct SkillsService {
    config: SkillsRuntimeConfig,
    plugins: PluginManagerService,
}

impl SkillsService {
    pub fn new(config: SkillsRuntimeConfig, plugins: PluginsRuntimeConfig) -> Self {
        Self {
            plugins: PluginManagerService::new(plugins, config.clone()),
            config,
        }
    }

    pub fn with_plugin_manager(mut self, plugin_manager: PluginManagerService) -> Self {
        self.plugins = plugin_manager;
        self
    }

    pub fn refresh(&self) -> TalosResult<SkillSnapshot> {
        refresh(&self.current_roots()?)
    }

    pub fn list(&self, include_disabled: bool) -> TalosResult<SkillSnapshot> {
        let mut snapshot = self.refresh()?;
        if !include_disabled {
            snapshot.skills.retain(|skill| skill.enabled);
        }
        Ok(snapshot)
    }

    pub fn list_with_request(&self, request: SkillsListRequest) -> TalosResult<SkillsListResponse> {
        self.snapshot_response(self.list(request.include_disabled)?)
    }

    pub fn refresh_with_request(
        &self,
        _request: SkillsRefreshRequest,
    ) -> TalosResult<SkillsListResponse> {
        self.snapshot_response(self.refresh()?)
    }

    pub fn suggest(&self, query: &str, limit: usize) -> TalosResult<Vec<SkillSuggestion>> {
        suggest(&self.current_roots()?, query, limit)
    }

    pub fn suggest_with_request(
        &self,
        request: SkillsSuggestRequest,
    ) -> TalosResult<SkillsSuggestResponse> {
        Ok(SkillsSuggestResponse {
            data: self.suggest(&request.query, request.limit.unwrap_or(20))?,
        })
    }

    pub fn read_document(&self, selector: &SkillSelector) -> TalosResult<SkillDocument> {
        read_document(&self.current_roots()?, selector)
    }

    pub fn read_with_request(&self, request: SkillsReadRequest) -> TalosResult<SkillDocument> {
        request
            .selector
            .validate()
            .map_err(|error| talos_error::TalosError::ValidationError(error.to_string()))?;
        self.read_document(&request.selector)
    }

    pub fn resolve(
        &self,
        text: &str,
        selected: &[SkillSelector],
    ) -> TalosResult<(Vec<SkillSummary>, Vec<SkillWarning>)> {
        resolve(&self.current_roots()?, text, selected)
    }

    pub fn resolve_with_request(
        &self,
        request: SkillsResolveRequest,
    ) -> TalosResult<SkillsResolveResponse> {
        let (skills, warnings) = self.resolve(&request.text, &request.selected)?;
        Ok(SkillsResolveResponse {
            data: skills,
            warnings,
        })
    }

    fn snapshot_response(&self, snapshot: SkillSnapshot) -> TalosResult<SkillsListResponse> {
        Ok(SkillsListResponse {
            data: snapshot.skills,
            errors: snapshot.errors,
        })
    }

    fn current_roots(&self) -> TalosResult<SkillRoots> {
        self.plugins.skill_roots(&self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use talos_plugin::{PluginManager, PluginStateStore};

    fn test_service(
        temp: &tempfile::TempDir,
        config: SkillsRuntimeConfig,
        plugins: PluginsRuntimeConfig,
    ) -> SkillsService {
        let manager = PluginManager::with_state_store(
            plugins.clone(),
            PluginStateStore::new(temp.path().join("plugin-state")),
        );
        SkillsService::new(config, plugins)
            .with_plugin_manager(PluginManagerService::with_manager(manager))
    }

    #[test]
    fn skills_service_lists_suggests_reads_and_resolves_from_configured_roots() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let external_root = temp.path().join("external-skills");
        let skill_root = external_root.join("review");
        talos_fs::create_directory(&skill_root, true).expect("skill root should be created");
        talos_fs::write_bytes(
            &skill_root.join("SKILL.md"),
            b"---\nname: review\ndescription: Review code changes.\n---\n\n# Review\n\nUse evidence.",
        )
        .expect("skill should be written");

        let plugins = PluginsRuntimeConfig {
            sources: Vec::new(),
            roots: vec![talos_config::PluginRootConfig {
                kind: talos_config::PluginRootKind::Skill,
                path: external_root.display().to_string(),
                read_only: true,
            }],
        };
        let service = test_service(
            &temp,
            SkillsRuntimeConfig {
                max_skill_md_bytes: 64 * 1024,
                max_available_summary_bytes: 8 * 1024,
            },
            plugins,
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
}
