use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use talos_config::{resolve_talos_home, AgentConfig, AgentKernelConfig, LlmConfig, ProviderConfig};
use talos_error::TalosResult;
use talos_skills::SkillRoots;
use talos_types::agent_dispatch::AgentDispatchResumeContext;
use talos_types::talos::{
    RunItem, StopReason, TalosInputItem, TalosReasoning, TalosSessionEnvironment, TokenUsage,
};
use talos_types::{ModelRef, TalosSession, DEFAULT_SESSION_TITLE};
use tokio::sync::mpsc;

use crate::{
    AgentCapabilities, AgentCapabilityValidator, AgentDescriptor, AgentError, AgentKind,
    AgentResult, AgentStatus,
};

pub use talos_types::talos::AgentEvent;

/// Minimal execution context prepared for an Agent run.
///
/// This context gives Harness and Runtime a stable Agent-domain view of the
/// current execution without making them depend on product store records,
/// Gateway DTOs, or a complete TalosSession.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentRunContext {
    /// Product session id associated with this Agent run.
    pub session_id: String,
    /// Talos turn id; this is also the internal Agent run identity.
    pub turn_id: String,
    /// Product message id used by timeline events for this run.
    pub message_id: String,
    /// Product session owner user id used for workspace resolution.
    pub owner_user_id: String,
    /// Optional execution user id prepared by Service or Coordinator.
    #[serde(default)]
    pub user_id: Option<String>,
    /// Optional workspace routing key, not a filesystem path.
    #[serde(default)]
    pub workspace_id: Option<String>,
    /// Optional resolved project root for the run.
    #[serde(default)]
    pub project_root: Option<PathBuf>,
    /// Optional execution environment prepared for the run.
    #[serde(default)]
    pub environment: Option<TalosSessionEnvironment>,
}

impl AgentRunContext {
    /// Build the minimal Agent run context from a prepared Talos session.
    pub fn from_session(session: &TalosSession, turn_id: String) -> Self {
        Self {
            session_id: session.context.session_id.clone(),
            turn_id,
            message_id: session.context.message_id.clone(),
            owner_user_id: session.user_id.clone(),
            user_id: Some(session.context.user_id.clone()),
            workspace_id: session.context.workspace_id.clone(),
            project_root: session.context.project_root.clone(),
            environment: session.context.environment.clone(),
        }
    }
}

/// Runtime configuration prepared for an Agent run.
///
/// This structure carries only configuration that Runtime needs for execution.
/// Product session state and store records must not be added here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunConfig {
    /// Agent configuration selected for this run.
    pub agent: AgentConfig,
    /// LLM configuration selected for this run.
    pub llm: LlmConfig,
}

impl AgentRunConfig {
    /// Build runtime config from the prepared product session.
    pub fn from_session(session: &TalosSession) -> Self {
        Self {
            agent: session.config.agent.clone(),
            llm: session.config.llm.clone(),
        }
    }

    /// Return the provider config for the selected LLM provider.
    pub fn current_provider_config(&self) -> Option<&ProviderConfig> {
        match self.llm.provider.trim().to_ascii_lowercase().as_str() {
            "anthropic" => Some(&self.llm.anthropic),
            "openai" => Some(&self.llm.openai),
            "bailian" => Some(&self.llm.bailian),
            "ollama" => Some(&self.llm.ollama),
            "openrouter" => Some(&self.llm.openrouter),
            _ => None,
        }
    }
}

/// Lightweight product session state needed by Harness output behavior.
///
/// This is not a DB record or full TalosSession. It only contains the session
/// metadata required for Agent-owned title generation decisions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentRunSessionState {
    /// Current product session title before the run starts.
    pub title: String,
    /// Product message count before the run starts.
    pub message_count: u64,
}

impl AgentRunSessionState {
    /// Build lightweight Agent session state from a prepared product session.
    pub fn from_session(session: &TalosSession) -> Self {
        Self {
            title: session.title.clone(),
            message_count: session.state.message_count,
        }
    }

