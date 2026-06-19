use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillsRuntimeConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_roots")]
    pub roots: Vec<String>,

    #[serde(default = "default_install_root")]
    pub install_root: String,

    #[serde(default = "default_state_path")]
    pub state_path: String,

    #[serde(default = "default_max_skill_md_bytes")]
    pub max_skill_md_bytes: usize,

    #[serde(default = "default_max_available_summary_bytes")]
    pub max_available_summary_bytes: usize,
}

impl Default for SkillsRuntimeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            roots: default_roots(),
            install_root: default_install_root(),
            state_path: default_state_path(),
            max_skill_md_bytes: default_max_skill_md_bytes(),
            max_available_summary_bytes: default_max_available_summary_bytes(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_roots() -> Vec<String> {
    vec![
        "~/.talos/skills".to_string(),
        "~/.agents/skills".to_string(),
    ]
}

fn default_install_root() -> String {
    "~/.talos/skills".to_string()
}

fn default_state_path() -> String {
    "~/.talos/skills-state.json".to_string()
}

fn default_max_skill_md_bytes() -> usize {
    64 * 1024
}

fn default_max_available_summary_bytes() -> usize {
    8 * 1024
}
