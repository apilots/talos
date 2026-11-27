//! Test composition reuses the production Worker journal instead of a product-side writer.
use agent_protocol::*;
use async_trait::async_trait;
use std::{sync::Arc, time::Duration};
use talos_coordinator::{AgentCoordinator, AgentSelectionParams};
use talos_error::TalosResult;
use talos_store::session::{AgentExecutionStore, SessionStore};

struct CoordinatorHarness(Arc<dyn AgentCoordinator>);
impl AgentHarness for CoordinatorHarness {
    fn start(&self, params: AgentRunParams) -> AgentFuture<'_, AgentResult<AgentRunExecution>> {
        Box::pin(self.0.start(params))
    }
    fn submit_input(
        &self,
        params: AgentInputParams,
    ) -> AgentFuture<'_, AgentResult<AgentAdmission>> {
        Box::pin(self.0.submit_input(params))
    }
    fn interrupt(
        &self,
        params: AgentInterruptParams,
    ) -> AgentFuture<'_, AgentResult<talos_agent_api::AgentInterruptOutcome>> {
        Box::pin(self.0.interrupt(params))
    }
    fn compact_session(
        &self,
        context: AgentContext,
    ) -> AgentFuture<'_, AgentResult<AgentRunExecution>> {
        Box::pin(self.0.compact_session(context))
    }
    fn clear_session(
        &self,
        context: AgentSessionContext,
        expected: talos_types::SessionHistoryCursor,
    ) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(self.0.clear_session(context, expected))
    }
    fn close_session(
        &self,
        context: AgentSessionContext,
        expected: talos_types::SessionHistoryCursor,
    ) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(self.0.close_session(context, expected))
    }
    fn shutdown(&self, timeout: Duration) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(self.0.shutdown(timeout))
    }
}
struct DurableCoordinator {
    delegate: Arc<dyn AgentCoordinator>,
    harness: Arc<dyn AgentHarness>,
    _directory: tempfile::TempDir,
    lose_next_ack: std::sync::atomic::AtomicBool,
}
#[async_trait]
impl AgentCoordinator for DurableCoordinator {
    async fn resolve_agent(&self, params: AgentSelectionParams) -> TalosResult<String> {
        self.delegate.resolve_agent(params).await
    }
    async fn ensure_agent(&self, agent_id: &str) -> TalosResult<()> {
        self.delegate.ensure_agent(agent_id).await
    }
    async fn select_execution_node(
        &self,
        agent_id: &str,
        user_id: &str,
        preferred: Option<&str>,
    ) -> TalosResult<String> {
        self.delegate
            .select_execution_node(agent_id, user_id, preferred)
            .await
    }
    async fn worker_environment(
        &self,
        node_id: &str,
        user_id: &str,
    ) -> TalosResult<Arc<dyn WorkerEnvironment>> {
        self.delegate.worker_environment(node_id, user_id).await
    }
    async fn session_harness(&self, _: &AgentSessionContext) -> TalosResult<Arc<dyn AgentHarness>> {
        Ok(self.harness.clone())
    }
    async fn read_session_events(
        &self,
        params: AgentSessionEventsParams,
    ) -> TalosResult<AgentSessionEventsResult> {
        self.harness.read_session_events(params).await
    }
    async fn read_session_execution_state(
        &self,
        params: AgentSessionContext,
    ) -> TalosResult<Option<SessionExecutionState>> {
        self.harness.read_session_execution_state(params).await
    }
    async fn start(&self, params: AgentRunParams) -> TalosResult<AgentRunExecution> {
        let mut execution = self.harness.start(params).await?;
        if self
            .lose_next_ack
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            let permit = std::mem::replace(
                &mut execution.admission.permit,
                AgentAdmissionPermit::new(async { Ok(()) }),
            );
            execution.admission.permit = AgentAdmissionPermit::new(async move {
                permit.consume().await?;
                Err(AgentAdmissionUnconfirmed::wrap(
                    talos_error::TalosError::new(
                        talos_error::ErrorCode::ServiceUnavailable,
                        "Worker admission confirmation disconnected.",
                    ),
                ))
            });
        }
        Ok(execution)
    }
    async fn submit_input(&self, params: AgentInputParams) -> TalosResult<AgentAdmission> {
        self.harness.submit_input(params).await
    }
    async fn interrupt(
        &self,
        params: AgentInterruptParams,
    ) -> TalosResult<talos_agent_api::AgentInterruptOutcome> {
        self.harness.interrupt(params).await
    }
    async fn compact_session(&self, params: AgentContext) -> TalosResult<AgentRunExecution> {
        self.harness.compact_session(params).await
    }
    async fn clear_session(
        &self,
        params: AgentSessionContext,
        expected: talos_types::SessionHistoryCursor,
    ) -> TalosResult<()> {
        self.harness.clear_session(params, expected).await
    }
    async fn retire_session(
        &self,
        params: AgentSessionContext,
        expected: talos_types::SessionHistoryCursor,
    ) -> TalosResult<()> {
        self.harness.retire_session(params, expected).await
    }
    async fn close_session(
        &self,
        params: AgentSessionContext,
        expected: talos_types::SessionHistoryCursor,
    ) -> TalosResult<()> {
        self.harness.close_session(params, expected).await
    }
    async fn shutdown(&self, timeout: Duration) -> TalosResult<()> {
        self.harness.shutdown(timeout).await
    }
}
pub(crate) async fn durable_with_store(
    coordinator: Arc<dyn AgentCoordinator>,
) -> (
    Arc<dyn AgentCoordinator>,
    Arc<talos_store::execution::WorkerExecutionStore>,
) {
    durable_with_store_and_confirmation(coordinator, false).await
}

