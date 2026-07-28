use super::*;

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
    let session = build_test_session("session-message", "workspace-a");
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

    let result = agent
        .submit_input(AgentInputParams {
            session,
            turn_id: turn_id.to_string(),
            input: AgentInput::Message {
                message: "focus on tests".to_string(),
            },
        })
        .await
        .expect("running message should be accepted");
    release_first.notify_one();
    while let Some(event) = stream.recv().await {
        if matches!(
            event.payload,
            AgentEventPayload::TurnCompleted(_) | AgentEventPayload::TurnError(_)
        ) {
            break;
        }
    }

    let requests = requests.lock().unwrap();
    assert!(result.accepted);
    assert_eq!(requests.len(), 2);
    assert!(requests[1].messages.iter().any(|message| {
        message.role == Role::User && message.content.contains("focus on tests")
    }));
}