    /// Return true when the session already has a meaningful user-visible title.
    pub fn has_real_title(&self) -> bool {
        let normalized = self.title.trim();
        !normalized.is_empty() && normalized != DEFAULT_SESSION_TITLE
    }
}

/// Standard options shared by AgentHarness and AgentRuntime.
///
/// Options must represent cross-runtime Agent semantics only. Runtime-private
/// settings belong in AgentDefinition.runtime_config, and continuation policy
/// belongs to AgentInput / AgentResumeParams rather than this structure.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentRunOptions {
    /// Selected model snapshot for cross-runtime checks and diagnostics.
    ///
    /// AgentRunConfig.llm remains the execution source of truth. This snapshot
    /// must not override runtime configuration or create a second model source.
    #[serde(default)]
    pub current_model: Option<String>,
}

impl AgentRunOptions {
    /// Derive cross-runtime run options from the selected Agent run config.
    pub fn from_config(config: &AgentRunConfig) -> Self {
        let current_model = config.llm.model.trim();
        let current_model = if current_model.is_empty() {
            None
        } else {
            Some(config.llm.model.clone())
        };
        Self { current_model }
    }
}

/// Parameters for starting or continuing an AgentHarness run.
///
/// This is the shared execution boundary between Coordinator, AgentHarness,
/// and AgentRuntime. It must carry prepared Agent domain inputs only; store
/// records, Gateway DTOs, and runtime-private SDK objects must not be added.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunParams {
    /// Prepared Agent-domain execution context for this run.
    pub context: AgentRunContext,
    /// Runtime configuration selected for this Agent run.
    pub config: AgentRunConfig,
    /// Lightweight product session state needed by Harness behavior.
    pub session_state: AgentRunSessionState,
    /// Validated continuation context supplied by Service or Coordinator.
    #[serde(default)]
    pub resume_context: Option<AgentDispatchResumeContext>,
    /// Cross-runtime options selected for this run.
    #[serde(default)]
    pub options: AgentRunOptions,
    /// Structured input items for this Agent run.
    pub input: Vec<TalosInputItem>,
}

impl AgentRunParams {
    /// Build Agent run parameters from prepared Agent-domain parts.
    pub fn new(
        context: AgentRunContext,
        config: AgentRunConfig,
        session_state: AgentRunSessionState,
        options: AgentRunOptions,
        input: Vec<TalosInputItem>,
    ) -> Self {
        Self {
            context,
            config,
            session_state,
            resume_context: None,
            options,
            input,
        }
    }

    /// Return the Talos turn id, which is also the internal Agent run identity.
    pub fn turn_id(&self) -> &str {
        &self.context.turn_id
    }

    /// Return the product session id associated with this Agent run.
    pub fn session_id(&self) -> &str {
        &self.context.session_id
    }

    /// Attach validated Service continuation context for the same Agent turn.
    pub fn with_resume_context(mut self, resume_context: AgentDispatchResumeContext) -> Self {
        self.resume_context = Some(resume_context);
        self
    }
}

/// Parameters for continuing an existing Agent run.
///
/// Resume is an internal AgentHarness and AgentRuntime API for same-turn
/// continuation, such as delivering a dynamic tool result back into the
/// runtime loop. It is not a Service timeline recovery API, an attach API, or a
/// Gateway/SDK DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResumeParams {
    /// Original run parameters carrying the same turn_id as the active run.
    pub run: AgentRunParams,
}

impl AgentResumeParams {
    /// Build resume parameters from the original Agent run params.
    pub fn new(run: AgentRunParams) -> Self {
        Self { run }
    }

    /// Return the Talos turn id, which is also the Agent run identity.
    pub fn turn_id(&self) -> &str {
        self.run.turn_id()
    }

    /// Return the product session id associated with the resumed run.
    pub fn session_id(&self) -> &str {
        self.run.session_id()
    }
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
    /// Optional generated session title emitted by Harness.
    #[serde(default)]
    pub generated_session_title: Option<String>,
}

