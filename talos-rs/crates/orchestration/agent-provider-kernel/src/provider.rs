use std::sync::Arc;

use talos_agent::{
    AgentCapabilities, AgentContext, AgentOperationControl, AgentProvider, AgentResult,
    AgentSession, AgentSessionContext, AgentUserActionKind, AgentUserContentKind, AttachmentReader,
    SessionCompactionParams, SessionCompactionResult, SessionHistoryReader, SessionTurnParams,
    SessionTurnResult,
};
use talos_agent_kernel::{
    AgentKernel, AgentKernelDependencies, KernelMemoryProgressStore, KernelSessionArtifactStore,
};
use talos_config::{AgentKernelConfig, ToolUserPermissionModeConfig};
use talos_error::TalosError;
use talos_llm_provider::LlmProviderRouter;
use talos_memory::{DreamMemoryFormationStore, UserWorkspaceMemoryReader};
use talos_tools::ToolRegistry;

use crate::{
    normalize_kernel_config, KernelSessionPromptContextSource, KernelTranscriptStore,
    SkillRootsResolver,
};

/// Construction inputs for one configured built-in Kernel Agent Provider.
pub struct KernelAgentProviderParams {
    /// Stable Agent Definition identity bound to every accepted Context.
    pub agent_id: String,
    /// Agent display name injected into Kernel prompts.
    pub name: String,
    /// Agent purpose injected into Kernel prompts.
    pub description: String,
    /// Router used to resolve the model selected by each Context.
    pub llm_provider_router: Arc<dyn LlmProviderRouter>,
    /// Shared registry used for Kernel Tool definitions and execution.
    pub tool_registry: Arc<ToolRegistry>,
    /// Validated base configuration specialized per Context at execution time.
    pub kernel_config: AgentKernelConfig,
    /// Resolver for configured and installed skill roots.
    pub skill_roots_resolver: Arc<dyn SkillRootsResolver>,
    /// Private Kernel transcript and summary storage.
    pub session_artifact_store: Arc<dyn KernelSessionArtifactStore>,
    /// Canonical completed Session history supplied by Talos Service.
    pub session_history_reader: Arc<dyn SessionHistoryReader>,
    /// Existing Session usage cursor storage required by Kernel compaction.
    pub memory_progress_store: Arc<dyn KernelMemoryProgressStore>,
    /// Dream Memory persistence used during Kernel memory formation.
    pub dream_memory_store: Arc<dyn DreamMemoryFormationStore>,
    /// Reader for the Context-scoped User Workspace Memory projection.
    pub user_workspace_memory_reader: Arc<dyn UserWorkspaceMemoryReader>,
    /// Authorized resolver for structured file input.
    pub attachment_reader: Arc<dyn AttachmentReader>,
    /// Exact rich-input content kinds exposed by this Provider instance.
    pub rich_input: Vec<AgentUserContentKind>,
}

/// Built-in Provider adapter that exclusively owns Agent API/Kernel mapping.
#[derive(Clone)]
pub struct KernelAgentProvider {
    pub(crate) agent_id: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) llm_provider_router: Arc<dyn LlmProviderRouter>,
    pub(crate) tool_registry: Arc<ToolRegistry>,
    pub(crate) base_kernel_config: AgentKernelConfig,
    pub(crate) skill_roots_resolver: Arc<dyn SkillRootsResolver>,
    pub(crate) session_artifact_store: Arc<dyn KernelSessionArtifactStore>,
    pub(crate) session_history_reader: Arc<dyn SessionHistoryReader>,
    pub(crate) memory_progress_store: Arc<dyn KernelMemoryProgressStore>,
    pub(crate) dream_memory_store: Arc<dyn DreamMemoryFormationStore>,
    pub(crate) user_workspace_memory_reader: Arc<dyn UserWorkspaceMemoryReader>,
    pub(crate) attachment_reader: Arc<dyn AttachmentReader>,
    capabilities: AgentCapabilities,
}

impl KernelAgentProvider {
    /// Build one Provider after validating its complete capability declaration.
    pub fn new(params: KernelAgentProviderParams) -> AgentResult<Self> {
        validate_rich_input(&params.rich_input)?;
        let capabilities = AgentCapabilities {
            rich_input: params.rich_input,
            dynamic_input: true,
            user_actions: vec![
                AgentUserActionKind::DynamicToolResult,
                AgentUserActionKind::Question,
                AgentUserActionKind::SandboxApproval,
            ],
            durable_continuation: true,
            context_compaction: true,
        };
        Ok(Self {
            agent_id: params.agent_id,
            name: params.name,
            description: params.description,
            llm_provider_router: params.llm_provider_router,
            tool_registry: params.tool_registry,
            base_kernel_config: normalize_kernel_config(params.kernel_config),
            skill_roots_resolver: params.skill_roots_resolver,
            session_artifact_store: params.session_artifact_store,
            session_history_reader: params.session_history_reader,
            memory_progress_store: params.memory_progress_store,
            dream_memory_store: params.dream_memory_store,
            user_workspace_memory_reader: params.user_workspace_memory_reader,
            attachment_reader: params.attachment_reader,
            capabilities,
        })
    }

