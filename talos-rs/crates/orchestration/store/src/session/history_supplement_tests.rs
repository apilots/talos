use super::*;
use crate::test_support::{ProjectTestWorkerEvents, TestWorkerEvents};
use talos_agent_api::trajectory::*;
use talos_error::ErrorCode;

fn supplement(name: &str) -> SessionHistorySupplement {
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let prompt_id = format!("prompt-{name}");
    let tools_id = format!("tools-{name}");
    SessionHistorySupplement {
        identity: identity.clone(),
        clock_segments: vec![],
        requests: vec![TrajectoryReadModelRequestResponse {
            identity,
            request: ModelRequestTrace {
                model_request_id: name.into(),
                ordinal: 1,
                purpose: ModelRequestPurpose::Working,
                connection_id: "main".into(),
                model_id: "model".into(),
                call_records: vec![],
            },
            prompt: Some(PayloadRef {
                payload_id: prompt_id.clone(),
                complete: Some(true),
            }),
            tools: Some(PayloadRef {
                payload_id: tools_id.clone(),
                complete: Some(true),
            }),
            parameters: None,
            outputs: vec![],
        }],
        tools: vec![],
        payloads: [
            (
                prompt_id,
                TrajectoryPayload::Prompt {
                    messages: vec![SnapshotMessage {
                        index: 0,
                        role: SnapshotRole::User,
                        content: name.into(),
                        reasoning: None,
                        name: None,
                        tool_call_id: None,
                        tool_calls: None,
                        attachments: None,
                        cache_control: None,
                        input_origin: None,
                    }],
                },
            ),
            (
                tools_id,
                TrajectoryPayload::Tools {
                    definitions: vec![],
                },
            ),
        ]
        .into(),
    }
}
async fn finish(store: &TestStore, sequence: u64) {
    store
        .project_test_events(TestWorkerEvents::new(vec![event(
            sequence,
            AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                stop_reason: AgentStopReason::EndTurn,
            }),
        )]))
        .await
        .unwrap();
}

