use std::sync::atomic::Ordering;

use crate::{
    AgentCapabilities, AgentErrorKind, AgentInputParams, AgentInterruptParams, AgentRunSettlement,
};
use talos_agent_api::{
    AgentEvent, AgentEventPayload, AgentMessageBlock, AgentMessageBlockContent,
    AgentMessageBlockStatus, AgentTurnRef, AgentUserContent,
};

use super::test_support::{run_params, user_message, FakeMode, FakeProvider, TEST_AGENT_ID};
use super::*;

#[tokio::test]
async fn start_waits_for_durable_admission_permit() {
    let provider = FakeProvider::new(FakeMode::Release);
    let harness = Harness::new(TEST_AGENT_ID.to_string(), provider.clone());
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let execution = harness
        .start(run_params(identity.clone(), "message-1"))
        .await
        .expect("admit run");

    assert!(!provider.started.load(Ordering::Acquire));
    assert_eq!(
        execution
            .admission
            .events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    execution
        .admission
        .permit
        .consume()
        .await
        .expect("start Provider");
    provider.wait_started().await;
    provider.release.notify_waiters();

    let settlement = execution.settlement.recv().await.expect("settle run");
    assert!(matches!(settlement, AgentRunSettlement::Finished { .. }));
    assert!(!harness.inner.active.contains_key(&identity.session_id));
}

#[tokio::test]
async fn provider_failure_closes_open_output_before_terminal_settlement() {
    let harness = Harness::new(
        TEST_AGENT_ID.to_string(),
        FakeProvider::new(FakeMode::OpenThenFail),
    );
    let identity = AgentTurnRef::new("session-1", "turn-open-failure");
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
    let mut progress = execution.events;
    let mut payloads = Vec::new();
    while let Some(event) = progress.recv().await {
        payloads.push(event.payload);
    }
    let settlement = execution
        .settlement
        .recv()
        .await
        .expect("failed Provider must settle canonically");

    assert!(matches!(
        payloads.as_slice(),
        [
            AgentEventPayload::MessageStarted(_),
            AgentEventPayload::MessageBlockStarted(_),
            AgentEventPayload::MessageBlockCompleted(completed),
            AgentEventPayload::MessageCompleted(message),
        ] if completed.block.status == talos_agent_api::AgentMessageBlockStatus::Failed
            && message.status == talos_agent_api::AgentMessageStatus::Failed
    ));
    assert!(matches!(
        settlement,
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
async fn fresh_rich_input_is_rejected_before_provider_admission() {
    let provider = FakeProvider::new(FakeMode::Release);
    let harness = Harness::new(TEST_AGENT_ID.to_string(), provider.clone());
    let identity = AgentTurnRef::new("session-1", "turn-rich-input");
    let mut params = run_params(identity, "message-1");
    params.input.blocks.push(AgentMessageBlock {
        block_id: "block-file".to_string(),
        status: AgentMessageBlockStatus::Completed,
        content: AgentMessageBlockContent::UserContent(AgentUserContent::File {
            file_id: "file-1".to_string(),
        }),
        started_at: 1,
        completed_at: Some(1),
    });

    let error = match harness.start(params).await {
        Ok(_) => panic!("undeclared rich input must fail before Provider admission"),
        Err(error) => error,
    };

    assert_eq!(error.kind, AgentErrorKind::CapabilityUnsupported);
    assert!(!provider.started.load(Ordering::Acquire));
}

#[tokio::test]
async fn running_input_requires_dynamic_input_before_delivery() {
    let provider = FakeProvider::new(FakeMode::Input).with_capabilities(AgentCapabilities {
        durable_continuation: true,
        ..AgentCapabilities::default()
    });
    let harness = Harness::new(TEST_AGENT_ID.to_string(), provider.clone());
    let identity = AgentTurnRef::new("session-1", "turn-dynamic-input");
    let execution = harness
        .start(run_params(identity.clone(), "message-1"))
        .await
        .expect("baseline Turn should be admitted");
    execution
        .admission
        .permit
        .consume()
        .await
        .expect("start Provider");
    provider.wait_started().await;

    let error = match harness
        .submit_input(AgentInputParams {
            agent_id: TEST_AGENT_ID.to_string(),
            identity: identity.clone(),
            input: user_message(&identity, "message-2"),
        })
        .await
    {
        Ok(_) => panic!("undeclared dynamic input must be rejected"),
        Err(error) => error,
    };
    harness
        .interrupt(AgentInterruptParams {
            agent_id: TEST_AGENT_ID.to_string(),
            identity,
        })
        .await
        .expect("test invocation should interrupt");
    execution
        .settlement
        .recv()
        .await
        .expect("interrupted invocation should settle");

    assert_eq!(error.kind, AgentErrorKind::CapabilityUnsupported);
    assert!(!provider.input_received.load(Ordering::Acquire));
}

#[tokio::test]
async fn interrupt_wins_over_pending_input_delivery_failure() {
    let provider = FakeProvider::new(FakeMode::PendingInput);
    let harness = Harness::new(TEST_AGENT_ID.to_string(), provider.clone());
    let identity = AgentTurnRef::new("session-1", "turn-pending-input");
    let execution = harness
        .start(run_params(identity.clone(), "message-1"))
        .await
        .expect("baseline Turn should be admitted");
    execution
        .admission
        .permit
        .consume()
        .await
        .expect("start Provider");
    provider.wait_started().await;

    let admission = harness
        .submit_input(AgentInputParams {
            agent_id: TEST_AGENT_ID.to_string(),
            identity: identity.clone(),
            input: user_message(&identity, "message-2"),
        })
        .await
        .expect("pending input should be admitted");
    let delivery = tokio::spawn(admission.permit.consume());
    while !provider.input_received.load(Ordering::Acquire) {
        tokio::task::yield_now().await;
    }

    harness
        .interrupt(AgentInterruptParams {
            agent_id: TEST_AGENT_ID.to_string(),
            identity,
        })
        .await
        .expect("test invocation should interrupt");
    let settlement = execution
        .settlement
        .recv()
        .await
        .expect("interrupted invocation should settle");
    let _ = delivery.await;

    assert!(matches!(
        settlement,
        AgentRunSettlement::Finished {
            event: AgentEvent {
                payload: AgentEventPayload::TurnFailed(ref failure),
                ..
            },
            ..
        } if failure.error.code == talos_error::ErrorCode::Cancelled
    ));
}

#[tokio::test]
async fn waiting_result_requires_durable_continuation_capability() {
    let provider = FakeProvider::new(FakeMode::Waiting).with_capabilities(AgentCapabilities {
        dynamic_input: true,
        ..AgentCapabilities::default()
    });
    let harness = Harness::new(TEST_AGENT_ID.to_string(), provider);
    let identity = AgentTurnRef::new("session-1", "turn-waiting-capability");
    let execution = harness
        .start(run_params(identity, "message-1"))
        .await
        .expect("baseline Turn should be admitted");
    execution
        .admission
        .permit
        .consume()
        .await
        .expect("start Provider");

    let settlement = execution
        .settlement
        .recv()
        .await
        .expect("unsupported waiting result must settle as failure");

    assert!(matches!(
        settlement,
        AgentRunSettlement::Finished {
            event: AgentEvent {
                payload: AgentEventPayload::TurnFailed(_),
                ..
            },
            ..
        }
    ));
}
