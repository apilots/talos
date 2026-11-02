use super::*;
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
async fn finish(store: &TestStore, sequence: u64, tail: i64) {
    store
        .commit_terminal_settlement(CommitTerminalSettlementParams {
            event: event(
                sequence,
                AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                    stop_reason: AgentStopReason::EndTurn,
                }),
            ),
            expected_event_seq: tail,
            completed_turn_metrics: None,
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn supplements_preserve_canonical_history_across_waiting_and_seal_at_terminal() {
    let store = store().await;
    store
        .insert_session_record(&session_record())
        .await
        .unwrap();
    let admitted = store
        .commit_initial_admission(initial_admission(admission()))
        .await
        .unwrap();
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let before = store.list_agent_turn_events(&identity, 10).await.unwrap();
    store
        .append_history_supplement(supplement("first"))
        .await
        .unwrap();
    assert_eq!(
        store.list_agent_turn_events(&identity, 10).await.unwrap(),
        before
    );
    assert!(store
        .read_terminal_history_supplement(&identity)
        .await
        .unwrap_err()
        .is_code(ErrorCode::ResourceNotFound));
    let paused = store
        .commit_waiting_settlement(
            waiting(3, "question"),
            AgentContinuationRef {
                opaque_id: "continuation".into(),
            },
            admitted.active_turn.last_event_seq,
        )
        .await
        .unwrap();
    store
        .append_history_supplement(supplement("resumed"))
        .await
        .unwrap();
    // Repeating a final checkpoint is idempotent, with the same frozen request.
    store
        .append_history_supplement(supplement("resumed"))
        .await
        .unwrap();
    finish(&store, 4, paused.active_turn.last_event_seq).await;
    let saved = store
        .read_terminal_history_supplement(&identity)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        saved
            .requests
            .iter()
            .map(|r| r.request.ordinal)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    // Metadata reads exclude bodies; exactly equal catalogs share their saved ID.
    assert!(saved.payloads.is_empty());
    let contents = store
        .storage
        .query(
            "SELECT execution_capture_json FROM agent_events WHERE sequence=1",
            &[],
        )
        .await
        .unwrap();
    let full: SessionHistorySupplement =
        serde_json::from_str(&contents[0].get_string("execution_capture_json").unwrap()).unwrap();
    assert_eq!(full.payloads.len(), 3);
    assert_eq!(saved.requests[0].tools, saved.requests[1].tools);
    assert!(store
        .append_history_supplement(supplement("late"))
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    store.clear_session_history("session-1", 20).await.unwrap();
    assert!(store
        .read_terminal_history_supplement(&identity)
        .await
        .unwrap_err()
        .is_code(ErrorCode::ResourceNotFound));
    assert!(store
        .append_history_supplement(supplement("deleted"))
        .await
        .is_err());
}

#[tokio::test]
async fn old_null_content_is_distinct_from_corrupt_saved_content() {
    let store = store().await;
    store
        .insert_session_record(&session_record())
        .await
        .unwrap();
    let admitted = store
        .commit_initial_admission(initial_admission(admission()))
        .await
        .unwrap();
    finish(&store, 3, admitted.active_turn.last_event_seq).await;
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
async fn optional_append_follows_pending_progress_and_keeps_invalid_links_atomic() {
    let store = store().await;
    store
        .insert_session_record(&session_record())
        .await
        .unwrap();
    store
        .commit_initial_admission(initial_admission(admission()))
        .await
        .unwrap();
    let tx = store.storage.begin_transaction().await.unwrap();
    let append = store.append_history_supplement(supplement("first"));
    tokio::pin!(append);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), &mut append)
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    append.await.unwrap();
    let before = store
        .storage
        .query(
            "SELECT execution_capture_json FROM agent_events WHERE sequence=1",
            &[],
        )
        .await
        .unwrap()[0]
        .get_string("execution_capture_json");
    let mut bad = supplement("wrong");
    bad.payloads.insert(
        "prompt-wrong".into(),
        TrajectoryPayload::Tools {
            definitions: vec![],
        },
    );
    assert!(store
        .append_history_supplement(bad)
        .await
        .unwrap_err()
        .is_code(ErrorCode::InvalidRequest));
    let after = store
        .storage
        .query(
            "SELECT execution_capture_json FROM agent_events WHERE sequence=1",
            &[],
        )
        .await
        .unwrap()[0]
        .get_string("execution_capture_json");
    assert_eq!(before, after);
}
