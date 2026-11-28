use super::*;
#[tokio::test]
async fn title_result_replays_after_restart_and_rejects_other_scope_or_generation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("worker.sqlite");
    let store = WorkerExecutionStore::open(&path).await.unwrap();
    admit(&store).await;
    let params = agent_protocol::AgentSessionTitleParams {
        mode: agent_protocol::AgentSessionTitleMode::Generate,
        session: session(),
        identity: talos_agent_api::AgentTurnRef::new("session", "turn"),
    };
    assert!(store
        .read_session_title(&params)
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    let AgentExecutionCommand::Input { params: input } = command() else {
        unreachable!()
    };
    let mut message = input.input;
    message.blocks.push(talos_agent_api::AgentMessageBlock {
        block_id: "title-input".into(),
        status: talos_agent_api::AgentMessageBlockStatus::Completed,
        content: talos_agent_api::AgentMessageBlockContent::UserContent(
            talos_agent_api::AgentUserContent::Text {
                text: "\u{754c}".repeat(100_000),
            },
        ),
        started_at: 1,
        completed_at: Some(1),
    });
    store
        .append(
            &session(),
            vec![event(2, AgentEventPayload::MessageCompleted(message))],
            &checkpoint(2),
            None,
        )
        .await
        .unwrap();
    store
        .append(
            &session(),
            vec![event(
                3,
                AgentEventPayload::TurnCompleted(talos_agent_api::AgentTurnCompleted {
                    stop_reason: talos_agent_api::AgentStopReason::EndTurn,
                }),
            )],
            &checkpoint(3),
            Some(AgentExecutionSettlement::Finished {
                memory_usage: vec![],
                metrics: Default::default(),
            }),
        )
        .await
        .unwrap();
    let text = store.session_title_text(&params).await.unwrap();
    assert_eq!(text.user_message, "\u{754c}".repeat(400));
    assert!(text.assistant_response.is_empty());
    let result = agent_protocol::AgentSessionTitleResult {
        outcome: agent_protocol::AgentSessionTitleOutcome::Generated {
            title: "Local Worker title".into(),
        },
        model_usage: Some(agent_protocol::AgentMemoryModelUsage {
            request_id: "worker-title-call".into(),
            model_ref: talos_types::ModelRef {
                provider: "target".into(),
                id: "inner".into(),
            },
            usage: talos_types::TokenUsage {
                input_tokens: 2,
                output_tokens: 1,
                total_tokens: 3,
                ..Default::default()
            },
            completed_at: 42,
            timezone: "Asia/Shanghai".into(),
        }),
    };
    {
        let mut writer = store.pool.begin().await.unwrap();
        sqlx::query("UPDATE sessions SET generation=generation WHERE session_id=?")
            .bind("session")
            .execute(&mut *writer)
            .await
            .unwrap();
        let save = store.save_session_title(&params, &result);
        tokio::pin!(save);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), &mut save)
                .await
                .is_err(),
            "A concurrent writer must serialize before title binding reads"
        );
        writer.commit().await.unwrap();
        save.await.unwrap();
    }
    store.pool.close().await;
    let store = WorkerExecutionStore::open(&path).await.unwrap();
    assert_eq!(
        store.read_session_title(&params).await.unwrap(),
        Some(result)
    );
    let mut other = params.clone();
    other.session.workspace.user_id = "other".into();
    assert!(store
        .read_session_title(&other)
        .await
        .unwrap_err()
        .is_code(ErrorCode::PermissionDenied));
    store.clear(&session()).await.unwrap();
    assert!(store
        .read_session_title(&params)
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
}
