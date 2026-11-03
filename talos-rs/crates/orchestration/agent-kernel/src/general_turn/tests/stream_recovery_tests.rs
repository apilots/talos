use super::*;

struct MalformedArgumentsRecoveryProvider {
    calls: AtomicUsize,
    raw_arguments: &'static str,
    expected_context: Value,
}

#[async_trait]
impl LlmProvider for MalformedArgumentsRecoveryProvider {
    fn name(&self) -> &str {
        "malformed-arguments-recovery"
    }
    fn default_model(&self) -> String {
        self.name().to_string()
    }
    async fn chat(&self, _: ChatRequest) -> TalosResult<ChatResponse> {
        panic!("streaming only")
    }
    async fn stream(&self, _: ChatRequest) -> TalosResult<ChatStream> {
        panic!("model blocks only")
    }
    async fn stream_model_blocks(&self, request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let events = if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            vec![
                tool_started(0, "malformed-read", Some("read")),
                ModelBlockStreamEvent::BlockDelta {
                    model_item_id: "tool_call_0".into(),
                    delta: ModelBlockDelta::ToolArguments {
                        delta: self.raw_arguments.into(),
                    },
                },
                block_completed("tool_call_0"),
                done(),
            ]
        } else {
            let call = request
                .messages
                .iter()
                .flat_map(|message| message.tool_calls.iter().flatten())
                .find(|call| call.id == "malformed-read")
                .unwrap();
            assert_eq!(call.arguments, self.expected_context);
            assert!(request
                .messages
                .iter()
                .any(|message| message.role == talos_llm_provider::Role::Tool
                    && message.tool_call_id.as_deref() == Some("malformed-read")
                    && message.content.contains("\"ok\":false")));
            vec![
                assistant_started("final", ModelOutputPhase::FinalAnswer),
                ModelBlockStreamEvent::BlockDelta {
                    model_item_id: "final".into(),
                    delta: ModelBlockDelta::AssistantText {
                        delta: "Recovered from invalid arguments.".into(),
                    },
                },
                block_completed("final"),
                done(),
            ]
        };
        ScriptedModelBlockProvider::new("malformed-arguments-recovery", events)
            .stream_model_blocks(request)
            .await
    }
}

#[tokio::test]
async fn malformed_arguments_fail_execution_but_allow_model_context_recovery() {
    for (raw_arguments, expected_context) in [
        ("{", json!({})),
        ("\"literal\"", json!("literal")),
        ("[1,2]", json!([1, 2])),
    ] {
        let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
        std::fs::create_dir_all(&workspace_dir).unwrap();
        let provider = Arc::new(MalformedArgumentsRecoveryProvider {
            calls: AtomicUsize::new(0),
            raw_arguments,
            expected_context,
        });
        let registry = Arc::new(ToolRegistry::new());
        register_core_local_tools(registry.as_ref());
        let mut config = AgentKernelConfig::default();
        config.prompt.bootstrap.enabled = false;
        config.workspace_dir = workspace_dir.to_string_lossy().into_owned();
        configure_test_skill_root(&mut config, &workspace_dir);
        let (tx, mut rx) = mpsc::channel(256);
        let mut ctx = build_ctx_with_stream_for_session("malformed-recovery", Some(tx));
        let result = run_general_turn(
            &mut ctx,
            "talos",
            "assistant",
            "Recover from a malformed tool call.",
            provider.clone(),
            registry.clone(),
            registry,
            Arc::new(TestMemoryStore::new()),
            &config,
            skill_options_from_config(&workspace_dir, &config),
        )
        .await
        .unwrap();
        assert_eq!(result.output_text(), "Recovered from invalid arguments.");
        assert_eq!(result.metrics().tool_errors, 1);
        assert_eq!(result.metrics().tool_successes, 0);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
        let mut original_observed = false;
        while let Ok(event) = rx.try_recv() {
            if let AgentKernelEventPayload::ToolCallStart(call) = event.payload {
                let expected_original = serde_json::from_str::<Value>(raw_arguments)
                    .unwrap_or_else(|_| json!(raw_arguments));
                original_observed = call.arguments == expected_original;
            }
        }
        assert!(
            original_observed,
            "canonical tool observation must retain the malformed input"
        );
        std::fs::remove_dir_all(workspace_dir).unwrap();
    }
}
struct TimeoutAfterToolFailureProvider {
    calls: AtomicUsize,
    missing_path: String,
    fail_on_call: usize,
}

