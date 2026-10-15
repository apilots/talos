use std::{collections::HashMap, sync::Arc};

use agent_protocol::{CommitSessionExecutionStateParams, SessionStateVisibility};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_store::session::SessionRecord;
use talos_sync::{SyncHandle, SyncResourceKey};

use super::SessionService;

#[derive(Default)]
pub(super) struct SessionSyncState {
    peer_id: Option<String>,
    handles: HashMap<String, Arc<SyncHandle>>,
}

impl SessionService {
    pub(crate) fn schedule_shared_history_cursor(self: &Arc<Self>, session_id: String) {
        let sessions = self.clone();
        tokio::spawn(async move {
            if let Err(error) = sessions.share_history_cursor(&session_id).await {
                if !matches!(
                    error.error_code(),
                    ErrorCode::ServiceUnavailable | ErrorCode::Conflict
                ) {
                    talos_logging::report_error(&error);
                }
            }
        });
    }
    pub(crate) async fn ensure_worker_idle(
        &self,
        record: &SessionRecord,
    ) -> TalosResult<talos_types::SessionHistoryCursor> {
        let session = self.build_agent_session_context(record).await?;
        if self
            .coordinator
            .read_session_execution_state(session)
            .await?
            .is_some_and(|state| state.active_turn.is_some())
        {
            return Err(TalosError::new(
                ErrorCode::ActiveTurnRunning,
                "Session has a nonterminal Turn on its Worker.",
            ));
        }
        Ok(self
            .session_store
            .get_history_cursor(&record.id)
            .await?
            .unwrap_or_default())
    }
    pub(crate) async fn retry_shared_history_cursors(&self, account_id: &str) -> TalosResult<()> {
        let peer = self.sync.read().expect("Session sync lock").peer_id.clone();
        let Some(peer) = peer else {
            return Ok(());
        };
        let mut offset = 0;
        loop {
            let records = self
                .session_store
                .list_session_records(&talos_store::session::SessionQuery {
                    user_id: Some(account_id.into()),
                    limit: Some(64),
                    offset: Some(offset),
                    ..Default::default()
                })
                .await?;
            let length = records.len();
            for record in records {
                let result = async {
                    let session = self.build_agent_session_context(&record).await?;
                    if let Some(state) = self
                        .coordinator
                        .read_session_execution_state(session)
                        .await?
                    {
                        if state.coordinator_id.as_deref() == Some(peer.as_str())
                            && state.active_turn.is_none()
                            && !state.handoff_ready
                        {
                            self.prepare_execution(&record).await?;
                            self.share_history_cursor(&record.id).await?;
                        }
                    }
                    Ok::<_, TalosError>(())
                }
                .await;
                if let Err(error) = result {
                    if !matches!(
                        error.error_code(),
                        ErrorCode::ServiceUnavailable
                            | ErrorCode::Conflict
                            | ErrorCode::ResourceNotFound
                    ) {
                        talos_logging::report_error(&error);
                    }
                }
            }
            if length < 64 {
                break;
            }
            offset += length;
        }
        Ok(())
    }

    pub(crate) async fn remote_active_target(
        &self,
        record: &SessionRecord,
    ) -> TalosResult<Option<agent_protocol::ActiveSessionTarget>> {
        let peer = self.sync.read().expect("Session sync lock").peer_id.clone();
        let Some(peer) = peer else {
            return Ok(None);
        };
        let session = self.build_agent_session_context(record).await?;
        let Some(state) = self
            .coordinator
            .read_session_execution_state(session)
            .await?
        else {
            return Ok(None);
        };
        let Some(identity) = state.active_turn else {
            return Ok(None);
        };
        let Some(coordinator_id) = state.coordinator_id else {
            return Ok(None);
        };
        if coordinator_id == peer {
            return Ok(None);
        }
        let computer_id = self.execution_computer_id(record).await?;
        Ok(Some(agent_protocol::ActiveSessionTarget {
            computer_id,
            coordinator_id,
            identity,
        }))
    }

    pub(crate) async fn execution_computer_id(
        &self,
        record: &SessionRecord,
    ) -> TalosResult<String> {
        Ok(self
            .session_store
            .get_shared_session_info(&record.id)
            .await?
            .map_or_else(|| record.execution_node_id.clone(), |info| info.node_id))
    }

    pub(crate) fn is_current_coordinator(&self, peer: &str) -> bool {
        self.sync
            .read()
            .expect("Session sync lock")
            .peer_id
            .as_deref()
            == Some(peer)
    }

    pub(crate) fn set_coordinator_id(&self, peer_id: String) {
        self.sync.write().expect("Session sync lock").peer_id = Some(peer_id);
    }

    pub(crate) fn remove_sync_handle(&self, account_id: &str) {
        self.sync
            .write()
            .expect("Session sync lock")
            .handles
            .remove(account_id);
    }

    pub(crate) fn install_sync_handle(&self, account_id: String, handle: Arc<SyncHandle>) {
        self.sync
            .write()
            .expect("Session sync lock")
            .handles
            .insert(account_id, handle);
    }

    fn sync_handle(&self, account_id: &str) -> Option<Arc<SyncHandle>> {
        self.sync
            .read()
            .expect("Session sync lock")
            .handles
            .get(account_id)
            .cloned()
    }

