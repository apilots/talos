//! Product completion effects shared by live execution and durable recovery.

use super::*;

#[derive(Clone)]
pub(super) struct ProfileTurnContext {
    execution_node_id: String,
    user_id: String,
    request_id: String,
    model_ref: ModelRef,
    timezone: String,
}

impl TurnService {
    pub(super) fn profile_turn_context(
        &self,
        session: &SessionRecord,
        request_id: String,
    ) -> ProfileTurnContext {
        ProfileTurnContext {
            execution_node_id: session.execution_node_id.clone(),
            user_id: session.user_id.clone(),
            request_id,
            model_ref: session
                .conf
                .model_selection
                .as_ref()
                .expect("loaded Session must have a model selection")
                .model_ref
                .clone(),
            timezone: self.base_config.agent.kernel.timezone.clone(),
        }
    }

    pub(super) fn spawn_turn_completion(
        &self,
        identity: AgentTurnRef,
        profile_context: ProfileTurnContext,
        auto_dispatch_ready: Option<oneshot::Receiver<()>>,
        completion: oneshot::Receiver<TalosResult<AgentRunSettlement>>,
        ownership: Option<talos_agent::SessionLease>,
    ) {
        let observation_stop = self
            .execution
            .observation_stop(&profile_context.execution_node_id);
        let turn_service = self.clone();
        self.execution.spawn_delivery(async move {
            let _ownership = ownership;
            let completed = tokio::select! {
                biased;
                _ = observation_stop.cancelled() => return,
                completed = completion => completed,
            };
            let settlement = match completed {
                Ok(Ok(settlement)) => settlement,
                Ok(Err(error)) => {
                    turn_service
                        .execution
                        .report_warning(&identity, error)
                        .await;
                    return;
                }
                Err(error) => {
                    turn_service
                        .execution
                        .report_warning(
                            &identity,
                            TalosError::internal_with_source(
                                "Agent Timeline writer stopped before completion",
                                error,
                            ),
                        )
                        .await;
                    return;
                }
            };
            if let AgentRunSettlement::Finished {
                result: Err(error), ..
            } = &settlement
            {
                talos_logging::report_error(error);
            }
            if let Some(receiver) = auto_dispatch_ready {
                let _ = receiver.await;
            }
            if let Err(error) = turn_service
                .recover_worker_settlements(&identity.session_id, true)
                .await
            {
                turn_service
                    .execution
                    .report_warning(
                        &identity,
                        error.context("apply committed Worker settlement effects"),
                    )
                    .await;
            }
        });
    }

    pub(crate) async fn reconcile_worker_events(&self, session_id: &str) -> TalosResult<()> {
        let Some(record) = self.execution.sessions.load_record(session_id).await? else {
            return Ok(());
        };
        self.execution
            .sessions
            .ensure_execution_supported(session_id)
            .await?;
        let context = self
            .execution
            .sessions
            .build_agent_session_context(&record)
            .await?;
        self.execution.project_worker(&context).await?;
        self.execution
            .coordinator
            .read_session_execution_state(context)
            .await?;
        if !self.execution.has_live_execution(session_id) {
            self.recover_worker_settlements(session_id, true).await?;
        }
        Ok(())
    }

