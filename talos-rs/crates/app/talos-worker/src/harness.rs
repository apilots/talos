use agent_protocol::*;
use std::sync::Arc;
use std::time::Duration;
use talos_memory::{MemoryScope, UserWorkspaceMemoryUpdate};
use talos_service::UserWorkspaceMemoryService;

/// Adds execution-node Workspace access to the original execution Harness.
/// Both embedded and standalone hosts construct this same adapter.
pub(crate) struct WorkerHarness {
    pub provider: talos_agent_provider_kernel::KernelAgentProvider,
    pub execution: Arc<dyn AgentHarness>,
    pub workspace_memory: Arc<UserWorkspaceMemoryService>,
}

impl AgentHarness for WorkerHarness {
    fn start(&self, params: AgentRunParams) -> AgentFuture<'_, AgentResult<AgentRunExecution>> {
        self.execution.start(params)
    }
    fn submit_input(
        &self,
        params: AgentInputParams,
    ) -> AgentFuture<'_, AgentResult<AgentAdmission>> {
        self.execution.submit_input(params)
    }
    fn interrupt(
        &self,
        params: AgentInterruptParams,
    ) -> AgentFuture<'_, AgentResult<talos_agent_api::AgentInterruptOutcome>> {
        self.execution.interrupt(params)
    }
    fn compact_session(
        &self,
        context: AgentContext,
    ) -> AgentFuture<'_, AgentResult<AgentRunExecution>> {
        self.execution.compact_session(context)
    }
    fn clear_session(&self, context: AgentSessionContext) -> AgentFuture<'_, AgentResult<()>> {
        self.execution.clear_session(context)
    }
    fn close_session(&self, context: AgentSessionContext) -> AgentFuture<'_, AgentResult<()>> {
        self.execution.close_session(context)
    }
    fn shutdown(&self, timeout: Duration) -> AgentFuture<'_, AgentResult<()>> {
        self.execution.shutdown(timeout)
    }

    fn form_dream_memory(
        &self,
        context: AgentSessionContext,
        model_call: DreamMemoryModelCall,
    ) -> AgentFuture<'_, AgentResult<Option<u64>>> {
        Box::pin(async move {
            let root = context.workspace.root.clone();
            let result = self
                .provider
                .form_dream_memory(
                    talos_agent_provider_kernel::KernelDreamMemoryParams {
                        session_id: context.session_id.clone(),
                        workspace_root: root,
                        memory_scope: scope(context),
                    },
                    move |messages| model_call(messages),
                )
                .await?;
            Ok(result.covers_until_seq)
        })
    }

    fn read_workspace_memory(
        &self,
        context: AgentSessionContext,
    ) -> AgentFuture<'_, AgentResult<talos_memory::UserWorkspaceMemorySnapshot>> {
        Box::pin(async move { self.workspace_memory.read(&scope(context)).await })
    }
    fn update_workspace_memory(
        &self,
        context: AgentSessionContext,
        update: UserWorkspaceMemoryUpdate,
    ) -> AgentFuture<'_, AgentResult<talos_memory::UserWorkspaceMemorySnapshot>> {
        Box::pin(async move {
            let scope = scope(context);
            match update {
                UserWorkspaceMemoryUpdate::CreateEntry { entry } => {
                    self.workspace_memory.create_entry(&scope, entry).await
                }
                UserWorkspaceMemoryUpdate::UpdateEntry { key, update } => {
                    self.workspace_memory
                        .update_entry(&scope, &key, update)
                        .await
                }
                UserWorkspaceMemoryUpdate::UpsertEntry { entry } => {
                    self.workspace_memory.upsert_entry(&scope, entry).await
                }
                UserWorkspaceMemoryUpdate::ReplaceEntry { entry } => {
                    self.workspace_memory.replace_entry(&scope, entry).await
                }
                UserWorkspaceMemoryUpdate::RemoveEntry { key } => {
                    self.workspace_memory.remove_entry(&scope, &key).await
                }
                UserWorkspaceMemoryUpdate::ReplaceDocument {
                    content,
                    base_version,
                } => {
                    self.workspace_memory
                        .replace_document(&scope, content, base_version)
                        .await
                }
            }
        })
    }
}

fn scope(context: AgentSessionContext) -> MemoryScope {
    MemoryScope {
        workspace_id: context.workspace.workspace_id,
        user_id: context.workspace.user_id,
        workspace_root: context.workspace.root,
    }
}
