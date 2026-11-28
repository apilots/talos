//! Product Timeline reads and cursor materialization share the original Service.
use super::TalosService;
use crate::timeline::TimelineEventId;
use crate::timeline::TimelineSubscribeCommand;
use crate::{Actor, ServiceResult, TimelineSubscription};
use talos_types::{RecoverTimelineRequest, SubscribeTimelineRequest};

impl TalosService {
    pub async fn subscribe_timeline(
        &self,
        actor: &Actor,
        request: SubscribeTimelineRequest,
    ) -> ServiceResult<talos_types::SubscribeTimelineResponse> {
        let subscription = self.open_timeline_subscription(actor, request).await?;
        Ok(subscription.response())
    }

    pub async fn open_timeline_subscription(
        &self,
        actor: &Actor,
        request: SubscribeTimelineRequest,
    ) -> ServiceResult<TimelineSubscription> {
        let session = self.access.load_session(actor, &request.session_id).await?;
        if self
            .sessions
            .supports_worker_execution(&request.session_id)
            .await?
        {
            self.prepare_worker_timeline_read(&session).await?;
        }
        let after_event_id = parse_timeline_event_id(request.since_event_id)?;
        let command = TimelineSubscribeCommand {
            session_id: request.session_id,
            turn_id: request.turn_id.clone(),
            after_event_id,
            replay_limit: request.replay_limit.unwrap_or(1).max(1),
            close_when_idle: request.close_when_idle.unwrap_or(false),
            close_on_terminal: request.close_on_terminal.unwrap_or(false),
        };
        let subscription = if request.turn_id.is_none() {
            self.events
                .subscribe_with_changes(
                    command,
                    self.message_queue.subscribe_changed(),
                    self.turns
                        .installed_automation()
                        .map(|automation| automation.subscribe_changed()),
                )
                .await?
        } else {
            self.events.subscribe(command).await?
        };
        Ok(subscription)
    }

    async fn prepare_worker_timeline_read(
        &self,
        session: &talos_store::session::SessionRecord,
    ) -> ServiceResult<()> {
        self.turns.claim_session_open(&session.id)?;
        if self
            .sessions
            .execution_node_permanently_removed(session)
            .await?
        {
            return Ok(());
        }
        if let Err(error) = self.turns.reconcile_worker_events(&session.id).await {
            if error.is_code(talos_error::ErrorCode::ServiceUnavailable)
                || error.is_code(talos_error::ErrorCode::Timeout)
            {
                // An offline execution source does not invalidate its materialized history.
                talos_logging::report_error(&error);
                return Ok(());
            }
            return Err(error);
        }
        if let Some(active) = self.turns.canonical_active_turn(&session.id).await? {
            if !self.turns.is_turn_execution_active(&active.identity) {
                self.turns
                    .recover_active_turn(session.clone(), active)
                    .await?;
            }
        }
        if let Err(error) = self.reconcile_session_goal(&session.id).await {
            talos_logging::report_error(&error);
        }
        Ok(())
    }

    pub async fn recover_timeline(
        &self,
        actor: &Actor,
        request: RecoverTimelineRequest,
    ) -> ServiceResult<talos_agent_api::RecoverTimelineResponse> {
        let session = self.access.load_session(actor, &request.session_id).await?;
        if self
            .sessions
            .supports_worker_execution(&request.session_id)
            .await?
        {
            self.prepare_worker_timeline_read(&session).await?;
        }
        let after_event_id = parse_timeline_event_id(request.after_event_id)?;
        self.events
            .recover_timeline(
                &request.session_id,
                after_event_id.as_ref(),
                request.replay_limit.unwrap_or(1).max(1),
            )
            .await
    }
}

fn parse_timeline_event_id(value: Option<String>) -> ServiceResult<Option<TimelineEventId>> {
    value
        .map(|raw| {
            let event_id = TimelineEventId::from(raw);
            event_id.event_seq()?;
            Ok(event_id)
        })
        .transpose()
}

impl TalosService {
    /// Materialize locally owned Worker facts and finish idempotent product effects.
    /// Hosts invoke this from their existing maintenance tick after startup/reconnect.
    /// Unavailable Workers retain the last cursor; no execution is cancelled here.
    pub async fn reconcile_worker_events(&self) -> ServiceResult<()> {
        let mut offset = 0;
        loop {
            let sessions = self
                .sessions
                .list_sessions(talos_store::session::SessionQuery {
                    limit: Some(256),
                    offset: Some(offset),
                    ..Default::default()
                })
                .await?;
            let count = sessions.len();
            for session in sessions {
                if let Err(error) = self.turns.reconcile_worker_events(&session.id).await {
                    if !error.is_code(talos_error::ErrorCode::InvalidRequest) {
                        talos_logging::report_error(&error);
                    }
                }
            }
            if count < 256 {
                return Ok(());
            }
            offset += count;
        }
    }
}
