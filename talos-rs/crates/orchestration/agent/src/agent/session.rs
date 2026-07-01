use super::runtime::AgentRuntimeAbortHandle;
use super::{ActiveTurnHandle, AgentHarnessImpl, QueuedTurn};
use chrono::Utc;
use dashmap::mapref::entry::Entry;
use std::collections::VecDeque;
use std::time::Duration;
use talos_config::AgentKernelConfig;
use talos_error::{TalosError, TalosResult};
use talos_memory::{Memory, MemoryStore};
use talos_memory::{
    SessionTranscriptActions, SessionTranscriptAuthor, SessionTranscriptBranch,
    SessionTranscriptDynamicToolResponsePayload, SessionTranscriptEventKind,
    SessionTranscriptHeader, SessionTranscriptPayload, SessionTranscriptRecord,
};
use talos_types::{talos::TokenUsage, TalosSession};
use uuid::Uuid;

impl AgentHarnessImpl {
    async fn ensure_session_is_idle(&self, session_id: &str, action: &str) -> TalosResult<()> {
        if self.active_turns.contains_key(session_id) {
            return Err(TalosError::ValidationError(format!(
                "cannot {} session `{}` while a turn is active",
                action, session_id
            )));
        }
        if self.has_queued_turns(session_id).await {
            return Err(TalosError::ValidationError(format!(
                "cannot {} session `{}` while queued turns are pending",
                action, session_id
            )));
        }
        Ok(())
    }

    pub(crate) fn try_reserve_active_turn(&self, session_id: &str) -> TalosResult<()> {
        match self.active_turns.entry(session_id.to_string()) {
            Entry::Occupied(_) => Err(TalosError::ValidationError(format!(
                "cannot start a new turn for session `{session_id}` while another turn is active"
            ))),
            Entry::Vacant(entry) => {
                entry.insert(ActiveTurnHandle::Reserved);
                Ok(())
            }
        }
    }

    pub(crate) fn activate_reserved_turn(
        &self,
        session_id: &str,
        turn_id: Option<String>,
        execution_task: AgentRuntimeAbortHandle,
    ) -> TalosResult<()> {
        match self.active_turns.entry(session_id.to_string()) {
            Entry::Occupied(mut entry) => {
                entry.insert(ActiveTurnHandle::Running {
                    turn_id,
                    execution_task,
                });
                Ok(())
            }
            Entry::Vacant(_) => Err(TalosError::InternalError(format!(
                "missing reserved active turn slot for session `{session_id}`"
            ))),
        }
    }

    pub(crate) fn clear_active_turn(&self, session_id: &str) {
        self.active_turns.remove(session_id);
    }

    pub(crate) async fn enqueue_turn(&self, session_id: &str, queued_turn: QueuedTurn) -> usize {
        let mut backlogs = self.session_backlogs.lock().await;
        let backlog = backlogs
            .entry(session_id.to_string())
            .or_insert_with(VecDeque::new);
        backlog.push_back(queued_turn);
        backlog.len()
    }

    pub(crate) async fn dequeue_next_turn(&self, session_id: &str) -> Option<QueuedTurn> {
        let mut backlogs = self.session_backlogs.lock().await;
        let backlog = backlogs.get_mut(session_id)?;
        let next = backlog.pop_front();
        if backlog.is_empty() {
            backlogs.remove(session_id);
        }
        next
    }

    pub(crate) async fn has_queued_turns(&self, session_id: &str) -> bool {
        self.session_backlogs
            .lock()
            .await
            .get(session_id)
            .is_some_and(|backlog| !backlog.is_empty())
    }

    pub(crate) async fn drain_pending_memory_flushes(&self, timeout: Duration) -> TalosResult<()> {
        self.memory_flush_manager.drain(timeout).await
    }

    pub(crate) fn pending_memory_flush_count(&self) -> usize {
        self.memory_flush_manager.pending_count()
    }

    pub(crate) async fn has_pending_memory_flush(&self, session_id: &str) -> bool {
        self.memory_flush_manager.contains_session(session_id).await
    }

    pub(crate) fn has_active_turn(&self, session_id: &str) -> bool {
        self.active_turns.contains_key(session_id)
    }

