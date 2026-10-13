use crate::{PluginManager, PluginStateStore};
use talos_config::{PluginRootConfig as ConfigRoot, PluginSourceConfig, PluginsRuntimeConfig};
use talos_types::{MarketplaceSourceInput, PluginSourceAddRequest, PluginSourceAddResponse};

pub(crate) struct LocalSkillSourceFixture {
    pub(crate) repo: std::path::PathBuf,
    pub(crate) skills: std::path::PathBuf,
    pub(crate) state: PluginStateStore,
    pub(crate) manager: PluginManager,
}

impl LocalSkillSourceFixture {
    pub(crate) fn new(temp: &tempfile::TempDir) -> Self {
        let repo = temp.path().join("repo");
        let skills = repo.join("skills");
        let state = PluginStateStore::new(temp.path().join("state"));
        let manager = manager_with_state(state.clone());
        Self {
            repo,
            skills,
            state,
            manager,
        }
    }

    pub(crate) fn write_skill(&self, name: &str, contents: &str) {
        write_skill(&self.skills, name, contents);
    }

    pub(crate) fn add_source(&self, refresh: bool) -> PluginSourceAddResponse {
        self.manager
            .add_source(PluginSourceAddRequest {
                node_id: None,
                source: MarketplaceSourceInput::LocalFolder {
                    path: self.repo.clone(),
                    sparse_paths: vec!["skills".to_string()],
                },
                refresh,
            })
            .expect("local skill source should add")
    }
}

pub(crate) fn empty_plugins_config() -> PluginsRuntimeConfig {
    PluginsRuntimeConfig {
        sources: Vec::<PluginSourceConfig>::new(),
        roots: Vec::<ConfigRoot>::new(),
    }
}

pub(crate) fn manager_with_state(state: PluginStateStore) -> PluginManager {
    PluginManager::with_state_store(empty_plugins_config(), state)
}

pub(crate) fn manager_with_state_path(path: impl Into<std::path::PathBuf>) -> PluginManager {
    manager_with_state(PluginStateStore::new(path.into()))
}

pub(crate) fn write_skill(root: &std::path::Path, name: &str, contents: &str) {
    let skill_dir = root.join(name);
    std::fs::create_dir_all(&skill_dir).expect("skill dir should be created");
    std::fs::write(skill_dir.join("SKILL.md"), contents).expect("skill file should be written");
}