#[tokio::test]
async fn sealed_worker_metadata_preserves_waiting_history_and_rejects_changed_replay() {
    let store = store().await;
    store
        .project_test_events(initial_admission(admission()))
        .await
        .unwrap();
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let before = store.list_agent_turn_events(&identity, 10).await.unwrap();
    assert!(store
        .read_terminal_history_supplement(&identity)
        .await
        .unwrap_err()
        .is_code(ErrorCode::ResourceNotFound));
    store
        .project_test_events(TestWorkerEvents::new(vec![waiting(3, "question")]))
        .await
        .unwrap();
    assert_eq!(
        store.list_agent_turn_events(&identity, 10).await.unwrap()[..before.len()],
        before
    );
    let mut sealed = supplement("first");
    let mut resumed = supplement("resumed");
    resumed.requests[0].request.ordinal = 2;
    resumed.requests[0].tools = sealed.requests[0].tools.clone();
    sealed.requests.extend(resumed.requests);
    sealed.payloads.clear();
    let source = store.worker_event_cursor("session-1").await.unwrap();
    let terminal = event(
        4,
        AgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    );
    store
        .project_test_events(TestWorkerEvents {
            history_supplement: Some(sealed.clone()),
            ..TestWorkerEvents::new(vec![terminal.clone()])
        })
        .await
        .unwrap();
    let saved = store
        .read_terminal_history_supplement(&identity)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved, sealed);
    assert_eq!(
        saved
            .requests
            .iter()
            .map(|request| request.request.ordinal)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(saved.payloads.is_empty());
    assert_eq!(saved.requests[0].tools, saved.requests[1].tools);
    let replay = store
        .project_test_events(TestWorkerEvents {
            history_supplement: Some(sealed.clone()),
            after_cursor: Some(source.clone()),
            ..TestWorkerEvents::new(vec![terminal.clone()])
        })
        .await
        .unwrap();
    assert!(replay.events.is_empty());
    sealed.requests[0].request.connection_id = "changed-after-commit".into();
    assert!(store
        .project_test_events(TestWorkerEvents {
            history_supplement: Some(sealed),
            after_cursor: Some(source),
            ..TestWorkerEvents::new(vec![terminal])
        })
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    for receipt in store.pending_worker_settlements("session-1").await.unwrap() {
        store
            .complete_worker_settlement(&receipt.cursor)
            .await
            .unwrap();
    }
    store.clear_session_history("session-1", 20).await.unwrap();
    assert!(store
        .read_terminal_history_supplement(&identity)
        .await
        .unwrap_err()
        .is_code(ErrorCode::ResourceNotFound));
}

#[tokio::test]
async fn old_null_content_is_distinct_from_corrupt_saved_content() {
    let store = store().await;
    let _admitted = store
        .project_test_events(initial_admission(admission()))
        .await
        .unwrap();
    finish(&store, 3).await;
    let identity = AgentTurnRef::new("session-1", "turn-1");
    assert_eq!(
        store
            .read_terminal_history_supplement(&identity)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        store
            .read_execution_payload(&identity, "missing")
            .await
            .unwrap(),
        None
    );
    let mut legacy = serde_json::to_value(supplement("legacy")).unwrap();
    legacy["payloads"]["prompt-legacy"]["messages"][0]
        .as_object_mut()
        .unwrap()
        .remove("reasoning");
    store
        .storage
        .execute(
            "UPDATE agent_events SET execution_capture_json=? WHERE sequence=1",
            &[&legacy.to_string()],
        )
        .await
        .unwrap();
    let body = store
        .read_execution_payload(&identity, "prompt-legacy")
        .await
        .unwrap()
        .unwrap();
    // Validating old saved input must not rewrite it with today's default fields.
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap(),
        legacy["payloads"]["prompt-legacy"]
    );
    legacy["payloads"]["prompt-legacy"]["messages"] = serde_json::Value::Null;
    store
        .storage
        .execute(
            "UPDATE agent_events SET execution_capture_json=? WHERE sequence=1",
            &[&legacy.to_string()],
        )
        .await
        .unwrap();
    assert!(store
        .read_execution_payload(&identity, "prompt-legacy")
        .await
        .unwrap_err()
        .is_code(ErrorCode::InternalError));
    store
        .storage
        .execute(
            "UPDATE agent_events SET execution_capture_json='{broken' WHERE sequence=1",
            &[],
        )
        .await
        .unwrap();
    assert!(store
        .read_terminal_history_supplement(&identity)
        .await
        .unwrap_err()
        .is_code(ErrorCode::InternalError));
}

#[tokio::test]
async fn invalid_worker_capture_rolls_back_the_terminal_projection_atomically() {
    let store = store().await;
    store
        .project_test_events(initial_admission(admission()))
        .await
        .unwrap();
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let before = store.list_agent_turn_events(&identity, 10).await.unwrap();
    let source = store.worker_event_cursor("session-1").await.unwrap();
    let terminal = event(
        3,
        AgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    );
    let mut wrong_identity = supplement("wrong");
    wrong_identity.payloads.clear();
    wrong_identity.identity.turn_id = "another-turn".into();
    for invalid in [wrong_identity, supplement("embedded-body")] {
        assert!(store
            .project_test_events(TestWorkerEvents {
                history_supplement: Some(invalid),
                ..TestWorkerEvents::new(vec![terminal.clone()])
            })
            .await
            .unwrap_err()
            .is_code(ErrorCode::Conflict));
        assert_eq!(
            store.list_agent_turn_events(&identity, 10).await.unwrap(),
            before
        );
        assert_eq!(
            store.worker_event_cursor("session-1").await.unwrap(),
            source
        );
    }
    assert!(store
        .get_agent_active_turn("session-1")
        .await
        .unwrap()
        .is_some());
}
