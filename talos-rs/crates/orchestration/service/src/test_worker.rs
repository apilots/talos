//! Test composition reuses the production Worker journal instead of a product-side writer.
use agent_protocol::*;
use async_trait::async_trait;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
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
/// Select one fault for the next paused start; consuming it restores healthy delivery.
#[derive(Default)]
pub(crate) enum AdmissionFault {
    #[default]
    None,
    BeforeCommit,
    AfterCommit,
}

struct DurableCoordinator {
    delegate: Arc<dyn AgentCoordinator>,
    harness: Arc<dyn AgentHarness>,
    _directory: tempfile::TempDir,
    admission_fault: Arc<Mutex<AdmissionFault>>,
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
        let fault = std::mem::take(&mut *self.admission_fault.lock().unwrap());
        match fault {
            AdmissionFault::None => {}
            AdmissionFault::BeforeCommit => {
                execution.admission.permit = AgentAdmissionPermit::new(async {
                    Err(AgentAdmissionUnconfirmed::wrap(
                        talos_error::TalosError::new(
                            talos_error::ErrorCode::ServiceUnavailable,
                            "Commit delivery disconnected.",
                        ),
                    ))
                });
            }
            AdmissionFault::AfterCommit => {
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
    durable_with_store_and_fault(coordinator, Arc::default()).await
}

pub(crate) async fn durable_with_store_and_fault(
    coordinator: Arc<dyn AgentCoordinator>,
    admission_fault: Arc<Mutex<AdmissionFault>>,
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
            admission_fault,
        }),
        store,
    )
}

/// Write an explicit fixture source into the Worker before projecting its committed page.
pub(crate) async fn seed_worker_events(
    sessions: &crate::session::SessionService,
    product: &talos_store::session::DatabaseSessionStore,
    journal: &talos_store::execution::WorkerExecutionStore,
    events: Vec<talos_agent_api::AgentEvent>,
    continuation: Option<talos_agent_api::AgentContinuationRef>,
) -> Vec<talos_store::session::AgentEventRecord> {
    let identity = events
        .first()
        .expect("Worker fixture events")
        .identity
        .clone();
    let session = product
        .get_session_record(&identity.session_id)
        .await
        .unwrap()
        .unwrap();
    let context = sessions
        .build_agent_context(&session, identity.clone())
        .await
        .unwrap();
    let existing = journal.checkpoint(&identity).await.unwrap();
    let last_sequence = existing.as_ref().map(|checkpoint| checkpoint.last_sequence);
    let mut reducer = match existing {
        Some(checkpoint) => {
            talos_agent::AgentEventReducer::from_checkpoint(identity.clone(), checkpoint).unwrap()
        }
        None => talos_agent::AgentEventReducer::new(identity.clone()),
    };
    let split = if last_sequence.is_none() {
        let (index, input) = events
            .iter()
            .enumerate()
            .find_map(|(index, event)| match &event.payload {
                talos_agent_api::AgentEventPayload::MessageCompleted(message)
                    if message.role == talos_agent_api::AgentMessageRole::User =>
                {
                    Some((index + 1, message.clone()))
                }
                _ => None,
            })
            .expect("Worker fixture admission input");
        let admission = events[..index].to_vec();
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
                        intent: agent_protocol::AgentRunIntent::Start,
                    }),
                },
                admission,
                &reducer.checkpoint(),
            )
            .await
            .unwrap();
        index
    } else {
        0
    };
    for event in events
        .into_iter()
        .skip(split)
        .filter(|event| last_sequence.is_none_or(|last| event.sequence > last))
    {
        reducer.apply(event.clone()).unwrap();
        let settlement = match &event.payload {
            talos_agent_api::AgentEventPayload::WaitingForInput(_) => {
                journal
                    .record_waiting_continuation(
                        &context.session,
                        &identity,
                        event.sequence,
                        continuation
                            .as_ref()
                            .expect("Worker fixture waiting continuation"),
                    )
                    .await
                    .unwrap();
                Some(agent_protocol::AgentExecutionSettlement::WaitingForInput {
                    metrics: reducer.checkpoint().metrics.clone(),
                })
            }
            payload if payload.is_terminal() => {
                Some(agent_protocol::AgentExecutionSettlement::Finished {
                    memory_usage: vec![],
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
    let mut projected = Vec::new();
    let mut after = product
        .worker_event_cursor(&identity.session_id)
        .await
        .unwrap();
    loop {
        let page = journal
            .read_events(AgentSessionEventsParams {
                session: context.session.clone(),
                after_cursor: after.clone(),
                limit: agent_protocol::MAX_EXECUTION_PAGE_EVENTS,
            })
            .await
            .unwrap();
        let more = page.has_more;
        let commit = product
            .project_worker_events(talos_store::session::WorkerProjectionParams {
                execution_node_id: session.execution_node_id.clone(),
                session_id: identity.session_id.clone(),
                after_cursor: after,
                page,
            })
            .await
            .unwrap();
        projected.extend(commit.events);
        after = Some(commit.cursor);
        if !more {
            break;
        }
    }
    projected
}

pub(crate) async fn seed_running_worker(
    sessions: &crate::session::SessionService,
    product: &talos_store::session::DatabaseSessionStore,
    journal: &talos_store::execution::WorkerExecutionStore,
    identity: talos_agent_api::AgentTurnRef,
    message: talos_agent_api::AgentMessage,
) {
    seed_worker_events(
        sessions,
        product,
        journal,
        crate::test_support::canonical_user_admission(identity, message, 1, true),
        None,
    )
    .await;
}
