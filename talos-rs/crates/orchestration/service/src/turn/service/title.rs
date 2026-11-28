//! Product consumption of the target Worker's retained title proposal and actual usage.
use super::*;

impl TurnService {
    pub(super) async fn generate_worker_title(
        &self,
        session: &SessionRecord,
        event: &talos_agent_api::AgentEvent,
    ) -> TalosResult<()> {
        let Some(session) = self.execution.sessions.load_record(&session.id).await? else {
            return Ok(());
        };
        if !matches!(
            event.payload,
            talos_agent_api::AgentEventPayload::TurnCompleted(_)
        ) {
            return Ok(());
        }
        let context = self
            .execution
            .sessions
            .build_agent_session_context(&session)
            .await?;
        let harness = self.execution.coordinator.session_harness(&context).await?;
        let result = harness
            .generate_session_title(agent_protocol::AgentSessionTitleParams {
                mode: if session.title == talos_types::DEFAULT_SESSION_TITLE {
                    agent_protocol::AgentSessionTitleMode::Generate
                } else {
                    agent_protocol::AgentSessionTitleMode::ReadRetained
                },
                session: context,
                identity: event.identity.clone(),
            })
            .await;
        let result = match result {
            Ok(Some(result)) => result,
            Ok(None) => return Ok(()),
            Err(error)
                if matches!(
                    error.error_code(),
                    talos_error::ErrorCode::SessionNotFound
                        | talos_error::ErrorCode::PermissionDenied
                        | talos_error::ErrorCode::InvalidRequest
                ) =>
            {
                self.execution.report_warning(&event.identity, error).await;
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        if let Some(call) = result.model_usage {
            let completed_at_ms = call.completed_at.checked_mul(1_000).ok_or_else(|| {
                TalosError::internal("Worker title usage timestamp exceeds supported range.")
            })?;
            let timezone = talos_config::parse_timezone(&call.timezone)?;
            let usage_date = chrono::DateTime::from_timestamp_millis(completed_at_ms)
                .ok_or_else(|| TalosError::internal("Worker title usage timestamp is invalid."))?
                .with_timezone(&timezone)
                .date_naive()
                .format("%Y-%m-%d")
                .to_string();
            self.profile_store
                .record_usage(ProfileUsageUpdate::ModelCall(ModelCallUsageUpdate {
                    user_id: session.user_id.clone(),
                    session_id: Some(session.id.clone()),
                    request_id: call.request_id,
                    kind: ModelUsageKind::Title,
                    model_ref: call.model_ref,
                    usage_available: call.usage.is_available(),
                    usage: call.usage,
                    timezone: call.timezone,
                    completed_at_ms,
                    usage_date,
                }))
                .await?;
        }
        match result.outcome {
            agent_protocol::AgentSessionTitleOutcome::Generated { title } => {
                self.execution
                    .sessions
                    .set_generated_title(&event.identity, &title)
                    .await?;
            }
            agent_protocol::AgentSessionTitleOutcome::Failed { error } => {
                self.execution.publish_warning(&event.identity, error).await
            }
        }
        Ok(())
    }
}
