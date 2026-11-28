//! Destruction must settle committed Worker facts before discarding either copy.
use super::*;
use agent_protocol::{AgentExecutionCommand, AgentExecutionSettlement, AgentSessionEventsParams};
use talos_store::{execution::WorkerExecutionStore, session::WorkerProjectionParams};

#[path = "destruction_retirement_tests.rs"]
mod retirement;

struct Fixture {
    service: TalosService,
    product: Arc<DatabaseSessionStore>,
    journal: Arc<WorkerExecutionStore>,
    storage: Arc<SqliteStorage>,
    actor: Actor,
}

struct Source {
    scope: AgentSessionContext,
    identity: AgentTurnRef,
    command: AgentExecutionCommand,
}

fn coordinator() -> Arc<TestAgentCoordinator> {
    Arc::new(TestAgentCoordinator {
        cancel_mode: TestCancelMode::Cancelled,
        cancelled_turn_ids: Arc::new(Mutex::new(Vec::new())),
        compaction_usage: TokenUsage::default(),
        compaction_error: None,
    })
}

impl Fixture {
    async fn new(coordinator: Arc<dyn AgentCoordinator>) -> Self {
        let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
        let (service, product, journal) = test_application_with_worker_storage(
            Arc::new(TestAgentExecution {
                mode: TestAgentExecutionMode::Accepted,
            }),
            coordinator,
            configured_test_config(),
            None,
            storage.clone(),
            None,
        )
        .await;
        Self {
            service,
            product,
            journal,
            storage,
            actor: Actor::regular("member-user"),
        }
    }

