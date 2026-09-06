use talos_agent_api::{
    AgentEvent, AgentEventPayload, AgentInterruptOutcome, AgentMessageBlockStatus,
    AgentMessageStatus, AgentStopReason, AgentTurnCompleted, AgentTurnFailed,
};
use talos_run_control::{CleanupReport, Settlement, SettlementKind, SpawnError};

use crate::{
    event_sink::AgentEventPipeline, AgentError, AgentResult, AgentRunSettlement, SessionTurnResult,
};

pub(super) async fn settle_provider_completion(
    pipeline: &AgentEventPipeline,
    completion: SessionTurnResult,
) -> AgentResult<AgentRunSettlement> {
    match completion {
        SessionTurnResult::Completed { stop_reason } => {
            settle_completed(pipeline, stop_reason).await
        }
        SessionTurnResult::WaitingForInput {
            pending,
            continuation,
        } => {
            let event = pipeline
                .record(AgentEventPayload::WaitingForInput(*pending))
                .await?;
            Ok(AgentRunSettlement::WaitingForInput {
                event,
                continuation,
                metrics: pipeline.checkpoint().await.metrics,
            })
        }
    }
}

pub(super) async fn settle_completed(
    pipeline: &AgentEventPipeline,
    stop_reason: AgentStopReason,
) -> AgentResult<AgentRunSettlement> {
    let event = pipeline
        .record(AgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason,
        }))
        .await?;
    finished_settlement(pipeline, event, Ok(())).await
}

pub(super) async fn settle_failure(
    pipeline: &AgentEventPipeline,
    error: AgentError,
) -> AgentResult<AgentRunSettlement> {
    let cancelled = error.error().is_code(talos_error::ErrorCode::Cancelled);
    pipeline
        .close_open_lifecycle(
            if cancelled {
                AgentMessageStatus::Cancelled
            } else {
                AgentMessageStatus::Failed
            },
            if cancelled {
                AgentMessageBlockStatus::Cancelled
            } else {
                AgentMessageBlockStatus::Failed
            },
        )
        .await
        .inspect_err(|_| talos_logging::report_error(error.error()))?;
    let product_error = error.clone().into_talos_error();
    let event = pipeline
        .record(AgentEventPayload::TurnFailed(AgentTurnFailed {
            error: product_error.to_api_error(),
            retryable: error.retryable,
        }))
        .await
        .inspect_err(|_| talos_logging::report_error(error.error()))?;
    finished_settlement(pipeline, event, Err(product_error)).await
}

async fn finished_settlement(
    pipeline: &AgentEventPipeline,
    event: AgentEvent,
    result: talos_error::TalosResult<()>,
) -> AgentResult<AgentRunSettlement> {
    let checkpoint = pipeline.checkpoint().await;
    Ok(AgentRunSettlement::Finished {
        event,
        messages: checkpoint.messages,
        metrics: checkpoint.metrics,
        result,
    })
}

pub(super) fn cleanup_confirmed(settlement: &Settlement) -> bool {
    matches!(settlement.cleanup, CleanupReport::Confirmed)
}

pub(super) fn cleanup_task_count(settlement: &Settlement) -> u32 {
    match &settlement.cleanup {
        CleanupReport::Confirmed => 0,
        CleanupReport::Unconfirmed { count, .. } => u32::try_from(*count).unwrap_or(u32::MAX),
    }
}

pub(super) fn cleanup_error(settlement: &Settlement) -> AgentError {
    let count = match &settlement.cleanup {
        CleanupReport::Confirmed => 0,
        CleanupReport::Unconfirmed { count, .. } => *count,
    };
    AgentError::new(
        talos_error::ErrorCode::RunCleanupUnconfirmed,
        format!("Agent Provider cleanup is unconfirmed for {count} task(s)"),
    )
}

pub(super) fn interrupt_outcome(settlement: &Settlement) -> AgentInterruptOutcome {
    match (&settlement.kind, &settlement.cleanup) {
        (_, CleanupReport::Unconfirmed { .. }) => AgentInterruptOutcome::CleanupUnconfirmed {
            task_count: cleanup_task_count(settlement),
        },
        (SettlementKind::Completed, CleanupReport::Confirmed) => AgentInterruptOutcome::Completed,
        (SettlementKind::Cancelled(_), CleanupReport::Confirmed) => {
            AgentInterruptOutcome::Cancelled
        }
    }
}

pub(super) fn spawn_error(error: SpawnError) -> AgentError {
    let message = match error {
        SpawnError::Closed => "Agent run control stopped accepting Provider tasks",
        SpawnError::CapacityExceeded => "Agent run control reached its Provider task capacity",
    };
    AgentError::provider_error(message)
}
