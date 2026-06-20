use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillsRuntimeConfig {
    #[serde(default = "default_max_skill_md_bytes")]
    pub max_skill_md_bytes: usize,

    #[serde(default = "default_max_available_summary_bytes")]
    pub max_available_summary_bytes: usize,
}

impl Default for SkillsRuntimeConfig {
    fn default() -> Self {
        Self {
            max_skill_md_bytes: default_max_skill_md_bytes(),
            max_available_summary_bytes: default_max_available_summary_bytes(),
        }
    }
}

fn default_max_skill_md_bytes() -> usize {
    64 * 1024
}

fn default_max_available_summary_bytes() -> usize {
    8 * 1024
}
