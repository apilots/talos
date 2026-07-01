use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use talos_config::{resolve_talos_home, AgentKernelConfig};
use talos_error::TalosResult;
use talos_skills::SkillRoots;
use talos_types::agent_dispatch::AgentDispatchResumeContext;
use talos_types::talos::{RunItem, StopReason, TalosInputItem, TalosReasoning, TokenUsage};
use talos_types::TalosSession;
use tokio::sync::mpsc;

use crate::{AgentCapabilities, AgentDescriptor, AgentKind, AgentStatus};

pub use talos_types::talos::AgentEvent;

/// Parameters for starting or continuing an AgentHarness run.
///
/// This is the shared execution boundary between Coordinator, AgentHarness,
/// and AgentRuntime. It must carry prepared Agent domain inputs only; store
/// records, Gateway DTOs, and runtime-private SDK objects must not be added.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunParams {
    /// Prepared Talos session domain object for this run.
    pub session: TalosSession,
    /// Talos turn id and Agent run identity.
    #[serde(default)]
    pub turn_id: Option<String>,
    /// Validated continuation context supplied by Service or Coordinator.
    #[serde(default)]
    pub resume_context: Option<AgentDispatchResumeContext>,
    /// Structured input items for this Agent run.
    pub input: Vec<TalosInputItem>,
}

/// Optional title update produced by an Agent run.
///
/// This remains Harness output metadata; Service decides whether and how to
/// persist it into product session state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTitleUpdate {
    pub title: String,
}

/// Standard result produced by a completed AgentHarness run.
///
/// Concrete runtimes must convert backend-specific outputs into this shared
/// result shape before crossing the AgentRuntime boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunResult {
    /// Final assistant text assembled for product history.
    pub output_text: String,
    /// Structured transcript items emitted by the run.
    #[serde(default)]
    pub output_items: Vec<RunItem>,
    /// Reasoning summaries or traces safe for the Agent domain.
    #[serde(default)]
    pub reasoning: Vec<TalosReasoning>,
    /// Standard stop reason for the run.
    pub stop_reason: StopReason,
    /// Token usage shared across run, history, metrics, and compaction paths.
    #[serde(default)]
    pub usage: TokenUsage,
    /// Number of executed internal tasks or tool batches.
    pub task_count: u32,
    /// Product session id; this is not a backend session or thread id.
    pub session_id: String,
    /// Optional session title update emitted by Harness.
    #[serde(default)]
    pub session_title_update: Option<SessionTitleUpdate>,
}

/// Parameters for interrupting an active Agent run.
///
/// Interrupts are addressed by product session and turn identity; callers must
/// not use runtime-private handles as public control identifiers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInterruptParams {
    pub session_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
}

/// Result of an Agent interrupt request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentInterruptResult {
    /// True when an active run accepted the interrupt signal.
    pub interrupted: bool,
}

/// Parameters for delivering structured input to an Agent run.
///
/// This is the single Harness input bridge. New input kinds should extend
/// AgentInput rather than adding parallel submit_* APIs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInputParams {
    /// Prepared Talos session domain object for input validation and context.
    pub session: TalosSession,
    /// Talos turn id and Agent run identity associated with this input.
    #[serde(default)]
    pub turn_id: Option<String>,
    /// Tagged input payload accepted by the Harness input bridge.
    pub input: AgentInput,
}

/// Structured input accepted by AgentHarness.
///
/// Variants must represent standard Agent-domain inputs. Runtime-private SDK
/// payloads must be converted before reaching this enum.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentInput {
    /// Result for a pending dynamic tool call validated by Service.
    DynamicToolResult {
        call_id: String,
        tool_name: String,
        output: String,
        success: bool,
    },
}

/// Result of delivering structured input to AgentHarness.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentInputResult {
    /// True when the Harness accepted the input for processing.
    pub accepted: bool,
}

/// Resolves skill roots for a concrete Agent runtime configuration.
///
/// Implementations provide Harness-owned infrastructure to runtimes without
/// making runtimes read global configuration or product stores directly.
pub trait SkillRootsResolver: Send + Sync {
    fn skill_roots(&self, kernel_config: &AgentKernelConfig) -> TalosResult<SkillRoots>;
}

/// Default resolver used when no external skill roots provider is installed.
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

/// Create the default SkillRootsResolver for Harness construction.
pub fn empty_skill_roots_resolver() -> Arc<dyn SkillRootsResolver> {
    Arc::new(EmptySkillRootsResolver)
}

/// Public facade used by Coordinator to run and control an Agent.
///
/// AgentHarness owns common Agent framework behavior such as run lifecycle,
/// input bridging, interrupts, event normalization, and session lifecycle
/// adapters. It must not expose store records, Gateway DTOs, or concrete
/// runtime-private objects.
#[async_trait]
pub trait AgentHarness: Send + Sync {
    /// Stable Agent identity registered in Coordinator.
    fn id(&self) -> &str;
    /// Descriptor used for routing, validation, and management surfaces.
    fn descriptor(&self) -> AgentDescriptor {
        AgentDescriptor {
            agent_id: self.id().to_string(),
            name: self.id().to_string(),
            kind: AgentKind::Kernel,
            description: String::new(),
            capabilities: AgentCapabilities::kernel(),
            status: AgentStatus::Enabled,
        }
    }
    /// Execute a non-streaming Agent run.
    async fn run(&self, request: AgentRunParams) -> TalosResult<AgentRunResult>;
    /// Execute an Agent run and stream standardized AgentEvent values.
    async fn run_streamed(
        &self,
        request: AgentRunParams,
    ) -> TalosResult<mpsc::Receiver<AgentEvent>>;
    /// Take a title update generated for a completed session turn.
    fn take_session_title_update(
        &self,
        _session_id: &str,
        _turn_id: &str,
    ) -> Option<SessionTitleUpdate> {
        None
    }
    /// Compact Agent-maintained session context and return token usage.
    async fn compact_session(&self, session: TalosSession) -> TalosResult<TokenUsage>;
    /// Clear Agent-maintained session state without deleting product records.
    async fn clear_session(&self, session: TalosSession) -> TalosResult<()>;
    /// Close Agent-maintained session resources.
    async fn close_session(&self, session: TalosSession) -> TalosResult<()>;
    /// Wait for pending Harness-owned memory flushes to finish.
    async fn drain_memory_flushes(&self, timeout: Duration) -> TalosResult<()>;
    /// Return the number of Harness-owned memory flushes still pending.
    fn pending_memory_flushes(&self) -> usize;
    /// Check whether a session has pending Harness-owned memory flushes.
    async fn has_pending_memory_flush(&self, session_id: &str) -> bool;
    /// Check whether Harness currently owns an in-memory active run.
    fn has_active_turn(&self, session_id: &str) -> bool;
    /// Interrupt an active Agent run by product session and turn identity.
    async fn interrupt(&self, params: AgentInterruptParams) -> TalosResult<AgentInterruptResult>;
    /// Wait for in-memory active runs to drain.
    async fn drain_active_turns(&self, timeout: Duration) -> TalosResult<()>;
    /// Submit structured input to the single Agent input bridge.
    async fn submit_input(&self, params: AgentInputParams) -> TalosResult<AgentInputResult>;
}