/// Parameters for interrupting an active Agent run.
///
/// Interrupts are addressed by product session and turn identity; callers must
/// not use runtime-private handles as public control identifiers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInterruptParams {
    pub session_id: String,
    pub turn_id: String,
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
    pub turn_id: String,
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
    /// Concrete default model selected for this Agent, when the Harness owns one.
    ///
    /// Coordinators use this only to initialize new product sessions. Runtime
    /// execution must still read the resolved model from AgentRunConfig.
    fn default_model_ref(&self) -> Option<ModelRef> {
        None
    }
    /// Descriptor used for routing, validation, and management surfaces.
    fn descriptor(&self) -> AgentDescriptor {
        AgentDescriptor {
            agent_id: self.id().to_string(),
            name: self.id().to_string(),
            kind: AgentKind::Kernel,
            description: String::new(),
            capabilities: AgentCapabilities::none(),
            status: AgentStatus::Enabled,
        }
    }
    /// Validate a non-streaming run before it reaches Harness lifecycle code.
    fn validate_run_request(&self, request: &AgentRunParams) -> AgentResult<()> {
        AgentCapabilityValidator::for_descriptor(&self.descriptor()).validate_run(request)
    }
    /// Validate a streaming run before it reaches Harness lifecycle code.
    fn validate_streaming_run_request(&self, request: &AgentRunParams) -> AgentResult<()> {
        AgentCapabilityValidator::for_descriptor(&self.descriptor()).validate_streaming_run(request)
    }
    /// Validate a non-streaming same-turn continuation request.
    fn validate_resume_request(&self, params: &AgentResumeParams) -> AgentResult<()> {
        AgentCapabilityValidator::for_descriptor(&self.descriptor()).validate_resume(params)
    }
    /// Validate a streaming same-turn continuation request.
    fn validate_streaming_resume_request(&self, params: &AgentResumeParams) -> AgentResult<()> {
        AgentCapabilityValidator::for_descriptor(&self.descriptor())
            .validate_streaming_resume(params)
    }
    /// Validate an interrupt before it reaches a runtime handle.
    fn validate_interrupt_request(&self, params: &AgentInterruptParams) -> AgentResult<()> {
        AgentCapabilityValidator::for_descriptor(&self.descriptor()).validate_interrupt(params)
    }
    /// Validate structured input before it reaches the single input bridge.
    fn validate_input_request(&self, params: &AgentInputParams) -> AgentResult<()> {
        AgentCapabilityValidator::for_descriptor(&self.descriptor()).validate_input(params)
    }
    /// Execute a non-streaming Agent run.
    async fn run(&self, request: AgentRunParams) -> AgentResult<AgentRunResult>;
    /// Execute an Agent run and stream standardized AgentEvent values.
    async fn run_streamed(
        &self,
        request: AgentRunParams,
    ) -> AgentResult<mpsc::Receiver<AgentEvent>>;
    /// Continue an existing Agent run without streaming.
    ///
    /// The continuation must address the original turn_id. Callers that need
    /// product timeline recovery must use Service timeline APIs instead.
    async fn resume(&self, params: AgentResumeParams) -> AgentResult<AgentRunResult> {
        self.validate_resume_request(&params)?;
        Err(AgentError::capability_unsupported(self.id(), "resume"))
    }
    /// Continue an existing Agent run and stream standardized AgentEvent values.
    ///
    /// This API resumes the runtime loop for the same turn_id. It is not an
    /// attach API for replaying historical events to a client.
    async fn resume_streamed(
        &self,
        params: AgentResumeParams,
    ) -> AgentResult<mpsc::Receiver<AgentEvent>> {
        self.validate_streaming_resume_request(&params)?;
        Err(AgentError::capability_unsupported(
            self.id(),
            "resume_streamed",
        ))
    }
    /// Take a generated title for a completed session turn.
    fn take_generated_session_title(&self, _session_id: &str, _turn_id: &str) -> Option<String> {
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
    async fn interrupt(&self, params: AgentInterruptParams) -> AgentResult<AgentInterruptResult>;
    /// Wait for in-memory active runs to drain.
    async fn drain_active_turns(&self, timeout: Duration) -> TalosResult<()>;
    /// Submit structured input to the single Agent input bridge.
    async fn submit_input(&self, params: AgentInputParams) -> AgentResult<AgentInputResult>;
}
