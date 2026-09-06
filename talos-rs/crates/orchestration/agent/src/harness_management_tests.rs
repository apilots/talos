use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::{
    AgentInputParams, AgentInterruptParams, AgentRunSettlement, AgentRunSettlementReceiver,
};
use talos_agent_api::{AgentEvent, AgentEventPayload, AgentInterruptOutcome, AgentTurnRef};

use super::test_support::{
    context, run_params, session_params, user_message, FakeMode, FakeProvider, TEST_AGENT_ID,
};
use super::*;

#[tokio::test]
async fn input_admission_blocks_delivery_until_permit() {
    let provider = FakeProvider::new(FakeMode::Input);
    let harness = Harness::new(TEST_AGENT_ID.to_string(), provider.clone());
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let settlement = start_execution(&harness, &provider, identity.clone()).await;

    let input = harness
        .submit_input(AgentInputParams {
            agent_id: TEST_AGENT_ID.to_string(),
            identity: identity.clone(),
            input: user_message(&identity, "message-2"),
        })
        .await
        .expect("admit input");
    assert!(!provider.input_received.load(Ordering::Acquire));
    assert_eq!(
        input
            .events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![4, 5]
    );

    input.permit.consume().await.expect("deliver input");
    assert!(provider.input_received.load(Ordering::Acquire));
    assert!(matches!(
        settlement.recv().await.expect("settle run"),
        AgentRunSettlement::Finished { .. }
    ));
}

#[tokio::test]
async fn rejected_input_keeps_its_cause_after_provider_cleanup() {
    let provider = FakeProvider::new(FakeMode::RejectInput);
    let harness = Harness::new(TEST_AGENT_ID.to_string(), provider.clone());
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let settlement = start_execution(&harness, &provider, identity.clone()).await;

    let delivery_error = harness
        .submit_input(AgentInputParams {
            agent_id: TEST_AGENT_ID.to_string(),
            identity,
            input: user_message(&AgentTurnRef::new("session-1", "turn-1"), "message-2"),
        })
        .await
        .expect("admit input")
        .permit
        .consume()
        .await
        .expect_err("Provider rejection should be reported to the delivery owner");
    assert_eq!(
        delivery_error.error().error_code(),
        talos_error::ErrorCode::InvalidRequest
    );

    assert!(matches!(
        settlement.recv().await.expect("settle rejected input"),
        AgentRunSettlement::Finished {
            event: AgentEvent {
                payload: AgentEventPayload::TurnFailed(ref failure),
                ..
            },
            ..
        } if failure.error.code == talos_error::ErrorCode::InvalidRequest
    ));
}

#[tokio::test]
async fn interrupt_targets_one_exact_live_turn() {
    let provider = FakeProvider::new(FakeMode::Release);
    let harness = Harness::new(TEST_AGENT_ID.to_string(), provider.clone());
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let settlement = start_execution(&harness, &provider, identity.clone()).await;

    assert!(harness
        .interrupt(AgentInterruptParams {
            agent_id: TEST_AGENT_ID.to_string(),
            identity: AgentTurnRef::new("session-1", "turn-wrong"),
        })
        .await
        .is_err());
    assert_eq!(
        harness
            .interrupt(AgentInterruptParams {
                agent_id: TEST_AGENT_ID.to_string(),
                identity,
            })
            .await
            .expect("interrupt exact Turn"),
        AgentInterruptOutcome::Cancelled
    );
    assert!(matches!(
        settlement.recv().await.expect("settle interrupt"),
        AgentRunSettlement::Finished {
            event: AgentEvent {
                payload: AgentEventPayload::TurnFailed(_),
                ..
            },
            ..
        }
    ));
}