pub(crate) async fn durable_with_store_and_confirmation(
    coordinator: Arc<dyn AgentCoordinator>,
    lose_confirmation: bool,
) -> (
    Arc<dyn AgentCoordinator>,
    Arc<talos_store::execution::WorkerExecutionStore>,
) {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(
        talos_store::execution::WorkerExecutionStore::open(&directory.path().join("worker.sqlite"))
            .await
            .unwrap(),
    );
    (
        Arc::new(DurableCoordinator {
            delegate: coordinator.clone(),
            harness: Arc::new(talos_agent::LocalExecutionHarness::new(
                Arc::new(CoordinatorHarness(coordinator)),
                store.clone(),
            )),
            _directory: directory,
            lose_next_ack: std::sync::atomic::AtomicBool::new(lose_confirmation),
        }),
        store,
    )
}

pub(crate) async fn seed_product_history(
    sessions: &crate::session::SessionService,
    product: &talos_store::session::DatabaseSessionStore,
    journal: &talos_store::execution::WorkerExecutionStore,
    session_id: &str,
) {
    let session = product
        .get_session_record(session_id)
        .await
        .unwrap()
        .unwrap();
    let events = product
        .list_agent_events_after(&session.id, None, 1000)
        .await
        .unwrap();
    if events.is_empty() {
        return;
    }
    let identity = events[0].identity.clone();
    let existing = journal.checkpoint(&identity).await.unwrap();
    let context = sessions
        .build_agent_context(&session, identity.clone())
        .await
        .unwrap();
    let input = events
        .iter()
        .find_map(|event| match &event.payload {
            talos_agent_api::AgentEventPayload::MessageCompleted(message)
                if message.role == talos_agent_api::AgentMessageRole::User =>
            {
                Some(message.clone())
            }
            _ => None,
        })
        .unwrap();
    let split=events.iter().position(|event|matches!(&event.payload,talos_agent_api::AgentEventPayload::MessageCompleted(message) if message.message_id==input.message_id)).unwrap()+1;
    let last_sequence = existing.as_ref().map(|checkpoint| checkpoint.last_sequence);
    let mut reducer = match existing {
        Some(checkpoint) => {
            talos_agent::AgentEventReducer::from_checkpoint(identity.clone(), checkpoint).unwrap()
        }
        None => talos_agent::AgentEventReducer::new(identity.clone()),
    };

    if last_sequence.is_none() {
        let admission = events[..split]
            .iter()
            .map(|event| talos_agent_api::AgentEvent {
                identity: event.identity.clone(),
                sequence: event.sequence,
                occurred_at: event.occurred_at,
                payload: event.payload.clone(),
            })
            .collect::<Vec<_>>();
        for event in &admission {
            reducer.apply(event.clone()).unwrap();
        }
        journal
            .admit(
                &context.session,
                &agent_protocol::AgentExecutionCommand::Run {
                    params: Box::new(AgentRunParams {
                        subagent_limit: None,
                        context: context.clone(),
                        input,
                        command: None,
                        automation_origin: None,
                        checkpoint: None,
                        continuation: None,
                    }),
                },
                admission,
                &reducer.checkpoint(),
            )
            .await
            .unwrap();
    }
    let continuation = product
        .get_agent_continuation_binding(&identity)
        .await
        .unwrap()
        .map(|binding| binding.provider_ref)
        .unwrap_or(talos_agent_api::AgentContinuationRef {
            opaque_id: "continuation-1".into(),
        });
    let progress = last_sequence.map_or(split, |last| {
        events
            .iter()
            .position(|event| event.sequence > last)
            .unwrap_or(events.len())
    });
    for event in &events[progress..] {
        let event = talos_agent_api::AgentEvent {
            identity: event.identity.clone(),
            sequence: event.sequence,
            occurred_at: event.occurred_at,
            payload: event.payload.clone(),
        };
        reducer.apply(event.clone()).unwrap();
        let settlement = match &event.payload {
            talos_agent_api::AgentEventPayload::WaitingForInput(_) => {
                Some(agent_protocol::AgentExecutionSettlement::WaitingForInput {
                    continuation: continuation.clone(),
                    metrics: reducer.checkpoint().metrics.clone(),
                })
            }
            payload if payload.is_terminal() => {
                Some(agent_protocol::AgentExecutionSettlement::Finished {
                    memory_usage: Vec::new(),
                    metrics: reducer.checkpoint().metrics.clone(),
                })
            }
            _ => None,
        };
        journal
            .append(
                &context.session,
                vec![event],
                &reducer.checkpoint(),
                settlement,
            )
            .await
            .unwrap();
    }
}
