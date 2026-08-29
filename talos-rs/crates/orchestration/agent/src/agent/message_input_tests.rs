use super::*;

#[test]
fn user_input_maps_attachments_between_model_segments() {
    let mut timeline = TimelineBuilder::new(
        "request-user-input",
        "turn-user-input",
        "message-1",
        "session-1",
    );
    let mut state = KernelTimelineState::default();
    let first_key = KernelStreamItemKey {
        round: Some(0),
        local_id: "first-answer".to_string(),
    };
    emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::AssistantTextStarted,
            KernelStreamEventData::AssistantTextStarted(AssistantTextStartedStreamData {
                item_key: first_key.clone(),
                phase: AssistantTextPhase::FinalAnswer,
                round: Some(0),
            }),
        ),
    )
    .expect("first model segment should start");
    emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::AssistantTextCompleted,
            KernelStreamEventData::AssistantTextCompleted(AssistantTextCompletedStreamData {
                item_key: first_key,
            }),
        ),
    )
    .expect("first model segment should complete");

    let input_events = emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::UserInput,
            KernelStreamEventData::UserInput(talos_agent_kernel::UserInput {
                text: "inspect this file".to_string(),
                file_ids: vec!["file-1".to_string()],
            }),
        ),
    )
    .expect("same-turn user input should start the next model segment");

    assert_eq!(input_events.len(), 2);
    assert!(matches!(
        &input_events[0].payload,
        AgentEventPayload::ItemStarted(payload)
            if payload.item.kind == TalosTimelineItemKind::UserInput
                && payload.item.phase == TalosTimelinePhase::Input
                && matches!(
                    &payload.item.content,
                    TalosTimelineContent::UserInput { text, file_ids }
                        if text == "inspect this file" && file_ids.as_slice() == ["file-1"]
                )
    ));
    assert!(matches!(
        &input_events[1].payload,
        AgentEventPayload::ItemCompleted(payload)
            if payload.status == TalosTimelineItemStatus::Completed
    ));

    let second_key = KernelStreamItemKey {
        round: Some(1),
        local_id: "second-answer".to_string(),
    };
    emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::AssistantTextStarted,
            KernelStreamEventData::AssistantTextStarted(AssistantTextStartedStreamData {
                item_key: second_key,
                phase: AssistantTextPhase::FinalAnswer,
                round: Some(1),
            }),
        ),
    )
    .expect("model work should continue after same-turn user input");
}

struct MessageAwareProvider {
    started: Arc<Notify>,
    release_first: Arc<Notify>,
    calls: AtomicUsize,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
}

#[async_trait::async_trait]
impl LlmProvider for MessageAwareProvider {
    fn name(&self) -> &str {
        "message-aware"
    }

    fn default_model(&self) -> String {
        "mock-model".to_string()
    }