    pub(crate) fn validate_agent_id(&self, agent_id: &str) -> AgentResult<()> {
        if agent_id == self.agent_id {
            return Ok(());
        }
        Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "Kernel Provider received another Agent identity",
        ))
    }

    pub(crate) fn effective_kernel_config(&self, context: &AgentContext) -> AgentKernelConfig {
        let mut config = self.base_kernel_config.clone();
        config.workspace_dir = context
            .session
            .workspace
            .root
            .to_string_lossy()
            .into_owned();
        config.tools.sandbox_policy.user_permission_mode = match context.permission_mode {
            talos_types::UserPermissionMode::AskForApproval => {
                ToolUserPermissionModeConfig::AskForApproval
            }
            talos_types::UserPermissionMode::ApproveForMe => {
                ToolUserPermissionModeConfig::ApproveForMe
            }
            talos_types::UserPermissionMode::FullAccess => ToolUserPermissionModeConfig::FullAccess,
        };
        config
    }
}

/// Kernel-backed in-process handle for one Talos Agent Session.
///
/// This handle binds the immutable Session Context once and delegates Kernel
/// mapping to the owning Provider. It does not own Talos persistence or a
/// second Turn lifecycle state machine.
#[derive(Clone)]
pub struct KernelAgentSession {
    pub(crate) provider: KernelAgentProvider,
    pub(crate) context: AgentSessionContext,
    pub(crate) kernel: AgentKernel,
    pub(crate) prompt_context: Arc<KernelSessionPromptContextSource>,
}

impl KernelAgentSession {
    fn validate_context(&self, context: &AgentContext) -> AgentResult<()> {
        if context.session == self.context {
            return Ok(());
        }
        Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "Kernel Agent Session received another Session Context",
        ))
    }
}

impl AgentProvider for KernelAgentProvider {
    type Session = KernelAgentSession;

    fn capabilities(&self) -> &AgentCapabilities {
        &self.capabilities
    }

    async fn session(
        &self,
        context: AgentSessionContext,
        _control: AgentOperationControl,
    ) -> AgentResult<Self::Session> {
        self.validate_agent_id(&context.agent_id)?;
        let transcript = Arc::new(KernelTranscriptStore::new(
            self.session_artifact_store.clone(),
            context.workspace.root.clone(),
        ));
        let prompt_context = Arc::new(KernelSessionPromptContextSource::new(
            self.session_history_reader.clone(),
            self.session_artifact_store.clone(),
            context.workspace.root.clone(),
            context.session_id.clone(),
        ));
        let kernel = AgentKernel::new(AgentKernelDependencies {
            agent_name: self.name.clone(),
            agent_description: self.description.clone(),
            llm_provider_router: self.llm_provider_router.clone(),
            tool_definition_provider: self.tool_registry.clone(),
            tool_executor: self.tool_registry.clone(),
            dream_memory_store: self.dream_memory_store.clone(),
            transcript,
            artifacts: self.session_artifact_store.clone(),
            prompt_context: prompt_context.clone(),
            memory_progress: self.memory_progress_store.clone(),
        });
        Ok(KernelAgentSession {
            provider: self.clone(),
            context,
            kernel,
            prompt_context,
        })
    }

    async fn shutdown(&self, _control: AgentOperationControl) -> AgentResult<()> {
        Ok(())
    }
}

impl AgentSession for KernelAgentSession {
    async fn turn(&self, turn: SessionTurnParams) -> AgentResult<SessionTurnResult> {
        self.validate_context(&turn.context)?;
        crate::execution::execute(self, turn).await
    }

    async fn compact(
        &self,
        params: SessionCompactionParams,
    ) -> AgentResult<SessionCompactionResult> {
        self.validate_context(&params.context)?;
        crate::compaction::compact_session(self, params).await
    }

    async fn clear(&self, _control: AgentOperationControl) -> AgentResult<()> {
        self.provider
            .session_artifact_store
            .reset_session(&self.context.workspace.root, &self.context.session_id)
            .await
    }

    async fn close(&self, _control: AgentOperationControl) -> AgentResult<()> {
        Ok(())
    }
}

fn validate_rich_input(content_kinds: &[AgentUserContentKind]) -> AgentResult<()> {
    if content_kinds.is_empty() {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "Kernel Provider rich input requires content kinds",
        ));
    }
    let mut unique_kinds = content_kinds.to_vec();
    unique_kinds.sort_unstable();
    unique_kinds.dedup();
    if unique_kinds.len() != content_kinds.len() {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "Kernel Provider rich input contains duplicate content kinds",
        ));
    }
    if content_kinds.iter().any(|kind| {
        matches!(
            kind,
            AgentUserContentKind::Audio | AgentUserContentKind::Video
        )
    }) {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "Kernel Provider cannot advertise unsupported audio or video input",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;
