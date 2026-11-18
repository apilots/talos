use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use talos_agent_api::*;
use talos_error::{ApiError, ErrorCode, TalosResult};
use talos_storage::{Row, SqliteStorage, Storage, StorageTransaction};
use tokio::sync::oneshot;

use super::*;

#[tokio::test]
async fn local_turns_finish_and_cancel_while_sync_backlog_is_being_read() {
    let (paused_tx, paused_rx) = oneshot::channel();
    let (resume_tx, resume_rx) = oneshot::channel();
    let peer = peer_with_sync_storage(true, |storage| {
        Arc::new(PausedOutbox {
            storage,
            pause: Arc::new(Mutex::new(Some((paused_tx, resume_rx)))),
        })
    })
    .await;
    let sessions = &peer.sessions;
    let completion = AgentTurnRef::new("completing-session", "completed-turn");
    let cancellation = AgentTurnRef::new("stopping-session", "cancelled-turn");
    for identity in [&completion, &cancellation] {
        sessions
            .insert_session_record(&record(&identity.session_id))
            .await
            .unwrap();
        sessions
            .commit_initial_admission(CommitInitialAdmissionParams {
                subagent_limit: None,
                automation_origin: None,
                events: vec![
                    event(
                        identity,
                        1,
                        AgentEventPayload::TurnStarted(AgentTurnStarted {}),
                    ),
                    event(
                        identity,
                        2,
                        AgentEventPayload::MessageCompleted(AgentMessage {
                            message_id: format!("message-{}", identity.turn_id),
                            turn_id: identity.turn_id.clone(),
                            role: AgentMessageRole::User,
                            status: AgentMessageStatus::Completed,
                            blocks: vec![],
                            created_at: 1,
                            completed_at: Some(1),
                        }),
                    ),
                ],
                command: None,
            })
            .await
            .unwrap();
    }
    for sequence in 3..35 {
        let active = sessions
            .get_agent_active_turn(&completion.session_id)
            .await
            .unwrap()
            .unwrap();
        sessions
            .commit_progress_event(
                event(
                    &completion,
                    sequence,
                    AgentEventPayload::MessageBlockDelta(AgentMessageBlockDelta {
                        message_id: "assistant-message".into(),
                        block_id: "assistant-text".into(),
                        delta: AgentMessageBlockDeltaContent::Text("streamed response ".repeat(64)),
                    }),
                ),
                active.last_event_seq,
            )
            .await
            .unwrap();
    }

    let resource = SyncResourceKey::Session {
        session_id: completion.session_id.clone(),
    };
    let upload = {
        let sync = peer.sync.clone();
        let binding = peer.adapter.binding().clone();
        let resource = resource.clone();
        tokio::spawn(async move { sync.export_pending(&binding, resource, 128).await })
    };
    tokio::time::timeout(Duration::from_secs(5), paused_rx)
        .await
        .expect("Sync must reach its backlog read")
        .unwrap();

    let local_result = tokio::time::timeout(Duration::from_secs(2), async {
        let finish = complete(
            sessions,
            &completion,
            35,
            AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                stop_reason: AgentStopReason::EndTurn,
            }),
        );
        let stop = async {
            let admission = sessions.begin_cancellation(&cancellation).await.unwrap();
            assert_eq!(admission.active_turn.state, ActiveTurnState::Cancelling);
            complete(
                sessions,
                &cancellation,
                3,
                AgentEventPayload::TurnFailed(AgentTurnFailed {
                    error: ApiError::new(ErrorCode::Cancelled),
                    retryable: false,
                }),
            )
            .await;
        };
        tokio::join!(finish, stop);
    })
    .await;
    let _ = resume_tx.send(());
    local_result.expect("Local completion and cancellation must commit while Sync is paused");
    let first = upload.await.unwrap().unwrap().unwrap();
    let terminal = peer
        .sync
        .export_pending(peer.adapter.binding(), resource, 128)
        .await
        .unwrap()
        .unwrap();
    assert!(terminal.through_sequence > first.through_sequence);
    assert_eq!(
        sessions
            .get_session_record(&completion.session_id)
            .await
            .unwrap()
            .unwrap()
            .latest_turn_seq,
        1,
    );
    for identity in [completion, cancellation] {
        assert!(sessions
            .get_agent_active_turn(&identity.session_id)
            .await
            .unwrap()
            .is_none());
    }
}

