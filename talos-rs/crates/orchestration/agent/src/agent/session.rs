use super::{ActiveTurnHandle, AgentImpl};
use chrono::Utc;
use std::time::Duration;
use talos_error::{TalosError, TalosResult};
use talos_memory::Memory;
use talos_memory::{
    SessionTranscriptActions, SessionTranscriptApprovalResponsePayload, SessionTranscriptAuthor,
    SessionTranscriptBranch, SessionTranscriptDynamicToolResponsePayload,
    SessionTranscriptEventKind, SessionTranscriptHeader, SessionTranscriptPayload,
    SessionTranscriptRecord,
};
use talos_types::{talos::TalosUsage, TalosSession};
use uuid::Uuid;

impl AgentImpl {
    fn ensure_session_is_idle(&self, session_id: &str, action: &str) -> TalosResult<()> {
        if self.active_turns.contains_key(session_id) {
            return Err(TalosError::ValidationError(format!(
                "cannot {} session `{}` while a turn is active",
                action, session_id
            )));
        }
        Ok(())
    }

    pub(crate) fn register_active_turn(&self, session_id: &str, handle: ActiveTurnHandle) {
        if let Some(previous) = self.active_turns.insert(session_id.to_string(), handle) {
            previous.execution_task.abort();
        }
    }

    pub(crate) fn clear_active_turn(&self, session_id: &str) {
        self.active_turns.remove(session_id);
    }

    pub(crate) async fn drain_pending_memory_flushes(&self, timeout: Duration) -> TalosResult<()> {
        self.memory_flush_manager.drain(timeout).await
    }

    pub(crate) fn pending_memory_flush_count(&self) -> usize {
        self.memory_flush_manager.pending_count()
    }

    pub(crate) async fn validate_dynamic_tool_submission(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
    ) -> TalosResult<()> {
        let records = self
            .memory_store
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

        let timeout_ms = self.kernel_config.tools.dynamic.response_timeout_ms;
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

    pub(crate) async fn submit_tool_approval(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        approved: bool,
        reason: Option<String>,
    ) -> TalosResult<()> {
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
            payload: SessionTranscriptPayload::ApprovalResponse {
                payload: SessionTranscriptApprovalResponsePayload {
                    call_id: call_id.to_string(),
                    tool_name: tool_name.to_string(),
                    approved,
                    reason,
                },
            },
            actions: SessionTranscriptActions::default(),
            partial: false,
        };
        self.memory_store.append_transcript_record(&record).await
    }

    pub(crate) async fn submit_dynamic_tool_result(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        self.validate_dynamic_tool_submission(session_id, call_id, tool_name)
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
        self.memory_store.append_transcript_record(&record).await
    }

    pub(crate) async fn compact_session_impl(
        &self,
        session: TalosSession,
    ) -> TalosResult<TalosUsage> {
        let session_id = session.context.session_id.clone();
        self.ensure_session_is_idle(&session_id, "compact")?;
        let selected_model = if session.config.llm.model.trim().is_empty() {
            self.model.clone()
        } else {
            session.config.llm.model.clone()
        };
        let report = talos_agent_kernel::flush_session_memory_with_report(
            &session_id,
            &selected_model,
            self.llm_provider.clone(),
            self.memory_store.as_ref(),
            &self.kernel_config,
            talos_agent_kernel::MemoryFlushPolicy::for_compaction(
                self.kernel_config.memory.workspace_memory_flush_enabled
                    && self
                        .kernel_config
                        .memory
                        .workspace_memory_flush_on_compaction,
            ),
        )
        .await?;
        Ok(TalosUsage {
            input_tokens: report.input_tokens,
            output_tokens: report.output_tokens,
            total_tokens: report.total_tokens,
            cache_creation_input_tokens: report.cache_creation_input_tokens,
            cache_read_input_tokens: report.cache_read_input_tokens,
        })
    }

    pub(crate) async fn clear_session_impl(&self, session: TalosSession) -> TalosResult<()> {
        let session_id = session.context.session_id.clone();
        self.ensure_session_is_idle(&session_id, "clear")?;
        self.memory_store.reset_session(&session_id).await
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
        let memory_store = self.memory_store.clone();
        let kernel_config = self.kernel_config.clone();
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
