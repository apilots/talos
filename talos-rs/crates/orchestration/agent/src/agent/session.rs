use super::lifecycle::AgentRuntimeAbortHandle;
use super::AgentHarnessImpl;
use crate::AgentSessionCompactionParams;
use chrono::Utc;
use talos_agent_kernel::{SessionCompactionState, TimelineBuilder, SESSION_COMPACTION_STATE_KEY};
use talos_config::AgentKernelConfig;
use talos_error::{TalosError, TalosResult};
use talos_store::session::{
    SessionTranscriptActions, SessionTranscriptAuthor, SessionTranscriptBranch,
    SessionTranscriptEventKind, SessionTranscriptHeader, SessionTranscriptPayload,
    SessionTranscriptRecord, SessionTranscriptStateDelta, SessionTranscriptStateNamespace,
    SessionTranscriptUserActionResponsePayload,
};
use talos_types::talos::{
    AgentEvent, AgentEventPayload, ContextCompactionMetrics, ContextCompactionOutcome,
    ContextCompactionStage, ContextCompactionTrigger, StopReason, TalosTimelineContent,
    TalosTimelineDelta, TalosTimelineItemStatus, TokenUsage,
};
use talos_types::TalosSession;
use uuid::Uuid;

impl AgentHarnessImpl {
    fn session_model(session: &TalosSession) -> TalosResult<String> {
        let model = session.config.llm.model.trim();
        if model.is_empty() {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "session llm model is required".to_string(),
            ));
        }
        Ok(model.to_string())
    }

    pub(crate) async fn validate_user_action_submission(
        &self,
        kernel_config: &AgentKernelConfig,
        session_id: &str,
        turn_id: &str,
        call_id: &str,
        tool_name: &str,
        response: &talos_types::UserActionResponse,
    ) -> TalosResult<()> {
        let records = self
            .session_artifact_store
            .load_transcript_records(
                std::path::Path::new(&kernel_config.workspace_dir),
                session_id,
                None,
                Some(SessionTranscriptBranch::ReactiveLoop),
            )
            .await?;

        let mut latest_request_timestamp = None;
        let mut latest_request_tool_name = None;
        let mut latest_request_params = None;
        let mut latest_response_timestamp = None;

        for record in records {
            match record.payload {
                SessionTranscriptPayload::UserActionRequest { payload }
                    if record.header.invocation_id == turn_id && payload.call_id == call_id =>
                {
                    latest_request_timestamp = Some(record.header.timestamp);
                    latest_request_tool_name = Some(payload.tool_name);
                    latest_request_params = Some(payload.params);
                }
                SessionTranscriptPayload::UserActionResponse { payload }
                    if record.header.invocation_id == turn_id && payload.call_id == call_id =>
                {
                    latest_response_timestamp = Some(record.header.timestamp);
                }
                _ => {}
            }
        }

        let Some(request_timestamp) = latest_request_timestamp else {
            return Err(talos_error::TalosError::public(
                talos_error::ErrorCode::Conflict,
                format!(
                "user action call `{call_id}` is not pending for session `{session_id}` turn `{turn_id}`"
                ),
            ));
        };

        if latest_request_tool_name.as_deref() != Some(tool_name) {
            return Err(talos_error::TalosError::public(
                talos_error::ErrorCode::Conflict,
                format!(
                    "user action call `{call_id}` belongs to `{}` not `{tool_name}`",
                    latest_request_tool_name.unwrap_or_else(|| "unknown".to_string())
                ),
            ));
        }

        if latest_response_timestamp.is_some_and(|ts| ts > request_timestamp) {
            return Err(talos_error::TalosError::public(
                talos_error::ErrorCode::Conflict,
                format!("user action call `{call_id}` has already been resolved"),
            ));
        }

        let params = latest_request_params.ok_or_else(|| {
            TalosError::internal(format!(
                "user action call `{call_id}` is missing typed parameters"
            ))
        })?;
        let variants_match = matches!(
            (&params, response),
            (
                talos_types::UserActionParams::DynamicToolResult(_),
                talos_types::UserActionResponse::DynamicToolResult(_)
            ) | (
                talos_types::UserActionParams::Question(_),
                talos_types::UserActionResponse::Question(_)
            ) | (
                talos_types::UserActionParams::SandboxApproval(_),
                talos_types::UserActionResponse::SandboxApproval(_)
            )
        );
        if !variants_match {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!("user action call `{call_id}` received a mismatched response"),
            ));
        }

        let timeout_ms = kernel_config.tools.dynamic.response_timeout_ms;
        if matches!(params, talos_types::UserActionParams::DynamicToolResult(_)) && timeout_ms > 0 {
            let elapsed_ms = Utc::now()
                .signed_duration_since(request_timestamp)
                .num_milliseconds()
                .max(0) as u64;
            if elapsed_ms >= timeout_ms {
                return Err(talos_error::TalosError::public(
                    talos_error::ErrorCode::Timeout,
                    format!("dynamic tool call `{call_id}` timed out after {timeout_ms} ms"),
                ));
            }
        }

        Ok(())
    }

    pub(crate) async fn submit_user_action_response(
        &self,
        session: TalosSession,
        turn_id: &str,
        call_id: &str,
        tool_name: &str,
        response: talos_types::UserActionResponse,
    ) -> TalosResult<()> {
        let session_id = session.context.session_id.clone();
        let kernel_config = self.runtime_kernel_config_for_session(&session)?;
        self.validate_user_action_submission(
            &kernel_config,
            &session_id,
            turn_id,
            call_id,
            tool_name,
            &response,
        )
        .await?;
        let record = SessionTranscriptRecord {
            header: SessionTranscriptHeader {
                id: Uuid::new_v4().to_string(),
                session_id: session_id.to_string(),
                invocation_id: turn_id.to_string(),
                branch: SessionTranscriptBranch::ReactiveLoop,
                timestamp: Utc::now(),
            },
            author: SessionTranscriptAuthor::System,
            kind: SessionTranscriptEventKind::SystemMessage,
            payload: SessionTranscriptPayload::UserActionResponse {
                payload: SessionTranscriptUserActionResponsePayload {
                    call_id: call_id.to_string(),
                    tool_name: tool_name.to_string(),
                    response,
                },
            },
            actions: SessionTranscriptActions::default(),
            partial: false,
        };
        self.session_artifact_store
            .append_transcript_record(std::path::Path::new(&kernel_config.workspace_dir), &record)
            .await
    }

    pub(crate) async fn compact_session_impl(
        &self,
        params: AgentSessionCompactionParams,
    ) -> TalosResult<TokenUsage> {
        let AgentSessionCompactionParams {
            session,
            identity,
            event_tx,
        } = params;
        let session_id = session.context.session_id.clone();
        self.try_reserve_active_turn(&session_id)?;
        let result = async {
            let kernel_config = self.runtime_kernel_config_for_session(&session)?;
            let selected_model = Self::session_model(&session)?;
            let llm_provider = self.llm_provider_for_session(&session)?;
            let memory_scope = talos_memory::MemoryScope {
                user_id: session.user_id.clone(),
                workspace_root: std::path::PathBuf::from(&kernel_config.workspace_dir),
            };
            let this = self.clone();
            let task_session_id = session_id.clone();
            let task_turn_id = identity.turn_id.clone();
            let workspace_root = std::path::PathBuf::from(&kernel_config.workspace_dir);
            let (start_tx, start_rx) = tokio::sync::oneshot::channel();
            let task = tokio::spawn(async move {
                start_rx.await.map_err(|_| {
                    TalosError::public(
                        talos_error::ErrorCode::Cancelled,
                        "session context compaction was cancelled before start".to_string(),
                    )
                })?;
                let report = talos_agent_kernel::compact_session_memory(
                    talos_agent_kernel::MemoryFormationParams {
                        session_id: &task_session_id,
                        selected_model: &selected_model,
                        llm_provider,
                        memory_writer: this.memory_writer.as_ref(),
                        memory_scope: &memory_scope,
                        session_artifact_store: this.session_artifact_store.as_ref(),
                        session_store: this.session_store.as_ref(),
                        kernel_config: &kernel_config,
                    },
                    talos_agent_kernel::MemoryCompactionPolicy::for_compaction(),
                )
                .await?;
                if report.covers_until_seq.is_some() {
                    let mut actions = SessionTranscriptActions::default();
                    actions.state_delta.push(SessionTranscriptStateDelta {
                        namespace: SessionTranscriptStateNamespace::Session,
                        key: SESSION_COMPACTION_STATE_KEY.to_string(),
                        value: serde_json::to_value(SessionCompactionState::recovery())
                            .map_err(|error| {
                                TalosError::internal(format!(
                                    "serialize session compaction recovery state: {error}"
                                ))
                            })?
                            .into(),
                    });
                    this.session_artifact_store
                        .append_transcript_record(
                            &workspace_root,
                            &SessionTranscriptRecord {
                                header: SessionTranscriptHeader {
                                    id: Uuid::new_v4().to_string(),
                                    session_id: task_session_id.clone(),
                                    invocation_id: task_turn_id,
                                    branch: SessionTranscriptBranch::ModelThinking,
                                    timestamp: Utc::now(),
                                },
                                author: SessionTranscriptAuthor::System,
                                kind: SessionTranscriptEventKind::Planning,
                                payload: SessionTranscriptPayload::StateOnly,
                                actions,
                                partial: false,
                            },
                        )
                        .await?;
                }
                Ok(report)
            });
            let abort_handle = AgentRuntimeAbortHandle::new(task.abort_handle());
            if let Err(error) = self.activate_reserved_turn(
                &session_id,
                identity.turn_id.clone(),
                abort_handle,
                None,
            ) {
                task.abort();
                return Err(error);
            }

            let mut timeline = TimelineBuilder::new(
                identity.request_id,
                identity.turn_id,
                identity.message_id,
                session_id.clone(),
            );
            let item_id = match async {
                send_compaction_event(&event_tx, timeline.start_turn()).await?;
                let started = timeline.start_context_compaction(
                    ContextCompactionTrigger::UserRequested,
                    ContextCompactionStage::Preparing,
                    Some("User-requested context compaction started.".to_string()),
                );
                let item_id = match &started.payload {
                    AgentEventPayload::ItemStarted(payload) => payload.item.item_id.clone(),
                    _ => {
                        return Err(TalosError::internal(
                            "context compaction did not create a timeline item".to_string(),
                        ));
                    }
                };
                send_compaction_event(&event_tx, started).await?;
                send_compaction_event(
                    &event_tx,
                    timeline.append_delta(
                        &item_id,
                        TalosTimelineDelta::ContextCompaction {
                            stage: Some(ContextCompactionStage::Summarizing),
                            outcome: None,
                            metrics: None,
                            message: Some("Summarizing session context.".to_string()),
                        },
                    )?,
                )
                .await?;
                Ok(item_id)
            }
            .await
            {
                Ok(item_id) => item_id,
                Err(error) => {
                    drop(start_tx);
                    task.abort();
                    let _ = task.await;
                    return Err(error);
                }
            };
            let _ = start_tx.send(());
            let report = match task.await {
                Ok(report) => report,
                Err(error) if error.is_cancelled() => {
                    let cancelled_content = TalosTimelineContent::ContextCompaction {
                        trigger: ContextCompactionTrigger::UserRequested,
                        stage: ContextCompactionStage::Failed,
                        outcome: None,
                        metrics: None,
                        message: Some("Session context compaction was cancelled.".to_string()),
                    };
                    send_compaction_event(
                        &event_tx,
                        timeline.complete_item(
                            &item_id,
                            TalosTimelineItemStatus::Cancelled,
                            Some(cancelled_content),
                        )?,
                    )
                    .await?;
                    send_compaction_event(
                        &event_tx,
                        timeline.complete_turn(
                            StopReason::Cancelled,
                            TokenUsage::default(),
                            None,
                            None,
                        ),
                    )
                    .await?;
                    return Err(TalosError::public(
                        talos_error::ErrorCode::Cancelled,
                        "session context compaction was cancelled".to_string(),
                    ));
                }
                Err(error) => Err(TalosError::internal(format!(
                    "session context compaction task failed: {error}"
                ))),
            };
            let report = match report {
                Ok(report) => report,
                Err(error) => {
                    let outcome = match error.error_code() {
                        talos_error::ErrorCode::ServiceUnavailable
                        | talos_error::ErrorCode::ProviderUnavailable
                        | talos_error::ErrorCode::Timeout
                        | talos_error::ErrorCode::ToolExecutionFailed => {
                            ContextCompactionOutcome::FailedProviderError
                        }
                        talos_error::ErrorCode::InvalidRequest
                        | talos_error::ErrorCode::InvalidRetryRequest
                        | talos_error::ErrorCode::Unauthenticated
                        | talos_error::ErrorCode::PermissionDenied
                        | talos_error::ErrorCode::ResourceNotFound
                        | talos_error::ErrorCode::SessionNotFound
                        | talos_error::ErrorCode::ResourceExpired
                        | talos_error::ErrorCode::Conflict
                        | talos_error::ErrorCode::ActiveTurnRunning
                        | talos_error::ErrorCode::RetrySourceCompleted
                        | talos_error::ErrorCode::RetrySourceMismatch
                        | talos_error::ErrorCode::IdempotencyConflict
                        | talos_error::ErrorCode::InvalidCursor
                        | talos_error::ErrorCode::EventCursorExpired
                        | talos_error::ErrorCode::FsDisabled
                        | talos_error::ErrorCode::InvalidPath
                        | talos_error::ErrorCode::PathOutsideRoot
                        | talos_error::ErrorCode::SymlinkNotSupported
                        | talos_error::ErrorCode::PathNotFound
                        | talos_error::ErrorCode::NotDirectory
                        | talos_error::ErrorCode::InvalidName
                        | talos_error::ErrorCode::AlreadyExists
                        | talos_error::ErrorCode::UploadTooLarge
                        | talos_error::ErrorCode::UnsupportedMediaType
                        | talos_error::ErrorCode::AttachmentLimitExceeded
                        | talos_error::ErrorCode::UnsupportedAttachment
                        | talos_error::ErrorCode::FileInputRejected
                        | talos_error::ErrorCode::ContextWindowExceeded
                        | talos_error::ErrorCode::SessionBudgetExceeded
                        | talos_error::ErrorCode::UsageLimitExceeded
                        | talos_error::ErrorCode::ContextCompactionFailed
                        | talos_error::ErrorCode::Cancelled
                        | talos_error::ErrorCode::InternalError => {
                            ContextCompactionOutcome::FailedInternalError
                        }
                    };
                    let message = error.to_string();
                    let failed_content = TalosTimelineContent::ContextCompaction {
                        trigger: ContextCompactionTrigger::UserRequested,
                        stage: ContextCompactionStage::Failed,
                        outcome: Some(outcome),
                        metrics: None,
                        message: Some(message.clone()),
                    };
                    send_compaction_event(
                        &event_tx,
                        timeline.append_delta(
                            &item_id,
                            TalosTimelineDelta::ContextCompaction {
                                stage: Some(ContextCompactionStage::Failed),
                                outcome: Some(outcome),
                                metrics: None,
                                message: Some(message),
                            },
                        )?,
                    )
                    .await?;
                    send_compaction_event(
                        &event_tx,
                        timeline.complete_item(
                            &item_id,
                            TalosTimelineItemStatus::Failed,
                            Some(failed_content),
                        )?,
                    )
                    .await?;
                    send_compaction_event(&event_tx, timeline.fail_turn(&error)).await?;
                    return Err(error);
                }
            };
            let usage = TokenUsage {
                input_tokens: report.input_tokens,
                output_tokens: report.output_tokens,
                total_tokens: report.total_tokens,
                cache_creation_input_tokens: report.cache_creation_input_tokens,
                cache_read_input_tokens: report.cache_read_input_tokens,
            };
            let (outcome, message) = if report.covers_until_seq.is_some() {
                (
                    ContextCompactionOutcome::Applied,
                    "Session context compaction completed.",
                )
            } else {
                (
                    ContextCompactionOutcome::NoChanges,
                    "Session context compaction found no new content.",
                )
            };
            let metrics = ContextCompactionMetrics {
                covers_from_seq: report.covers_from_seq,
                covers_until_seq: report.covers_until_seq,
                compaction_input_tokens: report.input_tokens,
                compaction_output_tokens: report.output_tokens,
                trimmed_context_unit_count: report.trimmed_context_unit_count,
                ..ContextCompactionMetrics::default()
            };
            let completed_content = TalosTimelineContent::ContextCompaction {
                trigger: ContextCompactionTrigger::UserRequested,
                stage: ContextCompactionStage::Completed,
                outcome: Some(outcome),
                metrics: Some(metrics.clone()),
                message: Some(message.to_string()),
            };
            send_compaction_event(
                &event_tx,
                timeline.append_delta(
                    &item_id,
                    TalosTimelineDelta::ContextCompaction {
                        stage: Some(ContextCompactionStage::Completed),
                        outcome: Some(outcome),
                        metrics: Some(metrics),
                        message: Some(message.to_string()),
                    },
                )?,
            )
            .await?;
            send_compaction_event(
                &event_tx,
                timeline.complete_item(
                    &item_id,
                    TalosTimelineItemStatus::Completed,
                    Some(completed_content),
                )?,
            )
            .await?;
            send_compaction_event(
                &event_tx,
                timeline.complete_turn(StopReason::EndTurn, usage.clone(), None, None),
            )
            .await?;
            Ok(usage)
        }
        .await;
        self.finish_active_turn_and_dispatch_next(&session_id).await;
        result
    }

    pub(crate) async fn clear_session_impl(&self, session: TalosSession) -> TalosResult<()> {
        let session_id = session.context.session_id.clone();
        self.try_reserve_active_turn(&session_id)?;
        let result = async {
            let kernel_config = self.runtime_kernel_config_for_session(&session)?;
            let workspace_root = std::path::Path::new(&kernel_config.workspace_dir);
            let latest_seq = self
                .session_artifact_store
                .latest_seq(workspace_root, &session_id)
                .await?;
            let session_record = self
                .session_store
                .get_session_record(&session_id)
                .await?
                .ok_or_else(|| {
                    TalosError::public(talos_error::ErrorCode::ResourceNotFound, session_id.clone())
                })?;
            if latest_seq > session_record.memory_processed_until_seq {
                self.session_store
                    .advance_memory_processed_until_seq(
                        &session_id,
                        session_record.memory_processed_until_seq,
                        latest_seq,
                    )
                    .await?;
            }
            self.session_artifact_store
                .reset_session(workspace_root, &session_id)
                .await
        }
        .await;
        self.finish_active_turn_and_dispatch_next(&session_id).await;
        result
    }

    pub(crate) async fn close_session_impl(&self, session: TalosSession) -> TalosResult<()> {
        let session_id = session.context.session_id;
        self.try_reserve_active_turn(&session_id)?;
        self.finish_active_turn_and_dispatch_next(&session_id).await;
        Ok(())
    }
}

async fn send_compaction_event(
    event_tx: &tokio::sync::mpsc::Sender<AgentEvent>,
    event: AgentEvent,
) -> TalosResult<()> {
    event_tx.send(event).await.map_err(|_| {
        TalosError::internal("session compaction event receiver was closed".to_string())
    })
}