    /// Read the same Worker boundary used by every transport. The current
    /// coordinator can reuse confirmed Local history without Sync. A different
    /// coordinator also needs Shared confirmation when terminal facts arrived
    /// first. Worker reservation still performs the final atomic check.
    pub(crate) async fn prepare_execution(
        &self,
        record: &SessionRecord,
    ) -> TalosResult<SessionRecord> {
        let Some(history_cursor) = self.session_store.get_history_cursor(&record.id).await? else {
            return Ok(record.clone());
        };
        let session = self.build_agent_session_context(record).await?;
        let Some(mut state) = self
            .coordinator
            .read_session_execution_state(session.clone())
            .await?
        else {
            return Ok(record.clone());
        };
        let peer = self.sync.read().expect("Session sync lock").peer_id.clone();
        if state.history_cursor == history_cursor
            && peer.is_some()
            && state.coordinator_id.is_some()
            && state.coordinator_id != peer
            && state.active_turn.is_none()
            && !state.handoff_ready
        {
            // Imported terminal facts can arrive before the source's durable ACK
            // and Shared commit. Bound this wait; only the Worker can confirm
            // handoff. Same-coordinator Local execution never waits.
            state = tokio::time::timeout(std::time::Duration::from_secs(30), async {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    let current = self
                        .coordinator
                        .read_session_execution_state(session.clone())
                        .await?
                        .ok_or_else(|| {
                            TalosError::new(
                                ErrorCode::Conflict,
                                "Worker execution state changed during handoff.",
                            )
                        })?;
                    if current.handoff_ready
                        || current.active_turn.is_some()
                        || current.coordinator_id == peer
                        || current.history_cursor != history_cursor
                    {
                        return Ok::<_, TalosError>(current);
                    }
                }
            })
            .await
            .map_err(|_| {
                TalosError::new(
                    ErrorCode::ServiceUnavailable,
                    "Worker history is awaiting synchronization confirmation.",
                )
            })??;
        }
        if state.history_cursor == history_cursor {
            return Ok(record.clone());
        }
        // Store reset is durable before its Worker commit. The same coordinator
        // may finish that exact next-generation empty reset after a lost reply.
        // The existing Worker commit checks cleanup, identity and generation.
        if state.coordinator_id == peer
            && peer.is_some()
            && state.active_turn.is_none()
            && state.history_cursor.history_generation.checked_add(1)
                == Some(history_cursor.history_generation)
            && history_cursor.completed_turn_id.is_none()
            && history_cursor.completed_event_sequence == 0
        {
            self.commit_cleared_history(record).await?;
            return Ok(record.clone());
        }
        if state.coordinator_id == peer
            && peer.is_some()
            && state.active_turn.as_ref().is_some_and(|active| {
                history_cursor.completed_turn_id.as_deref() == Some(active.turn_id.as_str())
            })
        {
            self.commit_worker_history_cursor(
                state.active_turn.as_ref().expect("confirmed terminal"),
                SessionStateVisibility::Local,
            )
            .await?;
            return Ok(record.clone());
        }
        if !state.handoff_ready {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Worker history is not available for this coordinator.",
            ));
        }
        let handle = self.sync_handle(&record.user_id).ok_or_else(|| {
            TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Shared Session history is unavailable.",
            )
        })?;
        handle
            .catch_up(
                SyncResourceKey::Session {
                    session_id: record.id.clone(),
                },
                None,
            )
            .await?;
        if self
            .session_store
            .get_history_cursor(&record.id)
            .await?
            .as_ref()
            != Some(&state.history_cursor)
        {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Shared Session history does not match its Worker.",
            ));
        }
        self.session_store
            .get_session_record(&record.id)
            .await?
            .ok_or_else(|| {
                TalosError::new(
                    ErrorCode::SessionNotFound,
                    "Session was deleted during synchronization.",
                )
            })
    }

    /// A local terminal commit remains usable when H is offline. The caller
    /// runs this after replying to execution; only H's durable ACK opens handoff.
    pub(crate) async fn share_history_cursor(&self, session_id: &str) -> TalosResult<()> {
        let Some(record) = self.session_store.get_session_record(session_id).await? else {
            return Ok(());
        };
        let Some(handle) = self.sync_handle(&record.user_id) else {
            return Ok(());
        };
        let history_cursor = self
            .session_store
            .get_history_cursor(session_id)
            .await?
            .ok_or_else(|| TalosError::new(ErrorCode::SessionNotFound, "Session was not found."))?;
        handle
            .flush(SyncResourceKey::Session {
                session_id: session_id.into(),
            })
            .await?;
        // A newer local Turn may have started while the upload was in flight.
        // The Worker rejects this obsolete confirmation instead of releasing it.
        let session = self.build_agent_session_context(&record).await?;
        self.coordinator
            .commit_session_execution_state(CommitSessionExecutionStateParams {
                session,
                identity: history_cursor
                    .completed_turn_id
                    .as_ref()
                    .map(|turn| talos_types::AgentTurnRef::new(session_id, turn)),
                history_cursor,
                visibility: SessionStateVisibility::Shared,
            })
            .await
    }

    pub(crate) async fn commit_cleared_history(&self, record: &SessionRecord) -> TalosResult<()> {
        let Some(history_cursor) = self.session_store.get_history_cursor(&record.id).await? else {
            return Ok(());
        };
        self.coordinator
            .commit_session_execution_state(CommitSessionExecutionStateParams {
                session: self.build_agent_session_context(record).await?,
                identity: None,
                history_cursor,
                visibility: SessionStateVisibility::Local,
            })
            .await
    }
}
