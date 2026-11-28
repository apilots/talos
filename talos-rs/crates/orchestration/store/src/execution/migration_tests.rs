use super::*;

#[tokio::test]
async fn version_two_upgrade_preserves_waiting_execution_and_command_replay() {
    use agent_protocol::{AgentCommandIdentity, AgentContext, AgentRunIntent, AgentRunParams};
    use talos_agent_api::{
        AgentMessageBlock, AgentMessageBlockContent, AgentMessageBlockStatus, AgentPendingInput,
        AgentUserContent,
    };
    use talos_types::{
        PendingUserAction, QuestionAnswer, QuestionParams, UserActionParams, UserActionResponse,
        UserActionState,
    };

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("execution.sqlite");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true)
                .foreign_keys(true),
        )
        .await
        .unwrap();
    let previous = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            MIGRATOR
                .iter()
                .filter(|migration| migration.version <= 2)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    previous.run(&pool).await.unwrap();
    let checksums: Vec<(i64, String)> = sqlx::query_as(
        "SELECT version, lower(hex(checksum)) FROM _sqlx_migrations ORDER BY version",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(checksums, vec![
        (1, "636c05db9edec445f35fd12c42047635cf44d919ec32d3284c3a8d1fae83802b3a34244685dc5c44593e49101e5d72d6".into()),
        (2, "183661fe375a2bc0b2721e7ae158ddc101ee0d5dd71de27a0ecfc56d22d5f2b22ddcab2b7f0e980d0da7761f6020135c".into()),
    ]);
    let AgentExecutionCommand::Input { params: input } = command() else {
        unreachable!()
    };
    let start = AgentRunParams {
        subagent_limit: None,
        command: Some(AgentCommandIdentity {
            command: talos_types::IdempotentCommand::TurnSubmission,
            idempotency_key: "start-key".into(),
            request_id: "start-request".into(),
        }),
        automation_origin: None,
        context: AgentContext {
            tree_identity: talos_types::SessionTreeIdentity::Primary,
            history_cursor: None,
            session: session(),
            turn_id: "turn".into(),
            project_id: None,
            working_directory: None,
            environment: None,
            model: talos_types::ModelInfo::from_provider_model(
                "test",
                serde_json::from_value(serde_json::json!({"id":"model"})).unwrap(),
            ),
            model_selection: talos_types::ModelSelection::without_reasoning(
                talos_types::ModelRef {
                    provider: "test".into(),
                    id: "model".into(),
                },
            ),
            permission_mode: talos_types::UserPermissionMode::FullAccess,
        },
        input: input.input,
        intent: AgentRunIntent::Start,
    };
    let mut response = start.clone();
    response.intent = AgentRunIntent::RespondUserAction;
    response.command.as_mut().unwrap().command = talos_types::IdempotentCommand::TurnInput;
    response.command.as_mut().unwrap().idempotency_key = "answer-key".into();
    response.command.as_mut().unwrap().request_id = "answer-request".into();
    response.input.message_id = "answer-1".into();
    response.input.blocks = vec![AgentMessageBlock {
        block_id: "answer-block".into(),
        status: AgentMessageBlockStatus::Completed,
        content: AgentMessageBlockContent::UserContent(AgentUserContent::UserActionResponse {
            action_id: "action-1".into(),
            response: UserActionResponse::Question(QuestionAnswer::FreeForm {
                text: "Continue".into(),
            }),
        }),
        started_at: 1,
        completed_at: Some(1),
    }];
    let pending = |action_id: &str| AgentPendingInput::UserAction {
        action: PendingUserAction {
            action_id: action_id.into(),
            call_id: action_id.into(),
            item_id: None,
            state: UserActionState::Pending,
            params: UserActionParams::Question(QuestionParams {
                header: "Continue".into(),
                question: "How should I continue?".into(),
                options: vec![],
            }),
        },
    };
    let continuation = talos_agent_api::AgentContinuationRef {
        opaque_id: "provider-state".into(),
    };
    let settlement = AgentExecutionSettlement::WaitingForInput {
        metrics: AgentRunMetrics::default(),
    };
    let legacy_settlement = serde_json::json!({"type":"waitingForInput","continuation":continuation,"metrics":AgentRunMetrics::default()});
    let events = vec![
        event(1, AgentEventPayload::TurnStarted(AgentTurnStarted {})),
        event(2, AgentEventPayload::WaitingForInput(pending("action-1"))),
        event(
            3,
            AgentEventPayload::MessageCompleted(response.input.clone()),
        ),
        event(4, AgentEventPayload::WaitingForInput(pending("action-2"))),
    ];
    let mut saved_checkpoint = checkpoint(4);
    saved_checkpoint.messages = vec![start.input.clone(), response.input.clone()];
    let (supplement, payload) = history_supplement();
    sqlx::query("INSERT INTO worker_sessions(session_id,binding_json,active_turn_id,memory_processed_until) VALUES ('session',?,'turn',8)")
        .bind(encode(&session()).unwrap()).execute(&pool).await.unwrap();
    for (offset, event) in (7_i64..).zip(&events) {
        sqlx::query("INSERT INTO worker_events(event_offset,session_id,generation,turn_id,sequence,event_json,settlement_json) VALUES (?,'session',0,'turn',?,?,?)")
            .bind(offset).bind(event.sequence as i64).bind(encode(event).unwrap())
            .bind(matches!(event.payload, AgentEventPayload::WaitingForInput(_)).then(|| legacy_settlement.to_string()))
            .execute(&pool).await.unwrap();
    }
    let start_command = AgentExecutionCommand::Run {
        params: Box::new(start.clone()),
    };
    let response_command = AgentExecutionCommand::Run {
        params: Box::new(response.clone()),
    };
    for (command, offset, continuation) in [
        (&start_command, 7_i64, None),
        (&response_command, 9, Some(&continuation)),
    ] {
        let mut legacy = serde_json::to_value(command).unwrap();
        let params = legacy["params"].as_object_mut().unwrap();
        params.remove("intent");
        params.insert(
            "checkpoint".into(),
            serde_json::to_value(continuation.map(|_| checkpoint(2))).unwrap(),
        );
        params.insert(
            "continuation".into(),
            serde_json::to_value(continuation).unwrap(),
        );
        sqlx::query("INSERT INTO worker_admissions(session_id,generation,turn_id,input_id,request_json,first_offset,last_offset) VALUES ('session',0,'turn',?,?,?,?)")
            .bind(command_key(command).unwrap()).bind(legacy.to_string()).bind(offset).bind(offset).execute(&pool).await.unwrap();
    }
    sqlx::query("INSERT INTO worker_checkpoints VALUES ('session',0,'turn',?)")
        .bind(encode(&saved_checkpoint).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO worker_supplements VALUES ('session',0,'turn',?)")
        .bind(encode(&supplement).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let store = WorkerExecutionStore::open(&path).await.unwrap();
    let upgraded: Vec<(i64, String)> = sqlx::query_as("SELECT version, lower(hex(checksum)) FROM _sqlx_migrations WHERE version <= 2 ORDER BY version").fetch_all(&store.pool).await.unwrap();
    assert_eq!(upgraded, checksums);
    assert!(sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&store.pool)
        .await
        .unwrap()
        .is_empty());
    let indexes: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='index' AND sql IS NOT NULL ORDER BY name",
    )
    .fetch_all(&store.pool)
    .await
    .unwrap();
    assert_eq!(
        indexes,
        [
            "agent_events_replay",
            "dream_formations_replay",
            "turn_admissions_command_identity"
        ]
    );
    let page = store
        .read_events(AgentSessionEventsParams {
            session: session(),
            after_cursor: None,
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(
        page.events
            .iter()
            .map(|fact| fact.event.clone())
            .collect::<Vec<_>>(),
        events
    );
    assert_eq!(
        page.events
            .iter()
            .map(|fact| fact.cursor.offset)
            .collect::<Vec<_>>(),
        [7, 8, 9, 10]
    );
    assert_eq!(page.active_turn, Some(AgentTurnRef::new("session", "turn")));
    assert_eq!(
        store.checkpoint(&events[3].identity).await.unwrap(),
        Some(saved_checkpoint.clone())
    );
    assert_eq!(
        store.settlement(&events[3].identity).await.unwrap(),
        Some((events[3].clone(), settlement))
    );
    assert_eq!(
        store
            .waiting_continuation(&session(), &events[3].identity, 4)
            .await
            .unwrap(),
        continuation
    );
    assert!(!serde_json::to_string(&page)
        .unwrap()
        .contains("provider-state"));
    assert_eq!(store.memory_progress("session").await.unwrap(), 8);
    assert!(store.interrupted_executions().await.unwrap().is_empty());
    for (command, expected) in [
        (&start_command, &events[0]),
        (&response_command, &events[2]),
    ] {
        assert_eq!(
            store
                .admission(&session(), &expected.identity, command)
                .await
                .unwrap(),
            Some(vec![expected.clone()])
        );
        let raw: String =
            sqlx::query_scalar("SELECT request_json FROM turn_admissions WHERE input_id=?")
                .bind(command_key(command).unwrap())
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_ne!(
            raw,
            encode(command).unwrap(),
            "migration JSON order must not become command identity"
        );
        assert_eq!(decode::<AgentExecutionCommand>(&raw).unwrap(), *command);
        assert!(store
            .admit(
                &session(),
                command,
                vec![expected.clone()],
                &saved_checkpoint
            )
            .await
            .unwrap_err()
            .is_code(ErrorCode::Conflict));
    }
    response.input.created_at += 1;
    let changed = AgentExecutionCommand::Run {
        params: Box::new(response.clone()),
    };
    assert!(store
        .admission(&session(), &events[3].identity, &changed)
        .await
        .unwrap_err()
        .is_code(ErrorCode::IdempotencyConflict));
    assert!(store
        .admit(
            &session(),
            &changed,
            vec![events[2].clone()],
            &saved_checkpoint
        )
        .await
        .unwrap_err()
        .is_code(ErrorCode::IdempotencyConflict));

    response.input.message_id = "answer-2".into();
    response.command.as_mut().unwrap().idempotency_key = "next-answer-key".into();
    let AgentMessageBlockContent::UserContent(AgentUserContent::UserActionResponse {
        action_id,
        ..
    }) = &mut response.input.blocks[0].content
    else {
        unreachable!()
    };
    *action_id = "action-2".into();
    response.validate().unwrap();
    let resumed = event(
        5,
        AgentEventPayload::MessageCompleted(response.input.clone()),
    );
    saved_checkpoint.last_sequence = 5;
    saved_checkpoint.messages.push(response.input.clone());
    store
        .admit(
            &session(),
            &AgentExecutionCommand::Run {
                params: Box::new(response),
            },
            vec![resumed],
            &saved_checkpoint,
        )
        .await
        .unwrap();
    saved_checkpoint.last_sequence = 6;
    store
        .append(
            &session(),
            vec![event(
                6,
                AgentEventPayload::TurnCompleted(talos_agent_api::AgentTurnCompleted {
                    stop_reason: talos_agent_api::AgentStopReason::EndTurn,
                }),
            )],
            &saved_checkpoint,
            Some(AgentExecutionSettlement::Finished {
                memory_usage: vec![],
                metrics: Default::default(),
            }),
        )
        .await
        .unwrap();
    let continued = store
        .read_events(AgentSessionEventsParams {
            session: session(),
            after_cursor: Some(page.next_cursor),
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(
        continued
            .events
            .iter()
            .map(|fact| fact.cursor.offset)
            .collect::<Vec<_>>(),
        [11, 12]
    );
    assert_eq!(
        continued.events[1]
            .history_supplement
            .as_ref()
            .unwrap()
            .requests,
        supplement.requests
    );
    let body = store
        .read_payload(agent_protocol::AgentExecutionPayloadParams {
            session: session(),
            identity: events[3].identity.clone(),
            payload_id: "payload".into(),
            offset: 0,
            limit: 256 * 1024,
        })
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<talos_agent_api::trajectory::TrajectoryPayload>(&body.data).unwrap(),
        payload
    );
    store.pool.close().await;
    let reopened = WorkerExecutionStore::open(&path).await.unwrap();
    assert_eq!(
        reopened
            .admission(&session(), &events[0].identity, &start_command)
            .await
            .unwrap(),
        Some(vec![events[0].clone()])
    );
    assert_eq!(reopened.memory_progress("session").await.unwrap(), 8);
}
