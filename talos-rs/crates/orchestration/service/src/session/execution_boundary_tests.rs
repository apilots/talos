//! Imported terminal history must not race the original coordinator's Shared commit.
use super::*;
use agent_protocol::SessionExecutionState;
use std::{future::Future, task::Poll, time::Duration};
use talos_agent::{
    AgentAdmission, AgentInputParams, AgentInterruptParams, AgentRunExecution, AgentRunParams,
};
use talos_agent_api::AgentInterruptOutcome;
use talos_coordinator::{AgentCoordinator, AgentSelectionParams};
use talos_types::SessionHistoryCursor;

const SOURCE: &str = "6bdff1a0-1439-4197-a4bb-1a2316371041";
const DESTINATION: &str = "f76e548e-47b6-46dd-9aef-6441c9e89d64";

struct BoundaryCoordinator {
    state: Mutex<SessionExecutionState>,
}

async fn fixture() -> (
    super::super::SessionService,
    SessionRecord,
    Arc<BoundaryCoordinator>,
    tempfile::TempDir,
) {
    let cursor = SessionHistoryCursor {
        history_generation: 0,
        completed_turn_id: Some("finished-turn".into()),
        completed_event_sequence: 42,
    };
    let store = Arc::new(RecordingSessionStore::default());
    *store.history_cursor.lock().unwrap() = Some(cursor.clone());
    let coordinator = Arc::new(BoundaryCoordinator {
        state: Mutex::new(SessionExecutionState {
            history_cursor: cursor,
            active_turn: None,
            coordinator_id: Some(SOURCE.into()),
            handoff_ready: false,
        }),
    });
    let home = tempfile::tempdir().unwrap();
    let mut config = config_with_model("anthropic", "test-model")
        .with_runtime_paths(talos_config::TalosPaths::from_home(home.path().into()));
    config.apply_derived_paths();
    let service = super::super::SessionService::new(
        store,
        Arc::new(MockProjectStore::new()),
        config.clone(),
        crate::test_support::with_worker_environment(
            coordinator.clone(),
            Arc::new(crate::WorkerEnvironmentService::new(
                Arc::new(std::sync::RwLock::new(config)),
                Some(home.path().into()),
            )),
        ),
    );
    service.set_coordinator_id(DESTINATION.into());
    let record = service
        .create_session(create_session_params(source(), "default", None))
        .await
        .unwrap();
    (service, record, coordinator, home)
}

#[tokio::test(start_paused = true)]
async fn imported_terminal_history_waits_for_worker_shared_confirmation() {
    let (service, record, coordinator, _home) = fixture().await;
    let mut prepare = std::pin::pin!(service.prepare_execution(&record));
    assert!(
        std::future::poll_fn(|context| Poll::Ready(prepare.as_mut().poll(context)))
            .await
            .is_pending(),
        "matching imported history alone must not admit a different coordinator"
    );
    coordinator.state.lock().unwrap().handoff_ready = true;
    assert_eq!(prepare.await.unwrap().id, record.id);
    assert_eq!(
        coordinator.state.lock().unwrap().coordinator_id.as_deref(),
        Some(SOURCE)
    );
}

#[tokio::test(start_paused = true)]
async fn imported_terminal_history_waits_through_worker_cleanup_and_local_commit() {
    let (service, record, coordinator, _home) = fixture().await;
    let terminal_cursor = coordinator.state.lock().unwrap().history_cursor.clone();
    {
        let mut state = coordinator.state.lock().unwrap();
        state.history_cursor = SessionHistoryCursor::default();
        state.active_turn = Some(AgentTurnRef::new(&record.id, "finished-turn"));
    }
    let mut prepare = std::pin::pin!(service.prepare_execution(&record));
    assert!(
        std::future::poll_fn(|context| Poll::Ready(prepare.as_mut().poll(context)))
            .await
            .is_pending()
    );
    {
        let mut state = coordinator.state.lock().unwrap();
        state.active_turn = None;
        state.history_cursor = terminal_cursor;
    }
    tokio::time::advance(Duration::from_millis(100)).await;
    assert!(
        std::future::poll_fn(|context| Poll::Ready(prepare.as_mut().poll(context)))
            .await
            .is_pending(),
        "Local commit alone must not complete the cross-coordinator handoff"
    );
    coordinator.state.lock().unwrap().handoff_ready = true;
    assert_eq!(prepare.await.unwrap().id, record.id);
}

#[tokio::test(start_paused = true)]
async fn unconfirmed_shared_history_times_out_without_opening_worker_boundary() {
    let (service, record, coordinator, _home) = fixture().await;
    let error = service.prepare_execution(&record).await.unwrap_err();
    assert_eq!(
        error.error_code(),
        talos_error::ErrorCode::ServiceUnavailable
    );
    assert!(!coordinator.state.lock().unwrap().handoff_ready);
}

#[tokio::test(start_paused = true)]
async fn current_coordinator_keeps_immediate_local_execution_without_sync() {
    let (service, record, coordinator, _home) = fixture().await;
    service.set_coordinator_id(SOURCE.into());
    let before = tokio::time::Instant::now();
    assert_eq!(
        service.prepare_execution(&record).await.unwrap().id,
        record.id
    );
    assert_eq!(tokio::time::Instant::now(), before);
    assert!(!coordinator.state.lock().unwrap().handoff_ready);
}

#[tokio::test(start_paused = true)]
async fn stale_imported_history_still_conflicts_without_waiting_or_releasing_worker() {
    let (service, record, coordinator, _home) = fixture().await;
    coordinator
        .state
        .lock()
        .unwrap()
        .history_cursor
        .completed_event_sequence += 1;
    let before = tokio::time::Instant::now();
    assert_eq!(
        service
            .prepare_execution(&record)
            .await
            .unwrap_err()
            .error_code(),
        talos_error::ErrorCode::Conflict
    );
    assert_eq!(tokio::time::Instant::now(), before);
    assert!(!coordinator.state.lock().unwrap().handoff_ready);
}

#[async_trait]
impl AgentCoordinator for BoundaryCoordinator {
    async fn read_session_execution_state(
        &self,
        _: AgentSessionContext,
    ) -> TalosResult<Option<SessionExecutionState>> {
        Ok(Some(self.state.lock().unwrap().clone()))
    }

    async fn resolve_agent(&self, _params: AgentSelectionParams) -> TalosResult<String> {
        Ok("default".to_string())
    }

    async fn ensure_agent(&self, _agent_id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn select_execution_node(
        &self,
        _agent_id: &str,
        _user_id: &str,
        preferred_node_id: Option<&str>,
    ) -> TalosResult<String> {
        Ok(preferred_node_id.unwrap_or("embedded").to_string())
    }

    async fn start(&self, _params: AgentRunParams) -> TalosResult<AgentRunExecution> {
        unreachable!("Agent execution is outside this test scenario")
    }

    async fn submit_input(&self, _params: AgentInputParams) -> TalosResult<AgentAdmission> {
        unreachable!("Agent input is outside this test scenario")
    }

    async fn interrupt(&self, _params: AgentInterruptParams) -> TalosResult<AgentInterruptOutcome> {
        Ok(AgentInterruptOutcome::Cancelled)
    }

    async fn compact_session(&self, _params: AgentContext) -> TalosResult<AgentRunExecution> {
        unreachable!("Agent compaction is outside this test scenario")
    }

    async fn clear_session(
        &self,
        _params: AgentSessionContext,
        _expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn close_session(
        &self,
        _params: AgentSessionContext,
        _expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn shutdown(&self, _timeout: Duration) -> TalosResult<()> {
        Ok(())
    }
}
