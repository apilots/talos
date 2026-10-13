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
    pub config: Arc<std::sync::RwLock<talos_config::Config>>,
    pub execution_gate: Arc<tokio::sync::RwLock<()>>,
    pub workspace_memory: Arc<UserWorkspaceMemoryService>,
}

impl WorkerHarness {
    fn resolve_context(&self, context: &mut AgentContext) -> talos_error::TalosResult<()> {
        let config = self.config.read().expect("Worker config lock poisoned");
        context.session.workspace = config.resolve_workspace(
            &context.session.workspace.workspace_id,
            &context.session.workspace.user_id,
        )?;
        let catalog = talos_types::resolve_model_catalog(&config.llm);
        context.model = catalog
            .into_iter()
            .find(|model| {
                model.provider == context.model_selection.model_ref.provider
                    && model.model.id == context.model_selection.model_ref.id
            })
            .ok_or_else(|| {
                talos_error::TalosError::new(
                    talos_error::ErrorCode::ProviderUnavailable,
                    "The requested model is not configured on this Computer",
                )
            })?;
        context.model_selection.resolve_reasoning(&context.model)?;
        Ok(())
    }
}

impl AgentHarness for WorkerHarness {
    fn start(&self, params: AgentRunParams) -> AgentFuture<'_, AgentResult<AgentRunExecution>> {
        Box::pin(async move {
            let guard = self.execution_gate.clone().read_owned().await;
            let mut params = params;
            self.resolve_context(&mut params.context)?;
            let execution = self.execution.start(params).await?;
            Ok(guard_execution(execution, guard))
        })
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
        Box::pin(async move {
            let guard = self.execution_gate.clone().read_owned().await;
            let mut context = context;
            self.resolve_context(&mut context)?;
            let execution = self.execution.compact_session(context).await?;
            Ok(guard_execution(execution, guard))
        })
    }
    fn clear_session(
        &self,
        context: AgentSessionContext,
        expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> AgentFuture<'_, AgentResult<()>> {
        self.execution
            .clear_session(context, expected_history_cursor)
    }
    fn close_session(
        &self,
        context: AgentSessionContext,
        expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> AgentFuture<'_, AgentResult<()>> {
        self.execution
            .close_session(context, expected_history_cursor)
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
            let _guard = self.execution_gate.clone().read_owned().await;
            let root = context.workspace.root.clone();
            let result = self
                .provider
                .resources_for(&context)?
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

/// Retain the node execution guard through the actual invocation settlement.
/// The existing paused admission, event channel and result remain unchanged.
fn guard_execution(
    mut execution: AgentRunExecution,
    guard: tokio::sync::OwnedRwLockReadGuard<()>,
) -> AgentRunExecution {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let settlement = execution.settlement;
    tokio::spawn(async move {
        let result = settlement.recv().await;
        let _ = sender.send(result);
        drop(guard);
    });
    execution.settlement = AgentRunSettlementReceiver::new(receiver);
    execution
}

#[cfg(test)]
#[path = "harness_tests.rs"]
mod tests;