    async fn chat(&self, request: ChatRequest) -> talos_error::TalosResult<ChatResponse> {
        Ok(ChatResponse {
            content: "provider ok".to_string(),
            reasoning: ReasoningOutput::default(),
            model: request.model,
            usage: Usage::default(),
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> talos_error::TalosResult<ChatStream> {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        Ok(ChatStream { rx })
    }

    async fn stream_model_blocks(
        &self,
        request: ChatRequest,
    ) -> talos_error::TalosResult<ModelBlockStream> {
        let call_index = self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().unwrap().push(request);
        if call_index == 0 {
            self.started.notify_one();
            self.release_first.notified().await;
            return Ok(single_text_block_stream("before message"));
        }
        Ok(single_text_block_stream("after message"))
    }
}

#[tokio::test]
async fn running_turn_message_is_included_in_the_next_model_request() {
    let started = Arc::new(Notify::new());
    let release_first = Arc::new(Notify::new());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let agent = test_agent_harness(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        static_router(Arc::new(MessageAwareProvider {
            started: started.clone(),
            release_first: release_first.clone(),
            calls: AtomicUsize::new(0),
            requests: requests.clone(),
        })),
        model_selection("gpt-4o"),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    )
    .expect("agent harness should build");
    let session = build_test_session("session-message", "default");
    let turn_id = "turn-message";
    let mut stream = agent
        .run_streamed(agent_run_params(
            session.clone(),
            turn_id,
            vec![TalosInputItem::Text {
                text: "initial".to_string(),
            }],
        ))
        .await
        .expect("stream should start");
    started.notified().await;

    let first_agent = agent.clone();
    let first_session = session.clone();
    let first_submit = tokio::spawn(async move {
        first_agent
            .submit_input(AgentInputParams {
                session: first_session,
                turn_id: turn_id.to_string(),
                input: AgentInput::Message {
                    input: vec![TalosInputItem::Text {
                        text: "focus on tests".to_string(),
                    }],
                },
            })
            .await
    });
    let second_agent = agent.clone();
    let second_submit = tokio::spawn(async move {
        second_agent
            .submit_input(AgentInputParams {
                session,
                turn_id: turn_id.to_string(),
                input: AgentInput::Message {
                    input: vec![TalosInputItem::Text {
                        text: "keep the api stable".to_string(),
                    }],
                },
            })
            .await
    });
    tokio::task::yield_now().await;
    assert!(!first_submit.is_finished());
    assert!(!second_submit.is_finished());
    release_first.notify_one();
    let first_result = first_submit
        .await
        .expect("first submit task should complete")
        .expect("running message should be accepted");
    let second_result = second_submit
        .await
        .expect("second submit task should complete")
        .expect("second running message should be accepted");
    let mut events = Vec::new();
    while let Some(event) = stream.recv().await {
        let terminal = matches!(
            &event.payload,
            AgentEventPayload::TurnCompleted(_) | AgentEventPayload::TurnError(_)
        );
        events.push(event);
        if terminal {
            break;
        }
    }

    let requests = requests.lock().unwrap();
    assert!(first_result.accepted);
    assert!(second_result.accepted);
    assert_eq!(requests.len(), 2);
    let user_messages = requests[1]
        .messages
        .iter()
        .filter(|message| message.role == Role::User)
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>();
    let first_index = user_messages
        .iter()
        .position(|message| message.contains("focus on tests"))
        .expect("first steer should be in the next model request");
    let second_index = user_messages
        .iter()
        .position(|message| message.contains("keep the api stable"))
        .expect("second steer should be in the next model request");
    assert!(first_index < second_index);

    let timeline_inputs = events
        .iter()
        .filter_map(|event| match &event.payload {
            AgentEventPayload::ItemStarted(payload)
                if payload.item.kind == TalosTimelineItemKind::UserInput =>
            {
                match &payload.item.content {
                    TalosTimelineContent::UserInput { text, file_ids } => Some((
                        event.message_id.as_str(),
                        payload.turn_id.as_str(),
                        text.as_str(),
                        file_ids.as_slice(),
                    )),
                    _ => None,
                }
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        timeline_inputs,
        vec![
            ("message-1", turn_id, "initial", &[] as &[String]),
            ("message-1", turn_id, "focus on tests", &[] as &[String],),
            (
                "message-1",
                turn_id,
                "keep the api stable",
                &[] as &[String],
            ),
        ]
    );
    let assistant_item_indices = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| match &event.payload {
            AgentEventPayload::ItemStarted(payload)
                if payload.item.kind == TalosTimelineItemKind::AssistantText =>
            {
                Some(index)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let steer_item_indices = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| match &event.payload {
            AgentEventPayload::ItemStarted(payload) => match &payload.item.content {
                TalosTimelineContent::UserInput { text, .. }
                    if text == "focus on tests" || text == "keep the api stable" =>
                {
                    Some(index)
                }
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(assistant_item_indices.len(), 2);
    assert_eq!(steer_item_indices.len(), 2);
    assert!(assistant_item_indices[0] < steer_item_indices[0]);
    assert!(steer_item_indices[1] < assistant_item_indices[1]);
    assert!(matches!(
        events.last().map(|event| &event.payload),
        Some(AgentEventPayload::TurnCompleted(_))
    ));
}
