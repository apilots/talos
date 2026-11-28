use std::sync::Arc;

use talos_agent::{
    AgentFuture, AgentResult, SessionHistoryReadParams, SessionHistoryReader, SessionHistoryWindow,
};
use talos_agent_api::{
    AgentEventPayload, AgentRunFailure, AgentRunOutput, AgentRunResult, SessionHistoryTurn,
};
use talos_agent_provider_kernel::KernelMemoryProgressStore;
use talos_error::{ErrorCode, TalosError};
use talos_store::execution::WorkerExecutionStore;

// Local context loading is independent of the RPC page budget. The Kernel
// applies its configured per-message/tool prompt limits after this bounded read.
const MAX_LOCAL_HISTORY_BYTES: usize = 16 * 1024 * 1024;

/// The same local reader is injected into every Provider on this Worker.
#[derive(Clone)]
pub(crate) struct WorkerExecutionHistory(pub Arc<WorkerExecutionStore>);

impl SessionHistoryReader for WorkerExecutionHistory {
    fn append_supplement(
        &self,
        supplement: talos_agent_api::trajectory::SessionHistorySupplement,
    ) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(async move { self.0.append_supplement(supplement).await })
    }

    fn read(
        &self,
        params: SessionHistoryReadParams,
    ) -> AgentFuture<'_, AgentResult<Vec<SessionHistoryTurn>>> {
        Box::pin(async move {
            if params.limit == 0 {
                return Ok(Vec::new());
            }
            let after = match &params.window {
                SessionHistoryWindow::Latest => None,
                SessionHistoryWindow::OldestAfter {
                    completed_event_seq,
                } => Some(*completed_event_seq),
            };
            let latest = matches!(params.window, SessionHistoryWindow::Latest);
            let mut terminals = self
                .0
                .terminal_events(&params.session_id, after, params.limit.min(256))
                .await?;
            if latest {
                terminals.reverse();
            }
            let mut result = Vec::new();
            let mut bytes = 0;
            for (offset, event) in terminals {
                let checkpoint = self.0.checkpoint(&event.identity).await?.ok_or_else(|| {
                    TalosError::internal("Worker terminal checkpoint is missing.")
                })?;
                let turn_result = match event.payload {
                    AgentEventPayload::TurnCompleted(completion) => {
                        AgentRunResult::Completed(AgentRunOutput {
                            messages: checkpoint.messages,
                            metrics: checkpoint.metrics,
                            completion,
                        })
                    }
                    AgentEventPayload::TurnFailed(failure) => {
                        AgentRunResult::Failed(AgentRunFailure {
                            messages: checkpoint.messages,
                            metrics: checkpoint.metrics,
                            failure,
                        })
                    }
                    _ => return Err(TalosError::internal("Worker terminal history is invalid.")),
                };
                let automation_input = self.0.turn_automation_origin(&event.identity).await?;
                let turn = SessionHistoryTurn {
                    automation_input,
                    identity: event.identity,
                    result: turn_result,
                    notices: Vec::new(),
                    completed_event_seq: offset,
                    completed_at: event.occurred_at,
                };
                let size = serde_json::to_vec(&turn)
                    .map_err(|error| {
                        TalosError::internal_with_source("Measure Worker context history.", error)
                    })?
                    .len();
                if bytes + size > MAX_LOCAL_HISTORY_BYTES {
                    if result.is_empty() {
                        return Err(TalosError::new(
                            ErrorCode::ContextWindowExceeded,
                            "One Worker Turn exceeds the local context read budget.",
                        ));
                    }
                    break;
                }
                bytes += size;
                result.push(turn);
            }
            if latest {
                result.reverse();
            }
            Ok(result)
        })
    }
}

#[async_trait::async_trait]
impl KernelMemoryProgressStore for WorkerExecutionHistory {
    async fn processed_until_event_seq(&self, session_id: &str) -> AgentResult<u64> {
        self.0.memory_progress(session_id).await
    }
    async fn advance_processed_until_event_seq(
        &self,
        session_id: &str,
        expected_until_seq: u64,
        next_until_seq: u64,
    ) -> AgentResult<()> {
        self.0
            .advance_memory_progress(session_id, expected_until_seq, next_until_seq)
            .await
    }
}

/// Recover only executions from a previous process after the host proved sole
/// namespace ownership. No input is reissued and paused continuations stay intact.
pub(crate) async fn recover_interrupted(store: &WorkerExecutionStore) -> AgentResult<()> {
    loop {
        let interrupted = store.interrupted_executions().await?;
        if interrupted.is_empty() {
            return Ok(());
        }
        for (session, identity, checkpoint) in interrupted {
            let mut reducer =
                talos_agent::AgentEventReducer::from_checkpoint(identity.clone(), checkpoint)?;
            let at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock follows epoch")
                .as_secs() as i64;
            let mut events = reducer.close_open_lifecycle(
                talos_agent_api::AgentMessageStatus::Cancelled,
                talos_agent_api::AgentMessageBlockStatus::Cancelled,
                at,
            )?;
            let failure = TalosError::new(
                ErrorCode::Cancelled,
                "Worker stopped before this Turn completed.",
            );
            let event = talos_agent_api::AgentEvent {
                identity,
                sequence: reducer.next_sequence(),
                occurred_at: at,
                payload: AgentEventPayload::TurnFailed(talos_agent_api::AgentTurnFailed {
                    error: failure.to_api_error(),
                    retryable: true,
                }),
            };
            reducer.apply(event.clone())?;
            events.push(event);
            store
                .append(
                    &session,
                    events,
                    &reducer.checkpoint(),
                    Some(agent_protocol::AgentExecutionSettlement::Finished {
                        memory_usage: Vec::new(),
                        metrics: reducer.checkpoint().metrics.clone(),
                    }),
                )
                .await?;
        }
    }
}

#[cfg(test)]
#[path = "execution_history_tests.rs"]
pub(crate) mod tests;