    pub(crate) async fn drain_active_turns(&self, timeout: Duration) -> TalosResult<()> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if self.active_turns.is_empty() {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                let remaining: Vec<_> = self
                    .active_turns
                    .iter()
                    .map(|entry| entry.key().clone())
                    .collect();
                // Force-abort any remaining running turns
                for session_id in &remaining {
                    if let Some(handle) = self.active_turns.get(session_id) {
                        if let ActiveTurnHandle::Running { execution_task, .. } = handle.value() {
                            execution_task.abort();
                        }
                    }
                }
                // Give aborted tasks a moment to observe cancellation
                tokio::time::sleep(Duration::from_millis(100)).await;
                // Clear forcibly-aborted entries
                for session_id in &remaining {
                    self.clear_active_turn(session_id);
                }
                return Err(TalosError::InternalError(format!(
                    "drain_active_turns timed out with {} sessions still active: {:?}",
                    remaining.len(),
                    remaining
                )));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    pub(crate) async fn validate_dynamic_tool_submission(
        &self,
        memory_store: &MemoryStore,
        kernel_config: &AgentKernelConfig,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
    ) -> TalosResult<()> {
        let records = memory_store
            .load_transcript_records(session_id, None, SessionTranscriptBranch::ReactiveLoop)
            .await?;

        let mut latest_request_timestamp = None;
        let mut latest_request_tool_name = None;
        let mut latest_response_timestamp = None;

        for record in records {
            match record.payload {
                SessionTranscriptPayload::DynamicToolRequest { payload }
                    if payload.call_id == call_id =>
                {
                    latest_request_timestamp = Some(record.header.timestamp);
                    latest_request_tool_name = Some(payload.tool_name);
                }
                SessionTranscriptPayload::DynamicToolResponse { payload }
                    if payload.call_id == call_id =>
                {
                    latest_response_timestamp = Some(record.header.timestamp);
                }
                _ => {}
            }
        }

        let Some(request_timestamp) = latest_request_timestamp else {
            return Err(talos_error::TalosError::ToolError(format!(
                "dynamic tool call `{call_id}` is not pending for session `{session_id}`"
            )));
        };

        if latest_request_tool_name.as_deref() != Some(tool_name) {
            return Err(talos_error::TalosError::ToolError(format!(
                "dynamic tool call `{call_id}` belongs to `{}` not `{tool_name}`",
                latest_request_tool_name.unwrap_or_else(|| "unknown".to_string())
            )));
        }

        if latest_response_timestamp.is_some_and(|ts| ts > request_timestamp) {
            return Err(talos_error::TalosError::ToolError(format!(
                "dynamic tool call `{call_id}` has already been resolved"
            )));
        }

        let timeout_ms = kernel_config.tools.dynamic.response_timeout_ms;
        if timeout_ms > 0 {
            let elapsed_ms = Utc::now()
                .signed_duration_since(request_timestamp)
                .num_milliseconds()
                .max(0) as u64;
            if elapsed_ms >= timeout_ms {
                return Err(talos_error::TalosError::ToolError(format!(
                    "dynamic tool call `{call_id}` timed out after {timeout_ms} ms"
                )));
            }
        }

        Ok(())
    }

    pub(crate) async fn submit_dynamic_tool_result(
        &self,
        session: TalosSession,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        let session_id = session.context.session_id.clone();
        let (kernel_config, memory_store) = self.memory_store_for_session(&session).await?;
        self.validate_dynamic_tool_submission(
            memory_store.as_ref(),
            &kernel_config,
            &session_id,
            call_id,
            tool_name,
        )
        .await?;
        let record = SessionTranscriptRecord {
            header: SessionTranscriptHeader {
                id: Uuid::new_v4().to_string(),
                session_id: session_id.to_string(),
                invocation_id: Uuid::new_v4().to_string(),
                branch: SessionTranscriptBranch::ReactiveLoop,
                timestamp: Utc::now(),
            },
            author: SessionTranscriptAuthor::System,
            kind: SessionTranscriptEventKind::SystemMessage,
            payload: SessionTranscriptPayload::DynamicToolResponse {
                payload: SessionTranscriptDynamicToolResponsePayload {
                    call_id: call_id.to_string(),
                    tool_name: tool_name.to_string(),
                    output,
                    success,
                },
            },
            actions: SessionTranscriptActions::default(),
            partial: false,
        };
        memory_store.append_transcript_record(&record).await
    }

    pub(crate) async fn compact_session_impl(
        &self,
        session: TalosSession,
    ) -> TalosResult<TokenUsage> {
        let session_id = session.context.session_id.clone();
        self.ensure_session_is_idle(&session_id, "compact").await?;
        let (kernel_config, memory_store) = self.memory_store_for_session(&session).await?;
        let selected_model = if session.config.llm.model.trim().is_empty() {
            self.model.clone()
        } else {
            session.config.llm.model.clone()
        };
        let report = talos_agent_kernel::flush_session_memory_with_report(
            &session_id,
            &selected_model,
            self.llm_provider.clone(),
            memory_store.as_ref(),
            &kernel_config,
            talos_agent_kernel::MemoryFlushPolicy::for_compaction(
                kernel_config.memory.workspace_memory_flush_enabled
                    && kernel_config.memory.workspace_memory_flush_on_compaction,
            ),
        )
        .await?;
        Ok(TokenUsage {
            input_tokens: report.input_tokens,
            output_tokens: report.output_tokens,
            total_tokens: report.total_tokens,
            cache_creation_input_tokens: report.cache_creation_input_tokens,
            cache_read_input_tokens: report.cache_read_input_tokens,
        })
    }

    pub(crate) async fn clear_session_impl(&self, session: TalosSession) -> TalosResult<()> {
        let session_id = session.context.session_id.clone();
        self.ensure_session_is_idle(&session_id, "clear").await?;
        let (_kernel_config, memory_store) = self.memory_store_for_session(&session).await?;
        memory_store.reset_session(&session_id).await
    }

    pub(crate) async fn close_session_impl(&self, session: TalosSession) -> TalosResult<()> {
        let selected_model = if session.config.llm.model.trim().is_empty() {
            self.model.clone()
        } else {
            session.config.llm.model.clone()
        };
        let session_id = session.context.session_id.clone();
        let task_session_id = session_id.clone();
        let llm_provider = self.llm_provider.clone();
        let (kernel_config, memory_store) = self.memory_store_for_session(&session).await?;
        let accepted = self
            .memory_flush_manager
            .spawn_flush(&session_id, async move {
                talos_agent_kernel::flush_session_memory(
                    &task_session_id,
                    &selected_model,
                    llm_provider,
                    memory_store.as_ref(),
                    &kernel_config,
                    talos_agent_kernel::MemoryFlushPolicy::for_session_close(
                        kernel_config.memory.workspace_memory_flush_enabled,
                    ),
                )
                .await
            })
            .await;
        if !accepted {
            tracing::info!(session_id = %session_id, "memory flush deduplicated for session");
        }
        Ok(())
    }
}
