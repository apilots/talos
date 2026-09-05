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
                    error!(error = %error, "Agent execution did not commit its settlement");
                    return;
                }
                Err(error) => {
                    error!(error = %error, "Agent Timeline writer stopped before completion");
                    return;
                }
            };
            let (event, metrics) = match settlement {
                AgentRunSettlement::WaitingForInput { event, metrics, .. }
                | AgentRunSettlement::Finished { event, metrics, .. } => (event, metrics),
            };
            Self::record_profile_turn_usage(
                profile_store.as_ref(),
                &profile_context,
                &event,
                &metrics,
            )
            .await;
            if let Err(error) = turn_service.finish_turn(&event, auto_dispatch_ready).await {
                error!(error = %error, "failed to finish committed Agent Turn");
            }
        });
    }

    async fn record_profile_turn_usage(
        profile_store: &dyn ProfileStore,
        context: &ProfileTurnContext,
        event: &talos_agent_api::AgentEvent,
        metrics: &AgentRunMetrics,
    ) {
        let stop_reason = match &event.payload {
            CanonicalAgentEventPayload::WaitingForInput(_) => {
                talos_types::talos::StopReason::WaitingUserAction
            }
            CanonicalAgentEventPayload::TurnCompleted(completed) => {
                profile_stop_reason(completed.stop_reason)
            }
            _ => return,
        };
        let completed_at_ms = match event.occurred_at.checked_mul(1_000) {
            Some(value) => value,
            None => {
                error!(
                    session_id = %event.identity.session_id,
                    turn_id = %event.identity.turn_id,
                    occurred_at = event.occurred_at,
                    "profile usage timestamp exceeds supported range"
                );
                return;
            }
        };
        if metrics.usage == talos_types::TokenUsage::default()
            && matches!(
                event.payload,
                CanonicalAgentEventPayload::WaitingForInput(_)
            )
        {
            return;
        }
        let timezone = match talos_config::parse_timezone(&context.timezone) {
            Ok(timezone) => timezone,
            Err(error) => {
                error!(
                    session_id = %event.identity.session_id,
                    turn_id = %event.identity.turn_id,
                    error = %error,
                    "profile usage timezone is invalid"
                );
                return;
            }
        };
        let usage_date = match chrono::DateTime::from_timestamp_millis(completed_at_ms) {
            Some(timestamp) => timestamp
                .with_timezone(&timezone)
                .date_naive()
                .format("%Y-%m-%d")
                .to_string(),
            None => {
                error!(
                    session_id = %event.identity.session_id,
                    turn_id = %event.identity.turn_id,
                    completed_at_ms,
                    "profile usage timestamp is invalid"
                );
                return;
            }
        };
        if let Err(error) = profile_store
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
        {
            error!(
                session_id = %event.identity.session_id,
                turn_id = %event.identity.turn_id,
                error = %error,
                "failed to persist profile turn usage"
            );
        }
    }

    /// Apply product effects to a committed normal or recovered terminal fact.
    /// The caller releases its live/maintenance ownership before queue dispatch.
    pub(super) async fn finish_turn(
        &self,
        event: &talos_agent_api::AgentEvent,
        auto_dispatch_ready: Option<oneshot::Receiver<()>>,
    ) -> TalosResult<()> {
        if matches!(event.payload, CanonicalAgentEventPayload::TurnFailed(_)) {
            self.profile_store
                .discard_turn_usage(&event.identity.session_id, &event.identity.turn_id)
                .await?;
        }
        if let Some(receiver) = auto_dispatch_ready {
            if receiver.await.is_err() {
                return Ok(());
            }
        }
        if let Some(session_id) = auto_dispatch_session_from_event(event) {
            self.try_dispatch_next(&session_id).await?;
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
