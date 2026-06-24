use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use talos_agent_kernel::TurnPhase;
use talos_config::{resolve_talos_home, AgentKernelConfig};
use talos_error::TalosResult;
use talos_skills::SkillRoots;
use talos_types::agent_dispatch::AgentDispatchResumeContext;
use talos_types::talos::{
    StopReason, TalosInputItem, TalosOutputItem, TalosReasoning, TalosTimelineEvent, TalosUsage,
};
use talos_types::TalosSession;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequest {
    pub session: TalosSession,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub resume_context: Option<AgentDispatchResumeContext>,
    pub input: Vec<TalosInputItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTitleUpdate {
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    pub output_text: String,
    #[serde(default)]
    pub output_items: Vec<TalosOutputItem>,
    #[serde(default)]
    pub reasoning: Vec<TalosReasoning>,
    pub stop_reason: StopReason,
    #[serde(default)]
    pub usage: TalosUsage,
    pub task_count: u32,
    pub session_id: String,
    #[serde(default)]
    pub session_title_update: Option<SessionTitleUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentKernelRequest {
    pub session: TalosSession,
    pub input: Vec<TalosInputItem>,
    #[serde(default)]
    pub phase: TurnPhase,
    pub planning_instruction: Option<String>,
    pub transfer_to: Option<String>,
}

pub type AgentEvent = TalosTimelineEvent;

pub trait SkillRootsResolver: Send + Sync {
    fn skill_roots(&self, kernel_config: &AgentKernelConfig) -> TalosResult<SkillRoots>;
}

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

pub fn empty_skill_roots_resolver() -> Arc<dyn SkillRootsResolver> {
    Arc::new(EmptySkillRootsResolver)
}

#[async_trait]
pub trait Agent: Send + Sync {
    fn id(&self) -> &str;
    async fn run(&self, request: AgentRequest) -> TalosResult<AgentResponse>;
    async fn run_stream(&self, request: AgentRequest) -> TalosResult<mpsc::Receiver<AgentEvent>>;
    fn take_session_title_update(
        &self,
        _session_id: &str,
        _turn_id: &str,
    ) -> Option<SessionTitleUpdate> {
        None
    }
    async fn compact_session(&self, session: TalosSession) -> TalosResult<TalosUsage>;
    async fn clear_session(&self, session: TalosSession) -> TalosResult<()>;
    async fn close_session(&self, session: TalosSession) -> TalosResult<()>;
    async fn drain_memory_flushes(&self, timeout: Duration) -> TalosResult<()>;
    fn pending_memory_flushes(&self) -> usize;
    async fn has_pending_memory_flush(&self, session_id: &str) -> bool;
    fn has_active_turn(&self, session_id: &str) -> bool;
    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()>;
    async fn drain_active_turns(&self, timeout: Duration) -> TalosResult<()>;
    async fn submit_dynamic_tool_result(
        &self,
        session: TalosSession,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()>;
}
