use super::*;

#[tokio::test]
async fn test_resume_streamed_replays_dynamic_resume_into_model_loop() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let provider = Arc::new(DynamicResumeAgentProvider {
        calls: AtomicUsize::new(0),
    });
    let registry = Arc::new(ToolRegistry::new_with_policy(ToolPolicy::allow_list(vec![
        "echo_dynamic",
        "Read",
    ])));
    registry.register(Arc::new(ReadTool));
    registry.register_external_dynamic_tool(RegisteredToolSpec::new(
        ToolDefinition::new(
            "echo_dynamic",
            "External dynamic echo tool",
            Some(json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" }
                }
            })),
        ),
        ToolSource::Dynamic,
        ToolPayloadKind::Function,
        ToolParallelism::ParallelSafe,
        false,
        default_governance_for_source(ToolSource::Dynamic),
    ));
    let mut kernel_config = AgentKernelConfig::default();
    kernel_config.prompt.bootstrap.enabled = false;
    kernel_config.workspace_dir = temp_dir.path().display().to_string();
    let agent = test_agent_harness(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        static_router(provider.clone()),
        model_selection("mock-model"),
        registry,
        kernel_config,
    )
    .expect("agent harness should build");
    let session_id = "session-dynamic-main-resume";
    let mut session = build_test_session(session_id, "workspace-1");
    session.config.agent.kernel.prompt.bootstrap.enabled = false;
    session.config.agent.kernel.workspace_dir = temp_dir.path().display().to_string();
    session.config.agent.kernel.output.max_tokens = 256;
    let kernel_config = AgentHarnessImpl::kernel_config_for_session(&session)
        .expect("session workspace should resolve");
    let workspace_root = std::path::PathBuf::from(&kernel_config.workspace_dir);
    std::fs::create_dir_all(&workspace_root).expect("session workspace should be created");
    std::fs::write(workspace_root.join("deferred.txt"), "deferred tool output")
        .expect("deferred read fixture should be written");
    let request_record = session_artifacts::SessionTranscriptRecord {
        header: session_artifacts::SessionTranscriptHeader {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            invocation_id: "turn-original".to_string(),
            branch: session_artifacts::SessionTranscriptBranch::ReactiveLoop,
            timestamp: Utc::now(),
        },
        author: session_artifacts::SessionTranscriptAuthor::System,
        kind: session_artifacts::SessionTranscriptEventKind::SystemMessage,
        payload: session_artifacts::SessionTranscriptPayload::UserActionRequest {
            payload: session_artifacts::SessionTranscriptUserActionRequestPayload {
                action_id: "ua-dynamic".to_string(),
                call_id: "call-dynamic".to_string(),
                tool_name: "echo_dynamic".to_string(),
                arguments: json!({ "message": "ping" }).into(),
                params: dynamic_user_action_params("echo_dynamic"),
                sandbox_request_fingerprint: None,
                reasoning_continuation: Some(ReasoningContinuation::Text(
                    "dynamic provider continuation".to_string(),
                )),
                tool_calls: vec![
                    FunctionCall::new("call-dynamic", "echo_dynamic", json!({ "message": "ping" })),
                    FunctionCall::new("call-deferred", "Read", json!({"path": "deferred.txt"})),
                ],
                tool_messages: Vec::new(),
                deferred_tool_calls: vec![FunctionCall::new(
                    "call-deferred",
                    "Read",
                    json!({"path": "deferred.txt"}),
                )],
            },
        },
        actions: session_artifacts::SessionTranscriptActions::default(),
        partial: false,
    };
    let response_record = session_artifacts::SessionTranscriptRecord {
        header: session_artifacts::SessionTranscriptHeader {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            invocation_id: "turn-original".to_string(),
            branch: session_artifacts::SessionTranscriptBranch::ReactiveLoop,
            timestamp: Utc::now(),
        },
        author: session_artifacts::SessionTranscriptAuthor::System,
        kind: session_artifacts::SessionTranscriptEventKind::SystemMessage,
        payload: session_artifacts::SessionTranscriptPayload::UserActionResponse {
            payload: session_artifacts::SessionTranscriptUserActionResponsePayload {
                call_id: "call-dynamic".to_string(),
                tool_name: "echo_dynamic".to_string(),
                response: dynamic_user_action_response("dynamic pong", true),
            },
        },
        actions: session_artifacts::SessionTranscriptActions::default(),
        partial: false,
    };
    append_transcript_record(&workspace_root, &request_record)
        .await
        .expect("dynamic request record should append");
    append_transcript_record(&workspace_root, &response_record)
        .await
        .expect("dynamic response record should append");

    let mut request = agent_run_params(session, "turn-original", Vec::new());
    request.resume_context = Some(AgentDispatchResumeContext {
        pending_call_id: "call-dynamic".to_string(),
        tool_calls: vec![
            AgentDispatchResumeToolCall {
                item_id: "tool-item-deferred".to_string(),
                call_id: "call-deferred".to_string(),
                tool_name: "Read".to_string(),
            },
            AgentDispatchResumeToolCall {
                item_id: "tool-item-original".to_string(),
                call_id: "call-dynamic".to_string(),
                tool_name: "echo_dynamic".to_string(),
            },
        ],
        execution_metrics: talos_types::talos::TurnExecutionMetrics {
            model_rounds: 1,
            tool_calls: 2,
            stop_reason: TalosStopReason::WaitingUserAction,
            ..Default::default()
        },
    });
    let mut stream = agent
        .resume_streamed(AgentResumeParams::new(request))
        .await
        .expect("resume stream should start");

    let mut saw_tool_result = false;
    let mut saw_final_answer = false;
    let mut observed_metrics = Vec::new();
    while let Some(event) = stream.recv().await {
        match event.payload {
            AgentEventPayload::ItemDelta(payload) => match payload.delta {
                talos_types::talos::TalosTimelineDelta::ToolResult { content, .. } => {
                    saw_tool_result |= content.iter().any(|item| {
                        matches!(
                            item,
                            TalosContentItem::Text { text }
                                if text.contains("dynamic pong")
                        ) || matches!(
                            item,
                            TalosContentItem::Json { value }
                                if value.to_string().contains("dynamic pong")
                        )
                    });
                }
                talos_types::talos::TalosTimelineDelta::Text { text }
                    if text.contains("Final answer after dynamic pong.") =>
                {
                    saw_final_answer = true;
                }
                _ => {}
            },
            AgentEventPayload::TurnMetrics(payload) => {
                observed_metrics.push(payload.metrics);
            }
            AgentEventPayload::TurnCompleted(payload) => {
                assert_eq!(payload.stop_reason, TalosStopReason::EndTurn);
                assert_eq!(
                    payload.metrics.as_ref().map(|metrics| metrics.tool_calls),
                    Some(2)
                );
                assert_eq!(
                    payload
                        .metrics
                        .as_ref()
                        .map(|metrics| metrics.tool_successes),
                    Some(2)
                );
                break;
            }
            AgentEventPayload::TurnError(payload) => {
                panic!("resume stream failed: {:?}", payload.error);
            }
            _ => {}
        }
    }

    assert!(
        saw_tool_result,
        "dynamic resume should emit replayed tool result"
    );
    assert!(
        saw_final_answer,
        "model loop should continue after dynamic replay"
    );
    assert!(!observed_metrics.is_empty());
    assert!(observed_metrics
        .iter()
        .all(|metrics| metrics.tool_calls == 2));
    assert!(observed_metrics
        .iter()
        .any(|metrics| metrics.tool_successes == 2));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}
