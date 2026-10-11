use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use sqlx::SqlitePool;
use talos_agent::{
    AgentAdmission, AgentContext, AgentInputParams, AgentInterruptParams, AgentRunExecution,
    AgentRunParams, AgentSessionContext,
};
use talos_agent_api::{
    AgentEvent, AgentEventPayload, AgentInterruptOutcome, AgentMessage, AgentMessageBlockCompleted,
    AgentMessageBlockStarted, AgentMessageBlockStatus, AgentMessageStarted, AgentMessageStatus,
    AgentUserContent, AgentUserInput,
};
use talos_coordinator::{AgentCoordinator, AgentSelectionParams};
use talos_error::TalosResult;
use talos_store::session::{AgentEventCommit, AgentExecutionStore, CommitInitialAdmissionParams};

pub(crate) async fn run_test_migrations(pool: &SqlitePool) {
    let migration_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite");
    talos_store::migration::run_migrations(pool, &migration_dir)
        .await
        .expect("test migrations should run");
}

pub(crate) fn test_user_input(text: impl Into<String>) -> AgentUserInput {
    AgentUserInput {
        content: vec![AgentUserContent::Text { text: text.into() }],
    }
}

pub(crate) fn canonical_user_admission(
    identity: talos_agent_api::AgentTurnRef,
    message: AgentMessage,
    sequence: u64,
    include_turn_started: bool,
) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    let message_sequence = if include_turn_started {
        sequence.saturating_add(1)
    } else {
        sequence
    };
    if include_turn_started {
        events.push(AgentEvent {
            identity: identity.clone(),
            sequence,
            occurred_at: message.created_at,
            payload: AgentEventPayload::TurnStarted(talos_agent_api::AgentTurnStarted {}),
        });
    }
    events.extend(canonical_completed_message_events(
        identity,
        message,
        message_sequence,
    ));
    events
}

pub(crate) fn canonical_completed_message_events(
    identity: talos_agent_api::AgentTurnRef,
    message: AgentMessage,
    mut sequence: u64,
) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    let mut started_message = message.clone();
    started_message.status = AgentMessageStatus::InProgress;
    started_message.blocks.clear();
    started_message.completed_at = None;
    events.push(AgentEvent {
        identity: identity.clone(),
        sequence,
        occurred_at: message.created_at,
        payload: AgentEventPayload::MessageStarted(AgentMessageStarted {
            message: started_message,
        }),
    });
    sequence = sequence.saturating_add(1);
    for block in &message.blocks {
        let mut started_block = block.clone();
        started_block.status = AgentMessageBlockStatus::Running;
        started_block.completed_at = None;
        events.push(AgentEvent {
            identity: identity.clone(),
            sequence,
            occurred_at: block.started_at,
            payload: AgentEventPayload::MessageBlockStarted(AgentMessageBlockStarted {
                message_id: message.message_id.clone(),
                block: started_block,
            }),
        });
        sequence = sequence.saturating_add(1);
        events.push(AgentEvent {
            identity: identity.clone(),
            sequence,
            occurred_at: block.completed_at.unwrap_or(block.started_at),
            payload: AgentEventPayload::MessageBlockCompleted(AgentMessageBlockCompleted {
                message_id: message.message_id.clone(),
                block: block.clone(),
            }),
        });
        sequence = sequence.saturating_add(1);
    }
    events.push(AgentEvent {
        identity,
        sequence,
        occurred_at: message.completed_at.unwrap_or(message.created_at),
        payload: AgentEventPayload::MessageCompleted(message),
    });
    events
}

pub(crate) async fn commit_test_running_turn(
    store: &dyn AgentExecutionStore,
    identity: talos_agent_api::AgentTurnRef,
    message: AgentMessage,
) -> AgentEventCommit {
    store
        .commit_initial_admission(CommitInitialAdmissionParams {
            events: canonical_user_admission(identity, message, 1, true),
            command: None,
        })
        .await
        .expect("canonical test Turn admission should commit")
}