#[tokio::test]
async fn waiting_settlement_returns_usage_and_releases_live_slot() {
    let provider = FakeProvider::new(FakeMode::Waiting);
    let harness = Harness::new(TEST_AGENT_ID.to_string(), provider);
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let execution = harness
        .start(run_params(identity.clone(), "message-1"))
        .await
        .expect("admit run");
    execution
        .admission
        .permit
        .consume()
        .await
        .expect("start Provider");

    assert!(matches!(
        execution.settlement.recv().await.expect("settle waiting"),
        AgentRunSettlement::WaitingForInput { metrics, .. } if metrics.usage.total_tokens == 3
    ));
    assert!(!harness.inner.active.contains_key(&identity.session_id));
}

#[tokio::test]
async fn compaction_reuses_run_execution_and_returns_usage_in_metrics() {
    let harness = Harness::new(
        TEST_AGENT_ID.to_string(),
        FakeProvider::new(FakeMode::Release),
    );
    let mut execution = harness
        .compact_session(context(AgentTurnRef::new("session-1", "turn-compact-1")))
        .await
        .expect("admit compaction");
    execution
        .admission
        .permit
        .consume()
        .await
        .expect("start compaction");

    assert!(matches!(
        execution
            .events
            .recv()
            .await
            .expect("compaction metrics")
            .payload,
        AgentEventPayload::MetricsUpdated(_)
    ));
    let AgentRunSettlement::Finished { metrics, .. } = execution
        .settlement
        .recv()
        .await
        .expect("settle compaction")
    else {
        panic!("compaction must finish")
    };
    assert_eq!(metrics.usage.total_tokens, 3);
}

#[tokio::test]
async fn session_maintenance_conflicts_with_a_live_turn() {
    let provider = FakeProvider::new(FakeMode::Release);
    let harness = Harness::new(TEST_AGENT_ID.to_string(), provider.clone());
    let settlement = start_execution(
        &harness,
        &provider,
        AgentTurnRef::new("session-1", "turn-1"),
    )
    .await;

    assert_eq!(
        harness
            .clear_session(session_params())
            .await
            .expect_err("clear must conflict with live Turn")
            .error()
            .error_code(),
        talos_error::ErrorCode::ActiveTurnRunning
    );
    provider.release.notify_waiters();
    settlement.recv().await.expect("settle run");
    harness
        .clear_session(session_params())
        .await
        .expect("clear idle Session");
    harness
        .close_session(session_params())
        .await
        .expect("close idle Session");
    assert!(provider.clear_called.load(Ordering::Acquire));
}

#[tokio::test]
async fn failed_session_maintenance_releases_its_reservation() {
    let provider = FakeProvider::new(FakeMode::Release).with_clear_failure();
    let harness = Harness::new(TEST_AGENT_ID.to_string(), provider);

    harness
        .clear_session(session_params())
        .await
        .expect_err("fake clear must fail");

    let identity = AgentTurnRef::new("session-1", "turn-after-clear");
    let execution = harness
        .start(run_params(identity, "message-after-clear"))
        .await
        .expect("failed maintenance must release its reservation");
    drop(execution);
}

#[tokio::test]
async fn failed_session_acquisition_retains_unconfirmed_resources() {
    let harness = Harness::new(
        TEST_AGENT_ID.to_string(),
        FakeProvider::new(FakeMode::UnconfirmedAcquisition),
    );
    let identity = AgentTurnRef::new("session-1", "turn-cleanup-pending");
    let execution = harness
        .start(run_params(identity.clone(), "message-1"))
        .await
        .unwrap();
    execution.admission.permit.consume().await.unwrap();
    assert_eq!(
        execution
            .settlement
            .recv()
            .await
            .unwrap_err()
            .error()
            .error_code(),
        talos_error::ErrorCode::RunCleanupUnconfirmed
    );

    assert_eq!(
        harness
            .interrupt(AgentInterruptParams {
                agent_id: TEST_AGENT_ID.to_string(),
                identity,
            })
            .await
            .expect("read cleanup state"),
        AgentInterruptOutcome::CleanupUnconfirmed { task_count: 1 }
    );
    assert!(harness
        .start(run_params(
            AgentTurnRef::new("session-1", "turn-2"),
            "message-2"
        ))
        .await
        .is_err());
    assert!(harness.close_session(session_params()).await.is_err());
}

