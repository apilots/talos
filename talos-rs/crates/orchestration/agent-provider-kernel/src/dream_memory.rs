use std::path::PathBuf;
use talos_error::TalosResult;
use talos_llm_provider::{ChatMessage, ChatResponse};
use talos_memory::MemoryScope;

use crate::{KernelAgentProvider, KernelSessionPromptContextSource};

/// Provider-owned inputs for one bounded background Dream Memory formation.
pub struct KernelDreamMemoryParams {
    /// Session whose canonical history is processed.
    pub session_id: String,
    /// Resolved User Workspace root for Provider-private artifacts.
    pub workspace_root: PathBuf,
    /// Typed destination for formed long-term memory.
    pub memory_scope: MemoryScope,
}

/// Provider-owned result projected from the Kernel memory formation outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KernelDreamMemoryResult {
    /// Canonical event sequence covered by the committed memory batch.
    pub covers_until_seq: Option<u64>,
}

impl KernelAgentProvider {
    /// Form Dream Memory through the Provider's configured Kernel boundary.
    pub async fn form_dream_memory<F, Fut>(
        &self,
        params: KernelDreamMemoryParams,
        model_call: F,
    ) -> TalosResult<KernelDreamMemoryResult>
    where
        F: FnOnce(Vec<ChatMessage>) -> Fut,
        Fut: std::future::Future<Output = TalosResult<ChatResponse>>,
    {
        let prompt_context = KernelSessionPromptContextSource::new(
            self.session_history_reader.clone(),
            self.session_artifact_store.clone(),
            params.workspace_root.clone(),
            params.session_id.clone(),
        );
        let mut kernel_config = self
            .base_kernel_config
            .read()
            .expect("Kernel config lock poisoned")
            .clone();
        kernel_config.workspace_dir = params.workspace_root.to_string_lossy().into_owned();
        let result = talos_agent_kernel::form_dream_session_memory(
            talos_agent_kernel::MemoryFormationParams {
                session_id: &params.session_id,
                producer_agent_id: &self.agent_id,
                dream_memory_store: self.dream_memory_store.as_ref(),
                memory_scope: &params.memory_scope,
                session_artifact_store: self.session_artifact_store.as_ref(),
                session_prompt_context_source: &prompt_context,
                memory_progress_store: self.memory_progress_store.as_ref(),
                kernel_config: &kernel_config,
            },
            model_call,
        )
        .await?;
        Ok(KernelDreamMemoryResult {
            covers_until_seq: result.covers_until_seq,
        })
    }
}
