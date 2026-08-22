use super::*;
use talos_run_control::{CancellationMode, CleanupReport, Settlement, SettlementKind};

#[tokio::test]
async fn dropping_stream_receiver_does_not_cancel_server_run() {
    let started = Arc::new(Notify::new());
    let agent = test_agent_harness(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        static_router(Arc::new(SequencedProvider {
            started: started.clone(),
            calls: AtomicUsize::new(0),
        })),
        model_selection("gpt-4o"),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    )
    .expect("agent harness should build");
    let session = build_test_session("session-disconnected", "workspace-a");
    let turn_id = "turn-disconnected";

    let stream = agent
        .run_streamed(agent_run_params(
            session.clone(),
            turn_id,
            vec![TalosInputItem::Text {
                text: "keep running".to_string(),
            }],
        ))
        .await
        .expect("stream should start");
    started.notified().await;
    drop(stream);

    assert!(agent.has_active_turn(&session.context.session_id));
    assert_eq!(
        agent
            .interrupt(AgentInterruptParams {
                session_id: session.context.session_id,
                turn_id: turn_id.to_string(),
            })
            .await
            .expect("explicit interrupt should still own cancellation"),
        talos_types::CancelActiveTurnOutcome::Cancelled,
    );
}

#[test]
fn forced_settlement_maps_to_forced_abort() {
    let forced = terminal_error(run_terminal_disposition(&Settlement {
        kind: SettlementKind::Cancelled(CancellationMode::Forced),
        cleanup: CleanupReport::Unconfirmed {
            count: 1,
            failures: vec![talos_run_control::CleanupFailure::ForceAborted],
        },
    }))
    .expect("forced cancellation should produce a terminal error");

    assert_eq!(forced.error_code(), talos_error::ErrorCode::RunForcedAbort);
}

#[tokio::test]
async fn interrupt_reports_missing_runtime_state() {
    let agent = build_test_agent();

    let error = interrupt_session(&agent, "missing-session")
        .await
        .expect_err("missing active turn should be reported");

    assert_eq!(error.kind, crate::AgentErrorKind::RunNotFound);
}

#[tokio::test]
async fn interrupt_rejects_reserved_turn_without_runtime_handle() {
    let agent = build_test_agent();
    agent
        .try_reserve_active_turn("reserved-session")
        .expect("test should reserve active slot");

    let error = interrupt_session(&agent, "reserved-session")
        .await
        .expect_err("reserved turn cannot report a successful interrupt");

    assert_eq!(error.kind, crate::AgentErrorKind::RunNotFound);
}