fn event(identity: &AgentTurnRef, sequence: u64, payload: AgentEventPayload) -> AgentEvent {
    AgentEvent {
        identity: identity.clone(),
        sequence,
        occurred_at: 1,
        payload,
    }
}

async fn complete(
    sessions: &DatabaseSessionStore,
    identity: &AgentTurnRef,
    sequence: u64,
    payload: AgentEventPayload,
) {
    let active = sessions
        .get_agent_active_turn(&identity.session_id)
        .await
        .unwrap()
        .unwrap();
    let completed_turn_metrics =
        matches!(payload, AgentEventPayload::TurnCompleted(_)).then(AgentRunMetrics::default);
    sessions
        .commit_terminal_settlement(CommitTerminalSettlementParams {
            event: event(identity, sequence, payload),
            expected_event_seq: active.last_event_seq,
            completed_turn_metrics,
        })
        .await
        .unwrap();
}

type Pause = Arc<Mutex<Option<(oneshot::Sender<()>, oneshot::Receiver<()>)>>>;

struct PausedOutbox {
    storage: Arc<SqliteStorage>,
    pause: Pause,
}

#[async_trait]
impl Storage for PausedOutbox {
    async fn begin_read_transaction(&self) -> TalosResult<Box<dyn StorageTransaction>> {
        Ok(self.wrap(self.storage.begin_read_transaction().await?))
    }

    async fn begin_transaction(&self) -> TalosResult<Box<dyn StorageTransaction>> {
        Ok(self.wrap(self.storage.begin_transaction().await?))
    }

    async fn query(&self, sql: &str, params: &[&str]) -> TalosResult<Vec<Row>> {
        self.storage.query(sql, params).await
    }

    async fn execute(&self, sql: &str, params: &[&str]) -> TalosResult<u64> {
        self.storage.execute(sql, params).await
    }

    async fn insert(&self, table: &str, data: &Row) -> TalosResult<i64> {
        self.storage.insert(table, data).await
    }

    async fn update(
        &self,
        table: &str,
        data: &Row,
        where_clause: &str,
        params: &[&str],
    ) -> TalosResult<u64> {
        self.storage.update(table, data, where_clause, params).await
    }

    async fn delete(&self, table: &str, where_clause: &str, params: &[&str]) -> TalosResult<u64> {
        self.storage.delete(table, where_clause, params).await
    }

    async fn table_exists(&self, table: &str) -> TalosResult<bool> {
        self.storage.table_exists(table).await
    }
}

impl PausedOutbox {
    fn wrap(&self, transaction: Box<dyn StorageTransaction>) -> Box<dyn StorageTransaction> {
        Box::new(PausedOutboxTransaction {
            transaction,
            pause: self.pause.clone(),
        })
    }
}

struct PausedOutboxTransaction {
    transaction: Box<dyn StorageTransaction>,
    pause: Pause,
}

#[async_trait]
impl StorageTransaction for PausedOutboxTransaction {
    async fn query(&mut self, sql: &str, params: &[&str]) -> TalosResult<Vec<Row>> {
        let rows = self.transaction.query(sql, params).await?;
        if sql.contains("changes_json") && sql.contains("FROM sync_outbox") {
            let pause = self.pause.lock().unwrap().take();
            if let Some((paused, resume)) = pause {
                let _ = paused.send(());
                let _ = resume.await;
            }
        }
        Ok(rows)
    }

    async fn execute(&mut self, sql: &str, params: &[&str]) -> TalosResult<u64> {
        self.transaction.execute(sql, params).await
    }

    async fn commit(self: Box<Self>) -> TalosResult<()> {
        self.transaction.commit().await
    }

    async fn rollback(self: Box<Self>) -> TalosResult<()> {
        self.transaction.rollback().await
    }
}