#[async_trait]
impl LlmProvider for TimeoutAfterToolFailureProvider {
    fn name(&self) -> &str {
        "timeout-after-tool-failure"
    }
    fn default_model(&self) -> String {
        self.name().to_string()
    }
    async fn chat(&self, _: ChatRequest) -> TalosResult<ChatResponse> {
        panic!("streaming only")
    }
    async fn stream(&self, _: ChatRequest) -> TalosResult<ChatStream> {
        panic!("model blocks only")
    }
    async fn stream_model_blocks(&self, request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if call == 1 {
            return StreamingToolExecutionProvider {
                path: self.missing_path.clone(),
            }
            .stream_model_blocks(request)
            .await;
        }
        assert!(request
            .messages
            .iter()
            .skip_while(|message| message.role == talos_llm_provider::Role::System)
            .all(|message| message.role != talos_llm_provider::Role::System));
        assert!(request
            .messages
            .iter()
            .any(|message| message.role == talos_llm_provider::Role::Tool
                && message.tool_call_id.as_deref() == Some("call_read")));
        if call < self.fail_on_call {
            return ScriptedModelBlockProvider::new("empty-before-finalization", vec![done()])
                .stream_model_blocks(request)
                .await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        Err(TalosError::new(
            talos_error::ErrorCode::Timeout,
            "Model response stream timed out.",
        ))
    }
}

#[tokio::test]
async fn failed_tool_recovery_keeps_system_prefix_and_counts_next_failed_model_attempt() {
    // Fail a regular model step, forced finalization, then salvage finalization.
    for fail_on_call in [2, 3, 4] {
        let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
        std::fs::create_dir_all(&workspace_dir).unwrap();
        let provider = Arc::new(TimeoutAfterToolFailureProvider {
            calls: AtomicUsize::new(0),
            fail_on_call,
            missing_path: workspace_dir
                .join("missing.txt")
                .to_string_lossy()
                .into_owned(),
        });
        let registry = Arc::new(ToolRegistry::new());
        register_core_local_tools(registry.as_ref());
        let mut config = AgentKernelConfig::default();
        config.prompt.bootstrap.enabled = false;
        config.workspace_dir = workspace_dir.to_string_lossy().into_owned();
        configure_test_skill_root(&mut config, &workspace_dir);
        let (tx, mut rx) = mpsc::channel(256);
        let mut ctx = build_ctx_with_stream_for_session("failure-metrics", Some(tx));
        let error = run_general_turn(
            &mut ctx,
            "talos",
            "assistant",
            "Read the file and recover from errors.",
            provider.clone(),
            registry.clone(),
            registry,
            Arc::new(TestMemoryStore::new()),
            &config,
            skill_options_from_config(&workspace_dir, &config),
        )
        .await
        .expect_err("the selected model request times out");
        assert_eq!(error.error_code(), talos_error::ErrorCode::Timeout);
        let mut metrics_updates = Vec::new();
        while let Ok(event) = rx.try_recv() {
            if let AgentKernelEventPayload::TurnMetrics(data) = event.payload {
                metrics_updates.push(data.metrics);
            }
        }
        let metrics = metrics_updates
            .pop()
            .expect("failure must publish terminal metrics");
        let before_failure = metrics_updates
            .last()
            .expect("tool batch must publish metrics");
        assert_eq!(metrics.model_rounds, fail_on_call as u32);
        assert!(metrics.duration_ms.unwrap() >= before_failure.duration_ms.unwrap() + 20);
        assert_eq!(metrics.tool_calls, 1);
        assert_eq!(metrics.tool_errors, 1);
        assert_eq!(provider.calls.load(Ordering::SeqCst), fail_on_call);
        std::fs::remove_dir_all(workspace_dir).unwrap();
    }
}