    async fn finished_source(&self, session_id: &str) -> Source {
        if self
            .product
            .get_session_record(session_id)
            .await
            .unwrap()
            .is_none()
        {
            self.service
                .create_session(
                    &self.actor,
                    create_session_params(session_id, request_source(), None),
                )
                .await
                .unwrap();
        }
        let record = self
            .product
            .get_session_record(session_id)
            .await
            .unwrap()
            .unwrap();
        let identity = AgentTurnRef::new(session_id, format!("{session_id}-turn"));
        let context = self
            .service
            .sessions
            .build_agent_context(&record, identity.clone())
            .await
            .unwrap();
        let scope = context.session.clone();
        let input = crate::turn::completed_agent_user_input(
            &identity.turn_id,
            &format!("{session_id}-input"),
            crate::test_support::test_user_input("committed work"),
            1_700_000_000,
        )
        .unwrap();
        let events =
            crate::test_support::canonical_user_admission(identity.clone(), input.clone(), 1, true);
        let command = AgentExecutionCommand::Run {
            params: Box::new(AgentRunParams {
                context,
                input,
                subagent_limit: None,
                command: None,
                automation_origin: None,
                intent: agent_protocol::AgentRunIntent::Start,
            }),
        };
        let mut reducer = talos_agent::AgentEventReducer::new(identity.clone());
        for event in &events {
            reducer.apply(event.clone()).unwrap();
        }
        self.journal
            .admit(&scope, &command, events, &reducer.checkpoint())
            .await
            .unwrap();
        let metrics = AgentRunMetrics {
            usage: TokenUsage {
                input_tokens: 3,
                output_tokens: 2,
                total_tokens: 5,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut terminal = Vec::new();
        for payload in [
            CanonicalAgentEventPayload::MetricsUpdated(metrics.clone()),
            CanonicalAgentEventPayload::TurnCompleted(AgentTurnCompleted {
                stop_reason: talos_agent_api::AgentStopReason::EndTurn,
            }),
        ] {
            let event = CanonicalAgentEvent {
                identity: identity.clone(),
                sequence: reducer.next_sequence(),
                occurred_at: 1_700_000_001,
                payload,
            };
            reducer.apply(event.clone()).unwrap();
            terminal.push(event);
        }
        self.journal
            .append(
                &scope,
                terminal,
                &reducer.checkpoint(),
                Some(AgentExecutionSettlement::Finished {
                    metrics,
                    memory_usage: vec![],
                }),
            )
            .await
            .unwrap();
        Source {
            scope,
            identity,
            command,
        }
    }

    async fn project(&self, source: &Source) {
        let page = self
            .journal
            .read_events(AgentSessionEventsParams {
                session: source.scope.clone(),
                after_cursor: None,
                limit: 256,
            })
            .await
            .unwrap();
        self.product
            .project_worker_events(WorkerProjectionParams {
                execution_node_id: "embedded".into(),
                session_id: source.identity.session_id.clone(),
                after_cursor: None,
                page,
            })
            .await
            .unwrap();
        assert_eq!(
            self.product
                .pending_worker_settlements(&source.identity.session_id)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    async fn usage(&self, session_id: &str) -> (i64, i64) {
        sqlx::query_as("SELECT COUNT(*), COALESCE(SUM(total_tokens),0) FROM profile_turn_usage WHERE session_id=?")
            .bind(session_id).fetch_one(self.storage.pool()).await.unwrap()
    }

    async fn destroy(&self, session_id: &str, delete: bool) -> TalosResult<()> {
        if delete {
            self.service
                .delete_session(&self.actor, session_id)
                .await
                .map(|_| ())
        } else {
            self.service
                .clear_session(&self.actor, session_id)
                .await
                .map(|_| ())
        }
    }

    async fn assert_source_removed(&self, source: &Source, retired: bool) {
        assert_eq!(
            self.journal
                .is_retired(&source.identity.session_id)
                .await
                .unwrap(),
            retired
        );
        assert!(self
            .journal
            .checkpoint(&source.identity)
            .await
            .unwrap()
            .is_none());
        assert!(self
            .journal
            .admission(&source.scope, &source.identity, &source.command)
            .await
            .unwrap()
            .is_none());
        let events = self
            .journal
            .read_events(AgentSessionEventsParams {
                session: source.scope.clone(),
                after_cursor: None,
                limit: 256,
            })
            .await
            .unwrap();
        assert!(events.events.is_empty());
        assert_eq!(events.next_cursor.generation, 1);
    }
}

async fn profile_failure_blocks_destruction(delete: bool) {
    let fixture = Fixture::new(coordinator()).await;
    let source = fixture.finished_source("pending-billing").await;
    fixture.project(&source).await;
    let events = fixture
        .product
        .list_agent_turn_events(&source.identity, 100)
        .await
        .unwrap();
    let checkpoint = fixture.journal.checkpoint(&source.identity).await.unwrap();
    let source_events = fixture.journal.turn_events(&source.identity).await.unwrap();
    sqlx::query("CREATE TRIGGER reject_destruction_profile BEFORE INSERT ON profile_turn_usage BEGIN SELECT RAISE(FAIL,'profile-unavailable'); END")
        .execute(fixture.storage.pool()).await.unwrap();

    fixture
        .destroy(&source.identity.session_id, delete)
        .await
        .expect_err("unsettled billing must protect history");
    assert_eq!(fixture.usage(&source.identity.session_id).await, (0, 0));
    assert_eq!(
        fixture.journal.checkpoint(&source.identity).await.unwrap(),
        checkpoint
    );
    assert_eq!(
        fixture.journal.turn_events(&source.identity).await.unwrap(),
        source_events
    );
    assert!(!fixture
        .journal
        .is_retired(&source.identity.session_id)
        .await
        .unwrap());
    assert_eq!(
        fixture
            .product
            .list_agent_turn_events(&source.identity, 100)
            .await
            .unwrap(),
        events
    );
    assert_eq!(
        fixture
            .product
            .pending_worker_settlements(&source.identity.session_id)
            .await
            .unwrap()
            .len(),
        1
    );
    sqlx::query("DROP TRIGGER reject_destruction_profile")
        .execute(fixture.storage.pool())
        .await
        .unwrap();

    fixture
        .destroy(&source.identity.session_id, delete)
        .await
        .unwrap();
    assert_eq!(fixture.usage(&source.identity.session_id).await, (1, 5));
    fixture.assert_source_removed(&source, delete).await;
    if delete {
        assert!(fixture
            .product
            .get_session_record(&source.identity.session_id)
            .await
            .unwrap()
            .is_none());
    } else {
        assert_eq!(
            fixture
                .product
                .get_history_cursor(&source.identity.session_id)
                .await
                .unwrap()
                .unwrap()
                .history_generation,
            1
        );
        assert!(fixture
            .product
            .list_agent_turn_events(&source.identity, 100)
            .await
            .unwrap()
            .is_empty());
    }
    fixture.service.reconcile_worker_events().await.unwrap();
    assert_eq!(fixture.usage(&source.identity.session_id).await, (1, 5));
}

#[tokio::test]
async fn clear_preserves_source_and_projection_until_profile_settlement_recovers() {
    profile_failure_blocks_destruction(false).await;
}

#[tokio::test]
async fn delete_preserves_source_and_projection_until_profile_settlement_recovers() {
    profile_failure_blocks_destruction(true).await;
}

#[tokio::test]
async fn destruction_ingests_unseen_worker_terminal_before_clearing_or_deleting() {
    for delete in [false, true] {
        let fixture = Fixture::new(coordinator()).await;
        let source = fixture.finished_source("unseen-terminal").await;
        assert!(fixture
            .product
            .worker_event_cursor(&source.identity.session_id)
            .await
            .unwrap()
            .is_none());
        fixture
            .destroy(&source.identity.session_id, delete)
            .await
            .unwrap();
        assert_eq!(fixture.usage(&source.identity.session_id).await, (1, 5));
        fixture.assert_source_removed(&source, delete).await;
        fixture.service.reconcile_worker_events().await.unwrap();
        assert_eq!(fixture.usage(&source.identity.session_id).await, (1, 5));
    }
}
