//! Product completion effects shared by live execution and durable recovery.

use super::*;

#[derive(Clone)]
pub(super) struct ProfileTurnContext {
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
    ) {
        let profile_store = self.profile_store.clone();
        let turn_service = self.clone();
        tokio::spawn(async move {
            let settlement = match completion.await {
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
            let (event, metrics) = match settlement {
                AgentRunSettlement::WaitingForInput { event, metrics, .. }
                | AgentRunSettlement::Finished { event, metrics, .. } => (event, metrics),
            };
            if let Err(error) = Self::record_profile_turn_usage(
                profile_store.as_ref(),
                &profile_context,
                &event,
                &metrics,
            )
            .await
            {
                turn_service
                    .execution
                    .report_warning(
                        &event.identity,
                        error.context(format!(
                            "record profile usage for session {} turn {}",
                            event.identity.session_id, event.identity.turn_id,
                        )),
                    )
                    .await;
            }
            if let Err(error) = turn_service.finish_turn(&event, auto_dispatch_ready).await {
                turn_service
                    .execution
                    .report_warning(
                        &event.identity,
                        error.context("finish committed Agent Turn"),
                    )
                    .await;
            }
        });
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
    ) -> TalosResult<()> {
        let profile_result = if matches!(event.payload, CanonicalAgentEventPayload::TurnFailed(_)) {
            self.profile_store
                .discard_turn_usage(&event.identity.session_id, &event.identity.turn_id)
                .await
        } else {
            Ok(())
        };
        if let Some(receiver) = auto_dispatch_ready {
            if receiver.await.is_err() {
                return profile_result;
            }
        }
        if let Some(session_id) = auto_dispatch_session_from_event(event) {
            if let Err(error) = self.try_dispatch_next(&session_id).await {
                // Independent completion effects must not prevent queue progress or lose causes.
                if let Err(profile_error) = profile_result {
                    self.execution
                        .report_warning(
                            &event.identity,
                            profile_error.context("discard failed Turn usage"),
                        )
                        .await;
                }
                return Err(error);
            }
        }
        profile_result
    }
}

fn profile_stop_reason(stop_reason: AgentStopReason) -> talos_types::talos::StopReason {
    match stop_reason {
        AgentStopReason::EndTurn => talos_types::talos::StopReason::EndTurn,
        AgentStopReason::OutputLimit => talos_types::talos::StopReason::ModelOutputLimit,
        AgentStopReason::TransferUnresolved => talos_types::talos::StopReason::TransferUnresolved,
    }
}
