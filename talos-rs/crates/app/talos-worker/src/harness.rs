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
    pub model_router: Arc<dyn talos_llm_provider::LlmProviderRouter>,
    pub history: Arc<talos_store::execution::WorkerExecutionStore>,
    pub workspace_memory: Arc<UserWorkspaceMemoryService>,
    pub browser: Arc<talos_browser::BrowserRuntime>,
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
    fn read_session_execution_state(
        &self,
        session: AgentSessionContext,
    ) -> AgentFuture<'_, AgentResult<Option<SessionExecutionState>>> {
        self.execution.read_session_execution_state(session)
    }

    fn read_execution_payload(
        &self,
        params: AgentExecutionPayloadParams,
    ) -> AgentFuture<'_, AgentResult<AgentExecutionPayloadResult>> {
        self.execution.read_execution_payload(params)
    }
    fn read_session_events(
        &self,
        params: AgentSessionEventsParams,
    ) -> AgentFuture<'_, AgentResult<AgentSessionEventsResult>> {
        self.execution.read_session_events(params)
    }

    fn replay_admitted_run(
        &self,
        mut params: AgentRunParams,
    ) -> AgentFuture<'_, AgentResult<Option<AgentRunExecution>>> {
        Box::pin(async move {
            self.resolve_context(&mut params.context)?;
            self.execution.replay_admitted_run(params).await
        })
    }

    fn start(&self, params: AgentRunParams) -> AgentFuture<'_, AgentResult<AgentRunExecution>> {
        Box::pin(async move {
            let guard = self.execution_gate.clone().read_owned().await;
            let mut params = params;
            self.resolve_context(&mut params.context)?;
            let browser_guard = BrowserTurnGuard {
                browser: self.browser.clone(),
                session_id: params.context.session.session_id.clone(),
                turn_id: params.context.turn_id.clone(),
            };
            let execution = self.execution.start(params).await?;
            Ok(guard_execution(execution, guard, Some(browser_guard)))
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
            Ok(guard_execution(execution, guard, None))
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
    fn retire_session(
        &self,
        context: AgentSessionContext,
        expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> AgentFuture<'_, AgentResult<()>> {
        self.execution
            .retire_session(context, expected_history_cursor)
    }
    fn close_session(
        &self,
        context: AgentSessionContext,
        expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> AgentFuture<'_, AgentResult<()>> {
        self.execution
            .close_session(context, expected_history_cursor)
    }
    fn stop_execution(&self, timeout: Duration) -> AgentFuture<'_, AgentResult<()>> {
        self.execution.stop_execution(timeout)
    }

    fn shutdown(&self, timeout: Duration) -> AgentFuture<'_, AgentResult<()>> {
        self.execution.shutdown(timeout)
    }

    fn generate_session_title(
        &self,
        params: AgentSessionTitleParams,
    ) -> AgentFuture<'_, AgentResult<Option<AgentSessionTitleResult>>> {
        Box::pin(self.generate_title(params))
    }

    fn form_dream_memory(
        &self,
        params: DreamMemoryParams,
    ) -> AgentFuture<'_, AgentResult<DreamMemoryResult>> {
        Box::pin(async move {
            let _guard = self.execution_gate.clone().read_owned().await;
            let mut params = params;
            {
                let config = self.config.read().expect("Worker config lock poisoned");
                params.session.workspace = config.resolve_workspace(
                    &params.session.workspace.workspace_id,
                    &params.session.workspace.user_id,
                )?;
            }
            let replay = self.history.read_dream_formation(&params).await?;
            if replay.formation.is_some() {
                return Ok(replay);
            }
            let context = params.session;
            let observation = Arc::new(tokio::sync::Mutex::new(None));
            let observed = observation.clone();
            let history = self.history.clone();
            let session = context.clone();
            let result = self
                .provider
                .resources_for(&context)?
                .form_dream_memory(
                    talos_agent_provider_kernel::KernelDreamMemoryParams {
                        session_id: context.session_id.clone(),
                        workspace_root: context.workspace.root.clone(),
                        memory_scope: scope(context.clone()),
                    },
                    |messages| async move {
                        let (response, usage) =
                            crate::builtin_model::chat(&self.config, &self.model_router, messages)
                                .await?;
                        let formation = DreamMemoryFormation {
                            covers_until_seq: None,
                            model_usage: usage,
                            failure: Some(
                                talos_error::TalosError::new(
                                    talos_error::ErrorCode::ServiceUnavailable,
                                    "Dream Memory formation was interrupted after its model call.",
                                )
                                .to_api_error(),
                            ),
                        };
                        let cursor = history.append_dream_formation(&session, &formation).await?;
                        *observed.lock().await = Some((cursor, formation));
                        Ok(response)
                    },
                )
                .await;
            let Some((cursor, mut formation)) = observation.lock().await.take() else {
                result?;
                return Ok(replay);
            };
            match result {
                Ok(result) => {
                    formation.covers_until_seq = result.covers_until_seq;
                    formation.failure = None;
                }
                Err(error) => {
                    talos_logging::report_error(&error);
                    formation.failure = Some(error.to_api_error());
                }
            }
            self.history
                .finish_dream_formation(&context, &cursor, &formation)
                .await?;
            Ok(DreamMemoryResult {
                formation: Some(formation),
                next_cursor: cursor,
                processed_until_seq: self.history.memory_progress(&context.session_id).await?,
            })
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
    browser_guard: Option<BrowserTurnGuard>,
) -> AgentRunExecution {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let settlement = execution.settlement;
    tokio::spawn(async move {
        let result = settlement.recv().await;
        drop(browser_guard);
        drop(guard);
        let _ = sender.send(result);
    });
    execution.settlement = AgentRunSettlementReceiver::new(receiver);
    execution
}

#[path = "session_title.rs"]
mod session_title;

#[cfg(test)]
#[path = "harness_tests.rs"]
mod tests;

struct BrowserTurnGuard {
    browser: Arc<talos_browser::BrowserRuntime>,
    session_id: String,
    turn_id: String,
}
impl Drop for BrowserTurnGuard {
    fn drop(&mut self) {
        self.browser.finish_turn(&self.session_id, &self.turn_id);
    }
}
