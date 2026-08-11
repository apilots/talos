use std::sync::Arc;

use talos_config::resolve_effective_kernel_workspace;
use talos_error::TalosResult;
use talos_store::session::{SessionArtifactStore, SessionTurnWindow, TimelineEventId};
use talos_types::talos::{SessionHistoryMessage, SessionHistorySnapshot, TalosTimelineItem};
use talos_types::TalosSession;

use super::message_merge::{
    history_messages_from_completed_events, map_turns_to_history_messages,
    merge_completed_timeline_message,
};
use super::stable_keys::stable_message_keys;
use crate::TimelineEventService;

const STABLE_EVENT_SCAN_BATCH_SIZE: usize = 2048;

#[derive(Clone)]
pub struct SessionHistoryService {
    session_artifact_store: Arc<dyn SessionArtifactStore>,
    event_service: Arc<TimelineEventService>,
}

impl SessionHistoryService {
    pub fn new(
        session_artifact_store: Arc<dyn SessionArtifactStore>,
        event_service: Arc<TimelineEventService>,
    ) -> Self {
        Self {
            session_artifact_store,
            event_service,
        }
    }

    pub async fn snapshot(&self, session: &TalosSession) -> TalosResult<SessionHistorySnapshot> {
        let session_id = &session.context.session_id;
        let workspace_root =
            resolve_effective_kernel_workspace(&session.config.agent.kernel, &session.user_id)?
                .user_workspace_dir;
        let turns = self
            .session_artifact_store
            .load_turns(&workspace_root, session_id)
            .await?;
        let mut messages = map_turns_to_history_messages(turns);
        let completed_timeline_messages = self.completed_timeline_messages(session_id).await?;
        let last_stable_event_id = self
            .last_stable_event_id(session_id, &completed_timeline_messages)
            .await?;
        for timeline_message in completed_timeline_messages {
            merge_completed_timeline_message(&mut messages, timeline_message);
        }
        Ok(SessionHistorySnapshot {
            session_id: session_id.clone(),
            messages,
            generated_at_ms: chrono::Utc::now().timestamp_millis(),
            last_stable_event_id,
        })
    }

    /// Loads the one persisted latest Turn and verifies its message identity.
    ///
    /// Subagent projection uses this bounded path because ActiveTurn identifies
    /// the latest Turn. General Session history must continue using `snapshot`.
    pub(crate) async fn latest_turn_items(
        &self,
        session: &TalosSession,
        expected_message_id: &str,
    ) -> TalosResult<Vec<TalosTimelineItem>> {
        let session_id = &session.context.session_id;
        let workspace_root =
            resolve_effective_kernel_workspace(&session.config.agent.kernel, &session.user_id)?
                .user_workspace_dir;
        let turns = self
            .session_artifact_store
            .load_turn_window(&workspace_root, session_id, SessionTurnWindow::Latest, 1)
            .await?;
        let message = map_turns_to_history_messages(turns)
            .into_iter()
            .next()
            .filter(|message| message.message_id == expected_message_id)
            .ok_or_else(|| {
                talos_error::TalosError::public(talos_error::ErrorCode::Conflict, format!(
                    "latest persisted turn for session '{session_id}' does not belong to message '{expected_message_id}'"
                ))
            })?;
        Ok(message.items)
    }

    async fn completed_timeline_messages(
        &self,
        session_id: &str,
    ) -> TalosResult<Vec<SessionHistoryMessage>> {
        let mut cursor: Option<TimelineEventId> = None;
        let mut collected = Vec::new();
        loop {
            let events = self
                .event_service
                .list_events_after(session_id, cursor.as_ref(), STABLE_EVENT_SCAN_BATCH_SIZE)
                .await?;
            if events.is_empty() {
                break;
            }
            collected.extend(events.iter().cloned());
            cursor = events
                .last()
                .and_then(|event| event.event_id.clone())
                .map(TimelineEventId::from);
            if events.len() < STABLE_EVENT_SCAN_BATCH_SIZE {
                break;
            }
        }
        collected.sort_by_key(|event| event.event_seq.unwrap_or_default());
        Ok(history_messages_from_completed_events(collected))
    }

    async fn last_stable_event_id(
        &self,
        session_id: &str,
        messages: &[SessionHistoryMessage],
    ) -> TalosResult<Option<String>> {
        let stable_keys = stable_message_keys(messages);
        if stable_keys.is_empty() {
            return Ok(None);
        }

        let mut cursor: Option<TimelineEventId> = None;
        let mut last_stable_event_id = None;
        loop {
            let events = self
                .event_service
                .list_events_after(session_id, cursor.as_ref(), STABLE_EVENT_SCAN_BATCH_SIZE)
                .await?;
            if events.is_empty() {
                break;
            }
            for event in &events {
                if stable_keys.contains_event(event) {
                    last_stable_event_id = event.event_id.clone();
                }
            }
            cursor = events
                .last()
                .and_then(|event| event.event_id.clone())
                .map(TimelineEventId::from);
            if events.len() < STABLE_EVENT_SCAN_BATCH_SIZE {
                break;
            }
        }
        Ok(last_stable_event_id)
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
