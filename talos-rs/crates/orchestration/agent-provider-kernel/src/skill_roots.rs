use std::sync::Arc;

use talos_config::{resolve_talos_home, AgentKernelConfig};
use talos_error::TalosResult;
use talos_skills::SkillRoots;

/// Resolves Kernel skill roots without making the Provider read global config.
pub trait SkillRootsResolver: Send + Sync {
    /// Resolve bounded roots for the supplied effective Kernel configuration.
    fn skill_roots(&self, kernel_config: &AgentKernelConfig) -> TalosResult<SkillRoots>;
}

/// Resolver used when no external skill installation source is configured.
#[derive(Debug, Default)]
pub struct EmptySkillRootsResolver;

impl SkillRootsResolver for EmptySkillRootsResolver {
    fn skill_roots(&self, kernel_config: &AgentKernelConfig) -> TalosResult<SkillRoots> {
        Ok(SkillRoots {
            roots: Vec::new(),
            state_path: resolve_talos_home()
                .join("plugins")
                .join("skills")
                .join("skills-state.json"),
            max_skill_md_bytes: kernel_config.skills.max_skill_md_bytes,
            max_available_summary_bytes: kernel_config.skills.max_available_summary_bytes,
        })
    }
}

/// Create the default Kernel skill-roots resolver.
pub fn empty_skill_roots_resolver() -> Arc<dyn SkillRootsResolver> {
    Arc::new(EmptySkillRootsResolver)
}
