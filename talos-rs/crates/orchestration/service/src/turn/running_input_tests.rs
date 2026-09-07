use super::*;
use talos_agent_api::{AgentTurnRef, SubmitTurnInput, SubmitTurnInputRequest};

#[tokio::test]
async fn start_admission_outlives_request_cancellation_before_store_commit() {
    let execution = Arc::new(RecordingAgentExecution::default());
    execution.hold_admission.store(true, Ordering::SeqCst);
    let (service, _, store, _) = test_service(execution.clone()).await;
    create_test_session_record(store.clone()).await;
    let request_owner = tokio::spawn({
        let service = service.clone();
        async move {
            submit_test_start(
                &service,
                test_session(),
                StartTurnRequest {
                    session_id: "s1".to_string(),
                    input: test_user_input("keep running after disconnect"),
                    model_selection: None,
                    user_permission_mode: None,
                    project_id: None,
                    current_directory: None,
                    client_request_id: None,
                },
                None,
            )
            .await
        }
    });
    execution.admission_reached.notified().await;
    request_owner.abort();
    let _ = request_owner.await;
    execution.admission_release.notify_one();
    wait_for_canonical_terminal(store.as_ref()).await;
    let identity = execution
        .stream_request
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .context
        .identity();
    tokio::time::timeout(Duration::from_secs(1), async {
        while service.is_turn_execution_active(&identity) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        (
            service.is_turn_execution_active(&identity),
            store.get_agent_active_turn("s1").await.unwrap()
        ),
        (false, None),
    );
}

#[tokio::test]
async fn running_input_replays_the_committed_response_without_delivering_twice() {
    let agent_execution = Arc::new(RecordingAgentExecution::default());
    *agent_execution.hold_running_turn.lock().unwrap() = true;
    let (service, _event_service, store, _message_queue) =
        test_service(agent_execution.clone()).await;
    create_test_session_record(store).await;
    let started = submit_test_start(
        &service,
        test_session(),
        StartTurnRequest {
            session_id: "s1".to_string(),
            input: test_user_input("running"),
            model_selection: None,
            user_permission_mode: None,
            project_id: None,
            current_directory: None,
            client_request_id: None,
        },
        None,
    )
    .await
    .unwrap();
    let request = SubmitTurnInputRequest {
        identity: AgentTurnRef::new("s1", started.turn_id.clone()),
        input: SubmitTurnInput::Message {
            input: test_user_input("steer once"),
        },
        client_request_id: Some("input-request-1".to_string()),
    };

    let first = service
        .submit_turn_input(test_session(), request.clone())
        .await
        .unwrap();
    let replay = service
        .submit_turn_input(test_session(), request)
        .await
        .unwrap();

    assert_eq!(replay, first);
    assert_eq!(agent_execution.submitted_inputs.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn durable_input_delivery_outlives_the_request_owner() {
    let agent_execution = Arc::new(RecordingAgentExecution::default());
    *agent_execution.hold_running_turn.lock().unwrap() = true;
    *agent_execution.hold_input_permit.lock().unwrap() = true;
    let (service, _event_service, store, _message_queue) =
        test_service(agent_execution.clone()).await;
    create_test_session_record(store).await;
    let started = submit_test_start(
        &service,
        test_session(),
        StartTurnRequest {
            session_id: "s1".to_string(),
            input: test_user_input("running"),
            model_selection: None,
            user_permission_mode: None,
            project_id: None,
            current_directory: None,
            client_request_id: None,
        },
        None,
    )
    .await
    .unwrap();
    let request = SubmitTurnInputRequest {
        identity: AgentTurnRef::new("s1", started.turn_id.clone()),
        input: SubmitTurnInput::Message {
            input: test_user_input("steer after disconnect"),
        },
        client_request_id: Some("disconnected-input-request".to_string()),
    };

    let request_owner = tokio::spawn({
        let service = service.clone();
        async move {
            service
                .submit_turn_input(test_session(), request.clone())
                .await
        }
    });
    agent_execution.input_permit_started.notified().await;
    request_owner.abort();
    agent_execution.input_permit_release.notify_one();
    tokio::time::timeout(Duration::from_secs(1), async {
        while !agent_execution
            .input_permit_completed
            .load(Ordering::SeqCst)
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the execution driver should retain the durable delivery permit");

    let replay = service
        .submit_turn_input(
            test_session(),
            SubmitTurnInputRequest {
                identity: AgentTurnRef::new("s1", started.turn_id.clone()),
                input: SubmitTurnInput::Message {
                    input: test_user_input("steer after disconnect"),
                },
                client_request_id: Some("disconnected-input-request".to_string()),
            },
        )
        .await
        .expect("the durable response should remain replayable");

    assert_eq!(replay.identity.turn_id, started.turn_id);
    assert_eq!(agent_execution.submitted_inputs.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn queued_input_replays_without_redelivering_or_requiring_the_consumed_message() {
    let agent_execution = Arc::new(RecordingAgentExecution::default());
    *agent_execution.hold_running_turn.lock().unwrap() = true;
    let (service, _event_service, store, message_queue) =
        test_service(agent_execution.clone()).await;
    create_test_session_record(store).await;
    let started = submit_test_start(
        &service,
        test_session(),
        StartTurnRequest {
            session_id: "s1".to_string(),
            input: test_user_input("running"),
            model_selection: None,
            user_permission_mode: None,
            project_id: None,
            current_directory: None,
            client_request_id: None,
        },
        None,
    )
    .await
    .unwrap();
    let queued = message_queue
        .add("s1", test_user_input("queued steer"))
        .unwrap();
    let request = SubmitTurnInputRequest {
        identity: AgentTurnRef::new("s1", started.turn_id),
        input: SubmitTurnInput::QueuedMessage {
            message_id: queued.messages[0].id.clone(),
        },
        client_request_id: Some("queued-input-request-1".to_string()),
    };

    let first = service
        .submit_turn_input(test_session(), request.clone())
        .await
        .unwrap();
    let replay = service
        .submit_turn_input(test_session(), request)
        .await
        .unwrap();

    assert_eq!(replay, first);
    assert!(message_queue.list("s1").unwrap().messages.is_empty());
    assert_eq!(agent_execution.submitted_inputs.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn queued_input_returns_durable_acceptance_when_provider_delivery_later_fails() {
    let agent_execution = Arc::new(RecordingAgentExecution::default());
    *agent_execution.hold_running_turn.lock().unwrap() = true;
    let (service, _event_service, store, message_queue) =
        test_service(agent_execution.clone()).await;
    create_test_session_record(store).await;
    let started = submit_test_start(
        &service,
        test_session(),
        StartTurnRequest {
            session_id: "s1".to_string(),
            input: test_user_input("running"),
            model_selection: None,
            user_permission_mode: None,
            project_id: None,
            current_directory: None,
            client_request_id: None,
        },
        None,
    )
    .await
    .unwrap();
    let queued = message_queue
        .add("s1", test_user_input("queued steer"))
        .unwrap();
    agent_execution
        .input_permit_error
        .lock()
        .unwrap()
        .replace(talos_error::TalosError::new(
            talos_error::ErrorCode::InternalError,
            "Provider stopped before accepting live input",
        ));

    service
        .steer_queued_message(
            test_session(),
            &queued.messages[0].id,
            &started.turn_id,
            None,
        )
        .await
        .expect("durable admission remains accepted despite later Provider failure");

    assert!(message_queue.list("s1").unwrap().messages.is_empty());
}

#[tokio::test]
async fn steer_accepts_text_and_files_but_rejects_skills_without_consuming_them() {
    let agent_execution = Arc::new(RecordingAgentExecution::default());
    *agent_execution.hold_running_turn.lock().unwrap() = true;
    let (service, _event_service, store, message_queue) =
        test_service(agent_execution.clone()).await;
    create_test_session_record(store.clone()).await;
    store
        .commit_maintenance_admission(CanonicalAgentEvent {
            identity: talos_agent_api::AgentTurnRef::new("other-session", "maintenance"),
            sequence: 1,
            occurred_at: 1,
            payload: CanonicalAgentEventPayload::TurnStarted(talos_agent_api::AgentTurnStarted {}),
        })
        .await
        .unwrap();
    let started = submit_test_start(
        &service,
        test_session(),
        StartTurnRequest {
            session_id: "s1".to_string(),
            input: test_user_input("running"),
            model_selection: None,
            user_permission_mode: None,
            project_id: None,
            current_directory: None,
            client_request_id: None,
        },
        None,
    )
    .await
    .unwrap();
    let active_turn_id = started.turn_id;
    let accepted_input = AgentUserInput {
        content: vec![
            AgentUserContent::Text {
                text: "inspect this".to_string(),
            },
            AgentUserContent::File {
                file_id: "file-1".to_string(),
            },
        ],
    };
    let accepted = message_queue.add("s1", accepted_input.clone()).unwrap();

    let stale_error = service
        .steer_queued_message(test_session(), &accepted.messages[0].id, "turn-stale", None)
        .await
        .expect_err("stale turn id must not consume the queued message");
    assert!(stale_error.is_code(talos_error::ErrorCode::ResourceNotFound));
    assert_eq!(message_queue.list("s1").unwrap(), accepted);
    assert!(agent_execution.submitted_inputs.lock().unwrap().is_empty());

    service
        .steer_queued_message(
            test_session(),
            &accepted.messages[0].id,
            &active_turn_id,
            None,
        )
        .await
        .expect("text and files should be accepted for steer");

    {
        let submitted = agent_execution.submitted_inputs.lock().unwrap();
        assert!(matches!(
            &submitted[0].input.blocks[0].content,
            talos_agent_api::AgentMessageBlockContent::UserContent(
                talos_agent_api::AgentUserContent::Text { text }
            ) if text == "inspect this"
        ));
        assert!(matches!(
            &submitted[0].input.blocks[1].content,
            talos_agent_api::AgentMessageBlockContent::UserContent(
                talos_agent_api::AgentUserContent::File { file_id }
            ) if file_id == "file-1"
        ));
    }
    let rejected = message_queue
        .add("s1", test_user_input("too late"))
        .unwrap();
    agent_execution
        .input_error
        .lock()
        .unwrap()
        .replace(TalosError::new(
            talos_error::ErrorCode::Conflict,
            "active turn ended before accepting the message".to_string(),
        ));

    let rejected_error = service
        .steer_queued_message(
            test_session(),
            &rejected.messages[0].id,
            &active_turn_id,
            None,
        )
        .await
        .expect_err("rejected active-turn input must remain queued");

    assert!(rejected_error.is_code(talos_error::ErrorCode::Conflict));
    assert_eq!(message_queue.list("s1").unwrap(), rejected);
    message_queue
        .delete("s1", &rejected.messages[0].id)
        .unwrap();
    let skill_queue = message_queue
        .add(
            "s1",
            AgentUserInput {
                content: vec![AgentUserContent::SkillSelection {
                    name: "review".to_string(),
                    path: "/skills/review".to_string(),
                }],
            },
        )
        .unwrap();
    let skill_id = skill_queue.messages[0].id.clone();

    let error = service
        .steer_queued_message(test_session(), &skill_id, &active_turn_id, None)
        .await
        .expect_err("skills are not supported by active turn steer");

    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
    assert_eq!(
        message_queue.list("s1").unwrap().messages,
        skill_queue.messages
    );
}