    /// Replay product effects from the same durable receipt used by live completion.
    /// Callers finish cursor ingestion first and release live Session ownership before dispatch.
    pub(crate) async fn recover_worker_settlements(
        &self,
        session_id: &str,
        dispatch: bool,
    ) -> TalosResult<()> {
        let session = self
            .execution
            .sessions
            .load_record(session_id)
            .await?
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::SessionNotFound,
                    "Session is unavailable.",
                )
            })?;
        let latest_terminal = self
            .execution
            .execution_store
            .get_latest_agent_terminal_event(session_id)
            .await?;
        let idle = self
            .execution
            .execution_store
            .get_agent_active_turn(session_id)
            .await?
            .is_none();
        for fact in self
            .execution
            .execution_store
            .pending_worker_settlements(session_id)
            .await?
        {
            let event = talos_agent_api::AgentEvent {
                identity: fact.event.identity.clone(),
                sequence: fact.event.sequence,
                occurred_at: fact.event.occurred_at,
                payload: fact.event.payload.clone(),
            };
            let mut completion_notice = None;
            if fact.execution_purpose == agent_protocol::AgentExecutionPurpose::Compaction {
                self.record_worker_compaction_usage(&session, &fact).await?;
            } else {
                let metrics = match &fact.settlement {
                    agent_protocol::AgentExecutionSettlement::WaitingForInput {
                        metrics, ..
                    }
                    | agent_protocol::AgentExecutionSettlement::Finished { metrics, .. } => metrics,
                };
                let mut context = self.profile_turn_context(
                    &session,
                    format!(
                        "worker:{}:{}:{}",
                        fact.cursor.session_id, fact.cursor.generation, fact.cursor.offset
                    ),
                );
                context.model_ref = fact.execution_model.ok_or_else(|| {
                    TalosError::internal(
                        "Worker normal Turn is missing its admitted model selection.",
                    )
                })?;
                let profile_result = Self::record_profile_turn_usage(
                    self.profile_store.as_ref(),
                    &context,
                    &event,
                    metrics,
                )
                .await;
                let memory_result = self
                    .record_worker_memory_usage(&session, &fact.settlement)
                    .await;
                let title_result = self.generate_worker_title(&session, &event).await;
                let finish_result = self.finish_turn(&event, None, Some(metrics), false).await;
                let failed = profile_result.is_err()
                    || memory_result.is_err()
                    || title_result.is_err()
                    || finish_result.is_err();
                let finish_result = finish_result.map(|notice| completion_notice = notice);
                for result in [profile_result, memory_result, title_result, finish_result] {
                    if let Err(error) = result {
                        self.execution.report_warning(&event.identity, error).await;
                    }
                }
                let dispatch_current = dispatch
                    && !self.execution.delivery_is_stopping()
                    && idle
                    && latest_terminal
                        .as_ref()
                        .is_some_and(|latest| latest.event_seq == fact.event.event_seq);
                if dispatch_current {
                    if let Err(error) = self.dispatch_finished_turn(&event, None).await {
                        self.execution.report_warning(&event.identity, error).await;
                        continue;
                    }
                }
                if failed {
                    continue;
                }
            }
            if let Some(error) = completion_notice {
                let created = self
                    .execution
                    .complete_worker_settlement_with_notice(
                        &fact.cursor,
                        talos_agent_api::TimelineSystemNotice {
                            identity: event.identity.clone(),
                            occurred_at: event.occurred_at,
                            notice: talos_agent_api::AgentSystemNotice {
                                level: talos_types::TalosTimelineNoticeLevel::Warning,
                                content: talos_agent_api::SystemNoticeContent::Error(
                                    error.to_api_error(),
                                ),
                            },
                        },
                    )
                    .await?;
                if created {
                    talos_logging::report_error(&error);
                }
            } else {
                self.execution
                    .execution_store
                    .complete_worker_settlement(&fact.cursor)
                    .await?;
            }
        }
        Ok(())
    }

    async fn record_worker_compaction_usage(
        &self,
        session: &SessionRecord,
        fact: &talos_store::session::WorkerSettlementRecord,
    ) -> TalosResult<()> {
        let agent_protocol::AgentExecutionSettlement::Finished { metrics, .. } = &fact.settlement
        else {
            return Err(TalosError::internal(
                "Compaction cannot wait for user input.",
            ));
        };
        let completed_at = chrono::DateTime::from_timestamp(fact.event.occurred_at, 0)
            .ok_or_else(|| TalosError::internal("Worker compaction timestamp is invalid."))?;
        let timezone_name = self.execution.sessions.kernel_timezone();
        let timezone = talos_config::parse_timezone(&timezone_name)?;
        self.profile_store
            .record_usage(ProfileUsageUpdate::ModelCall(ModelCallUsageUpdate {
                user_id: session.user_id.clone(),
                session_id: Some(session.id.clone()),
                request_id: format!(
                    "worker-compact:{}:{}:{}",
                    fact.cursor.session_id, fact.cursor.generation, fact.cursor.offset
                ),
                kind: ModelUsageKind::Compact,
                model_ref: fact.execution_model.clone().ok_or_else(|| {
                    TalosError::internal("Worker compaction model selection is missing.")
                })?,
                usage: metrics.usage.clone(),
                usage_available: metrics.usage.is_available(),
                timezone: timezone_name,
                completed_at_ms: completed_at.timestamp_millis(),
                usage_date: completed_at
                    .with_timezone(&timezone)
                    .date_naive()
                    .format("%Y-%m-%d")
                    .to_string(),
            }))
            .await
    }

    async fn record_worker_memory_usage(
        &self,
        session: &SessionRecord,
        settlement: &agent_protocol::AgentExecutionSettlement,
    ) -> TalosResult<()> {
        let agent_protocol::AgentExecutionSettlement::Finished { memory_usage, .. } = settlement
        else {
            return Ok(());
        };
        for call in memory_usage {
            let completed_at_ms = call.completed_at.checked_mul(1_000).ok_or_else(|| {
                TalosError::internal("Worker Memory usage timestamp exceeds supported range.")
            })?;
            let timezone = talos_config::parse_timezone(&call.timezone)?;
            let date = chrono::DateTime::from_timestamp_millis(completed_at_ms)
                .ok_or_else(|| TalosError::internal("Worker Memory usage timestamp is invalid."))?
                .with_timezone(&timezone)
                .date_naive()
                .format("%Y-%m-%d")
                .to_string();
            self.profile_store
                .record_usage(ProfileUsageUpdate::ModelCall(ModelCallUsageUpdate {
                    user_id: session.user_id.clone(),
                    session_id: Some(session.id.clone()),
                    request_id: call.request_id.clone(),
                    kind: ModelUsageKind::Memory,
                    model_ref: call.model_ref.clone(),
                    usage: call.usage.clone(),
                    usage_available: call.usage.is_available(),
                    timezone: call.timezone.clone(),
                    completed_at_ms,
                    usage_date: date,
                }))
                .await?;
        }
        Ok(())
    }

    async fn record_profile_turn_usage(
        profile_store: &dyn ProfileStore,
        context: &ProfileTurnContext,
        event: &talos_agent_api::AgentEvent,
        metrics: &AgentRunMetrics,
    ) -> TalosResult<()> {
        let stop_reason = match &event.payload {
            CanonicalAgentEventPayload::WaitingForInput(_) => {
                talos_types::talos::StopReason::WaitingUserAction
            }
            CanonicalAgentEventPayload::TurnCompleted(completed) => {
                profile_stop_reason(completed.stop_reason)
            }
            _ => return Ok(()),
        };
        let completed_at_ms = event.occurred_at.checked_mul(1_000).ok_or_else(|| {
            TalosError::internal("Profile usage timestamp exceeds the supported range.")
                .context(event.occurred_at)
        })?;
        if metrics.usage == talos_types::TokenUsage::default()
            && matches!(
                event.payload,
                CanonicalAgentEventPayload::WaitingForInput(_)
            )
        {
            return Ok(());
        }
        let timezone = talos_config::parse_timezone(&context.timezone)?;
        let usage_date = chrono::DateTime::from_timestamp_millis(completed_at_ms)
            .ok_or_else(|| {
                TalosError::internal("Profile usage timestamp is invalid.").context(completed_at_ms)
            })?
            .with_timezone(&timezone)
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();
        profile_store
            .record_turn_usage(TurnUsageUpdate {
                user_id: context.user_id.clone(),
                session_id: event.identity.session_id.clone(),
                turn_id: event.identity.turn_id.clone(),
                request_id: context.request_id.clone(),
                model_ref: context.model_ref.clone(),
                usage: metrics.usage.clone(),
                usage_available: metrics.usage.is_available(),
                stop_reason,
                timezone: context.timezone.clone(),
                completed_at_ms,
                usage_date,
            })
            .await
    }

    /// Apply product effects to a committed normal or recovered terminal fact.
    /// The caller releases its live/maintenance ownership before queue dispatch.
    pub(super) async fn finish_turn(
        &self,
        event: &talos_agent_api::AgentEvent,
        auto_dispatch_ready: Option<oneshot::Receiver<()>>,
        automation_metrics: Option<&AgentRunMetrics>,
        dispatch: bool,
    ) -> TalosResult<Option<TalosError>> {
        let mut completion_notice = None;
        let mut automation_result = Ok(());
        if self.automation.get().is_some()
            || self
                .nodes
                .as_ref()
                .is_some_and(crate::ProductNodeAccess::managed_worker_enabled)
        {
            let result: TalosResult<()> = async {
                if let Some(session) = self
                    .execution
                    .sessions
                    .load_record(&event.identity.session_id)
                    .await?
                {
                    if let Some(nodes) = &self.nodes {
                        if let Err(error) = nodes
                            .record_managed_activity(&session.execution_node_id)
                            .await
                        {
                            self.execution.report_warning(&event.identity, error).await;
                        }
                    }
                    if let Some(automation) = self.automation.get() {
                        let outcome = automation.settle(&session, event, automation_metrics)
                            .await.map_err(|error| error.context("settle Session automation"))?;
                        if let crate::automation::goal_loop::AutomationSettlement::MissingRegistration(mode) = outcome {
                            completion_notice = Some(TalosError::new(
                                talos_error::ErrorCode::ToolExecutionFailed,
                                match mode {
                                    talos_types::AutomationMode::Goal => "No Goal was created in this Turn.",
                                    talos_types::AutomationMode::Loop => "No Loop was created in this Turn.",
                                },
                            ));
                        }
                    }
                }
                Ok(())
            }
            .await;
            automation_result = result;
        }
        let profile_result = if matches!(event.payload, CanonicalAgentEventPayload::TurnFailed(_)) {
            self.profile_store
                .discard_turn_usage(&event.identity.session_id, &event.identity.turn_id)
                .await
        } else {
            Ok(())
        };
        let profile_result = match (profile_result, automation_result) {
            (Err(profile), Err(automation)) => {
                self.execution
                    .report_warning(&event.identity, automation)
                    .await;
                Err(profile)
            }
            (Err(error), _) | (_, Err(error)) => Err(error),
            _ => Ok(()),
        };
        if !dispatch || self.execution.delivery_is_stopping() {
            return profile_result.map(|()| completion_notice);
        }
        self.dispatch_finished_turn(event, auto_dispatch_ready)
            .await?;
        profile_result.map(|()| completion_notice)
    }
    async fn dispatch_finished_turn(
        &self,
        event: &talos_agent_api::AgentEvent,
        auto_dispatch_ready: Option<oneshot::Receiver<()>>,
    ) -> TalosResult<()> {
        if let Some(receiver) = auto_dispatch_ready {
            if receiver.await.is_err() {
                return Ok(());
            }
        }
        if let Some(session_id) = auto_dispatch_session_from_event(event) {
            self.try_dispatch_next(&session_id).await?;
        }
        if auto_dispatch_session_from_event(event).is_some() {
            if let Some(session) = self
                .execution
                .sessions
                .load_record(&event.identity.session_id)
                .await?
            {
                self.continue_goal(session, false).await?;
            }
        }
        Ok(())
    }
}

fn profile_stop_reason(stop_reason: AgentStopReason) -> talos_types::talos::StopReason {
    match stop_reason {
        AgentStopReason::EndTurn => talos_types::talos::StopReason::EndTurn,
        AgentStopReason::OutputLimit => talos_types::talos::StopReason::ModelOutputLimit,
        AgentStopReason::TransferUnresolved => talos_types::talos::StopReason::TransferUnresolved,
    }
}
