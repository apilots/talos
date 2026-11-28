use super::*;

#[tokio::test]
async fn dream_usage_replays_after_restart_and_is_isolated_by_session_generation() {
    use agent_protocol::{DreamMemoryFormation, DreamMemoryParams};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("worker.sqlite");
    let store = WorkerExecutionStore::open(&path).await.unwrap();
    assert!(store
        .read_dream_formation(&DreamMemoryParams {
            session: session(),
            after_cursor: None
        })
        .await
        .unwrap_err()
        .is_code(ErrorCode::SessionNotFound));
    admit(&store).await;
    let params = DreamMemoryParams {
        session: session(),
        after_cursor: None,
    };
    assert!(store
        .read_dream_formation(&params)
        .await
        .unwrap_err()
        .is_code(ErrorCode::ActiveTurnRunning));
    store
        .append(
            &session(),
            vec![event(
                2,
                AgentEventPayload::TurnCompleted(talos_agent_api::AgentTurnCompleted {
                    stop_reason: talos_agent_api::AgentStopReason::EndTurn,
                }),
            )],
            &checkpoint(2),
            Some(AgentExecutionSettlement::Finished {
                memory_usage: vec![],
                metrics: Default::default(),
            }),
        )
        .await
        .unwrap();
    let formation = DreamMemoryFormation {
        covers_until_seq: None,
        model_usage: agent_protocol::AgentMemoryModelUsage {
            request_id: "stable-worker-call".into(),
            model_ref: talos_types::ModelRef {
                provider: "worker".into(),
                id: "inner".into(),
            },
            usage: talos_types::TokenUsage {
                input_tokens: 2,
                output_tokens: 1,
                total_tokens: 3,
                ..Default::default()
            },
            completed_at: 100,
            timezone: "Asia/Shanghai".into(),
        },
        failure: Some(
            TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Formation interrupted after model usage.",
            )
            .to_api_error(),
        ),
    };
    let cursor = store
        .append_dream_formation(&session(), &formation)
        .await
        .unwrap();
    // Simulate a crash after local progress commits but before the receipt is finalized.
    store
        .advance_memory_progress("session", 0, 2)
        .await
        .unwrap();
    store.pool.close().await;
    let reopened = WorkerExecutionStore::open(&path).await.unwrap();
    let replay = reopened.read_dream_formation(&params).await.unwrap();
    assert_eq!(
        replay,
        agent_protocol::DreamMemoryResult {
            processed_until_seq: 2,
            formation: Some(formation),
            next_cursor: cursor.clone()
        }
    );
    let drained = reopened
        .read_dream_formation(&DreamMemoryParams {
            session: session(),
            after_cursor: Some(cursor.clone()),
        })
        .await
        .unwrap();
    assert!(drained.formation.is_none());
    assert_eq!(drained.processed_until_seq, 2);
    let mut other = params.clone();
    other.session.workspace.user_id = "other-user".into();
    assert!(reopened
        .read_dream_formation(&other)
        .await
        .unwrap_err()
        .is_code(ErrorCode::PermissionDenied));
    reopened.clear(&session()).await.unwrap();
    assert!(reopened
        .read_dream_formation(&DreamMemoryParams {
            session: session(),
            after_cursor: Some(cursor)
        })
        .await
        .unwrap_err()
        .is_code(ErrorCode::EventCursorExpired));
    assert!(reopened
        .read_dream_formation(&params)
        .await
        .unwrap()
        .formation
        .is_none());
}