/// Minimal shared Coordinator fixture for Service tests that do not exercise
/// Agent execution. Scenario-specific tests should implement the port locally.
pub(crate) struct NoopAgentCoordinator;

#[async_trait]
impl AgentCoordinator for NoopAgentCoordinator {
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

    async fn clear_session(&self, _params: AgentSessionContext) -> TalosResult<()> {
        Ok(())
    }

    async fn close_session(&self, _params: AgentSessionContext) -> TalosResult<()> {
        Ok(())
    }

    async fn shutdown(&self, _timeout: Duration) -> TalosResult<()> {
        Ok(())
    }
}

/// Reuse the actual environment owner with deterministic fixture config and host paths.
pub(crate) fn with_environment(
    delegate: std::sync::Arc<dyn AgentCoordinator>,
    config: talos_types::TalosConfig,
) -> std::sync::Arc<dyn AgentCoordinator> {
    with_worker_environment(
        delegate,
        std::sync::Arc::new(crate::WorkerEnvironmentService::new(
            std::sync::Arc::new(std::sync::RwLock::new(config)),
            Some(std::env::temp_dir()),
        )),
    )
}

pub(crate) fn session_service(
    store: std::sync::Arc<dyn talos_store::session::SessionStore>,
    projects: std::sync::Arc<dyn talos_store::project::ProjectStore>,
    config: talos_types::TalosConfig,
) -> crate::session::SessionService {
    let coordinator = with_environment(std::sync::Arc::new(NoopAgentCoordinator), config.clone());
    crate::session::SessionService::new(store, projects, config, coordinator)
}

struct EnvironmentCoordinator {
    delegate: std::sync::Arc<dyn AgentCoordinator>,
    environment: std::sync::Arc<dyn agent_protocol::WorkerEnvironment>,
}
#[async_trait]
impl AgentCoordinator for EnvironmentCoordinator {
    async fn worker_environment(
        &self,
        _node_id: &str,
        _user_id: &str,
    ) -> TalosResult<std::sync::Arc<dyn agent_protocol::WorkerEnvironment>> {
        Ok(self.environment.clone())
    }
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
        preferred_node_id: Option<&str>,
    ) -> TalosResult<String> {
        self.delegate
            .select_execution_node(agent_id, user_id, preferred_node_id)
            .await
    }
    async fn session_harness(
        &self,
        context: &AgentSessionContext,
    ) -> TalosResult<std::sync::Arc<dyn talos_agent::AgentHarness>> {
        self.delegate.session_harness(context).await
    }
    async fn start(&self, params: AgentRunParams) -> TalosResult<AgentRunExecution> {
        self.delegate.start(params).await
    }
    async fn submit_input(&self, params: AgentInputParams) -> TalosResult<AgentAdmission> {
        self.delegate.submit_input(params).await
    }
    async fn interrupt(&self, params: AgentInterruptParams) -> TalosResult<AgentInterruptOutcome> {
        self.delegate.interrupt(params).await
    }
    async fn compact_session(&self, context: AgentContext) -> TalosResult<AgentRunExecution> {
        self.delegate.compact_session(context).await
    }
    async fn clear_session(&self, context: AgentSessionContext) -> TalosResult<()> {
        self.delegate.clear_session(context).await
    }
    async fn close_session(&self, context: AgentSessionContext) -> TalosResult<()> {
        self.delegate.close_session(context).await
    }
    async fn shutdown(&self, timeout: Duration) -> TalosResult<()> {
        self.delegate.shutdown(timeout).await
    }
}

pub(crate) fn with_worker_environment(
    delegate: std::sync::Arc<dyn AgentCoordinator>,
    environment: std::sync::Arc<dyn agent_protocol::WorkerEnvironment>,
) -> std::sync::Arc<dyn AgentCoordinator> {
    std::sync::Arc::new(EnvironmentCoordinator {
        delegate,
        environment,
    })
}