#[tokio::test]
async fn failed_session_acquisition_releases_confirmed_resources() {
    let harness = Harness::new(
        TEST_AGENT_ID.to_string(),
        FakeProvider::new(FakeMode::FailAcquisition),
    );
    let identity = AgentTurnRef::new("session-1", "turn-acquisition-failure");
    let execution = harness
        .start(run_params(identity, "message-1"))
        .await
        .unwrap();
    execution.admission.permit.consume().await.unwrap();
    assert!(matches!(
        execution.settlement.recv().await.unwrap(),
        AgentRunSettlement::Finished { .. }
    ));
    let next = harness
        .start(run_params(
            AgentTurnRef::new("session-1", "turn-2"),
            "message-2",
        ))
        .await
        .unwrap();
    drop(next);
}

#[tokio::test]
async fn shutdown_closes_admission_before_provider_shutdown() {
    let provider = FakeProvider::new(FakeMode::Waiting);
    let harness = Harness::new(TEST_AGENT_ID.to_string(), provider.clone());
    let execution = harness
        .start(run_params(
            AgentTurnRef::new("session-1", "turn-1"),
            "message-1",
        ))
        .await
        .expect("admit run");
    execution
        .admission
        .permit
        .consume()
        .await
        .expect("start Provider");
    execution.settlement.recv().await.expect("settle run");

    harness
        .shutdown(Duration::from_secs(1))
        .await
        .expect("shutdown Harness");
    assert!(provider.close_called.load(Ordering::Acquire));
    assert!(provider.shutdown_called.load(Ordering::Acquire));
    assert!(harness
        .start(run_params(
            AgentTurnRef::new("session-2", "turn-2"),
            "message-2",
        ))
        .await
        .is_err());
}

#[tokio::test]
async fn shutdown_cancels_in_flight_session_acquisition() {
    let provider = FakeProvider::new(FakeMode::Release).with_blocked_session_acquisition();
    let harness = Harness::new(TEST_AGENT_ID.to_string(), provider.clone());
    let execution = harness
        .start(run_params(
            AgentTurnRef::new("session-1", "turn-1"),
            "message-1",
        ))
        .await
        .expect("admit run");
    execution
        .admission
        .permit
        .consume()
        .await
        .expect("start Provider");
    provider.wait_session_requested().await;

    harness
        .shutdown(Duration::from_secs(1))
        .await
        .expect("shutdown during Session acquisition");

    assert!(provider.shutdown_called.load(Ordering::Acquire));
    assert!(harness.inner.active.is_empty());
    assert!(harness.inner.sessions.is_empty());
}

#[tokio::test]
async fn shutdown_retains_cached_session_when_close_fails() {
    let provider = FakeProvider::new(FakeMode::Waiting).with_close_failure();
    let harness = Harness::new(TEST_AGENT_ID.to_string(), provider.clone());
    let execution = harness
        .start(run_params(
            AgentTurnRef::new("session-1", "turn-1"),
            "message-1",
        ))
        .await
        .expect("admit run");
    execution
        .admission
        .permit
        .consume()
        .await
        .expect("start Provider");
    execution.settlement.recv().await.expect("settle run");

    harness
        .shutdown(Duration::from_secs(1))
        .await
        .expect_err("failed Session close must fail shutdown");

    assert!(provider.close_called.load(Ordering::Acquire));
    assert!(!provider.shutdown_called.load(Ordering::Acquire));
    assert_eq!(harness.inner.sessions.len(), 1);
}

async fn start_execution(
    harness: &Harness<FakeProvider>,
    provider: &FakeProvider,
    identity: AgentTurnRef,
) -> AgentRunSettlementReceiver {
    let execution = harness
        .start(run_params(identity, "message-1"))
        .await
        .expect("admit run");
    execution
        .admission
        .permit
        .consume()
        .await
        .expect("start Provider");
    provider.wait_started().await;
    execution.settlement
}
