//! One product ingestion path for both live delivery and cursor recovery.
use super::AgentExecutionDriver;
use agent_protocol::{AgentExecutionSettlement, AgentSessionContext, AgentSessionEventsParams};
use talos_agent_api::{AgentEvent, AgentEventPayload, AgentTurnRef};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_store::session::WorkerProjectionParams;

impl AgentExecutionDriver {
    pub(crate) async fn project_worker(&self, session: &AgentSessionContext) -> TalosResult<()> {
        let record = self
            .sessions
            .load_record(&session.session_id)
            .await?
            .ok_or_else(|| {
                TalosError::new(ErrorCode::SessionNotFound, "Session is unavailable.")
            })?;
        loop {
            let after = self
                .execution_store
                .worker_event_cursor(&session.session_id)
                .await?;
            let page = self
                .coordinator
                .read_session_events(AgentSessionEventsParams {
                    session: session.clone(),
                    after_cursor: after.clone(),
                    limit: agent_protocol::MAX_EXECUTION_PAGE_EVENTS,
                })
                .await?;
            let more = page.has_more;
            let commit = self
                .execution_store
                .project_worker_events(WorkerProjectionParams {
                    execution_node_id: record.execution_node_id.clone(),
                    session_id: session.session_id.clone(),
                    after_cursor: after,
                    page,
                })
                .await?;
            let changed = !commit.events.is_empty();
            let identity = commit.events.last().map(|event| event.identity.clone());
            if let Some(queue) = &self.message_queue {
                for event in &commit.events {
                    if let AgentEventPayload::MessageCompleted(message) = &event.payload {
                        if message.role == talos_agent_api::AgentMessageRole::User {
                            queue
                                .acknowledge_admission(&session.session_id, &message.message_id)?;
                        }
                    }
                }
            }
            self.event_service.publish_committed(commit.events);
            if changed {
                if let Err(error) = self
                    .event_service
                    .publish_active_turn(&session.session_id)
                    .await
                {
                    if let Some(identity) = identity {
                        self.report_warning(&identity, error).await;
                    } else {
                        return Err(error);
                    }
                }
            }
            if !more {
                return Ok(());
            }
        }
    }

    pub(super) async fn recovered_worker_settlement(
        &self,
        identity: &AgentTurnRef,
        original: &TalosError,
    ) -> TalosResult<agent_protocol::AgentRunSettlement> {
        let record = self
            .sessions
            .load_record(&identity.session_id)
            .await?
            .ok_or_else(|| {
                TalosError::new(ErrorCode::SessionNotFound, "Session is unavailable.")
            })?;
        let session = self.sessions.build_agent_session_context(&record).await?;
        for _ in 0..50 {
            self.project_worker(&session).await?;
            for fact in self
                .execution_store
                .pending_worker_settlements(&identity.session_id)
                .await?
                .into_iter()
                .rev()
            {
                if fact.event.identity != *identity {
                    continue;
                }
                let event = AgentEvent {
                    identity: fact.event.identity,
                    sequence: fact.event.sequence,
                    occurred_at: fact.event.occurred_at,
                    payload: fact.event.payload,
                };
                return Ok(match fact.settlement {
                    AgentExecutionSettlement::WaitingForInput { metrics } => {
                        agent_protocol::AgentRunSettlement::WaitingForInput { event, metrics }
                    }
                    AgentExecutionSettlement::Finished { metrics, .. } => {
                        let result = match &event.payload {
                            AgentEventPayload::TurnFailed(failed) => {
                                Err(failed.error.clone().into())
                            }
                            _ => Ok(()),
                        };
                        agent_protocol::AgentRunSettlement::Finished {
                            event,
                            metrics,
                            result,
                        }
                    }
                });
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        Err(original
            .clone()
            .context("Worker execution has not yet published a settlement"))
    }
}
