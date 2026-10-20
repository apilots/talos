use super::*;

fn prepared_with_budget(effective_prompt_budget: usize) -> PreparedGeneralTurn {
    PreparedGeneralTurn {
        browser_uploads: Vec::new(),
        session_id: "s1".to_string(),
        user_id: "u1".to_string(),
        invocation_id: "turn-1".to_string(),
        agent_id: "talos".to_string(),
        workspace_id: "default".to_string(),
        project_root: None,
        selected_model: "test-model".to_string(),
        max_output_tokens: None,
        reasoning_effort: None,
        speed: Default::default(),
        attachment_tokens: 0,
        image_input_enabled: false,
        messages: vec![ChatMessage::user("current task")],
        maintenance_usage: TokenUsage::default(),
        tool_selection_request: talos_tools::ToolSelectionRequest::default(),
        prompt_diagnostics: PromptDiagnostics {
            mode: talos_prompt::PromptMode::Full,
            included_sections: Vec::new(),
            dropped_sections: Vec::new(),
            bootstrap_files: Vec::new(),
            total_estimated_tokens: 0,
            static_estimated_tokens: 0,
            dynamic_estimated_tokens: 0,
            window_usage: talos_prompt::PromptWindowUsage {
                window_resolved: true,
                effective_prompt_budget,
                within_effective_budget: true,
                ..Default::default()
            },
            session_history_compaction: None,
        },
        prompt_warning_state: None,
        stream_tx: None,
        message_rx: None,
        run_tasks: crate::test_run_tasks(),
    }
}

#[test]
fn test_working_context_trim_preserves_recent_complete_tool_exchange() {
    let prepared = prepared_with_budget(80);
    let mut state = GeneralTurnLoopState::new(&prepared);
    let body = "The user prefers tea. ".repeat(80);
    let memory = talos_types::MemoryInfo {
        memory_id: "mem-1".to_string(),
        title: "Tea preference".to_string(),
        body,
        kind: talos_types::MemoryKind::Preference,
        importance: talos_types::MemoryImportance::High,
        occurred_at: 1,
        source: talos_types::MemorySourceInfo {
            date: "2026-07-20".to_string(),
            session_id: None,
            from_seq: None,
            until_seq: None,
        },
    };
    let batch =
        super::tool_batch::collect_general_turn_tool_batch(vec![crate::ToolExecutionOutput {
            call_id: "call-old".to_string(),
            tool_name: "MemoryGet".to_string(),
            arguments: serde_json::json!({"memory_id": "mem-1"}),
            outcome: ToolDispatchOutcome::executed(
                ToolOutputPayload::success_json(
                    &MemoryGetResponse {
                        memory: memory.clone(),
                    },
                    "serialize test memory",
                )
                .expect("memory payload"),
            ),
            is_error: false,
        }])
        .expect("collect retrieved memory batch");
    state.messages = vec![
        ChatMessage::assistant("old assistant text ".repeat(80)),
        ChatMessage::assistant_with_tool_calls(
            "old call",
            vec![FunctionCall::new(
                "call-old",
                "MemoryGet",
                serde_json::json!({}),
            )],
        ),
        batch.tool_messages[0].clone(),
        ChatMessage::assistant_with_tool_calls(
            "fresh call",
            vec![FunctionCall::new(
                "call-fresh",
                "Read",
                serde_json::json!({}),
            )],
        ),
        ChatMessage::tool("call-fresh", "Read", "fresh output"),
    ];

    let report = working_context::trim_working_context_to_budget(
        &prepared,
        &mut state,
        80,
        1,
        &AgentKernelConfig::default(),
    )
    .expect("working context should trim");

    assert!(report.trimmed_messages > 0);
    assert!(state.messages.iter().any(|message| {
        message.tool_call_id.as_deref() == Some("call-fresh")
            || message
                .tool_calls
                .as_ref()
                .is_some_and(|calls| calls.iter().any(|call| call.id == "call-fresh"))
    }));
    assert!(!state.messages.iter().any(|message| {
        message.tool_call_id.as_deref() == Some("call-old")
            || message
                .tool_calls
                .as_ref()
                .is_some_and(|calls| calls.iter().any(|call| call.id == "call-old"))
    }));
    assert_eq!(state.retrieved_memories, vec![memory]);
}

#[test]
fn test_working_context_trim_skips_failed_memory_get_result() {
    let prepared = prepared_with_budget(16);
    let mut state = GeneralTurnLoopState::new(&prepared);
    let batch =
        super::tool_batch::collect_general_turn_tool_batch(vec![crate::ToolExecutionOutput {
            call_id: "call-memory".to_string(),
            tool_name: "MemoryGet".to_string(),
            arguments: serde_json::json!({"memory_id": "missing"}),
            outcome: ToolDispatchOutcome::executed(ToolOutputPayload::failure_structured(
                "memory was not found ".repeat(20),
                None,
            )),
            is_error: true,
        }])
        .expect("collect failed memory batch");
    state.messages = vec![
        ChatMessage::assistant_with_tool_calls(
            "memory call",
            vec![FunctionCall::new(
                "call-memory",
                "MemoryGet",
                serde_json::json!({}),
            )],
        ),
        batch.tool_messages[0].clone(),
        ChatMessage::assistant_with_tool_calls(
            "fresh call",
            vec![FunctionCall::new(
                "call-fresh",
                "Read",
                serde_json::json!({}),
            )],
        ),
        ChatMessage::tool("call-fresh", "Read", "fresh output"),
    ];

    let report = working_context::trim_working_context_to_budget(
        &prepared,
        &mut state,
        16,
        1,
        &AgentKernelConfig::default(),
    )
    .expect("failed MemoryGet should be removable");

    assert!(report.trimmed_messages > 0);
    assert!(state.retrieved_memories.is_empty());
}

#[test]
fn test_working_context_trim_fails_when_latest_group_is_too_large() {
    let prepared = prepared_with_budget(16);
    let mut state = GeneralTurnLoopState::new(&prepared);
    state.messages = vec![ChatMessage::assistant("x".repeat(4_000))];

    let report = working_context::trim_working_context_to_budget(
        &prepared,
        &mut state,
        16,
        1,
        &AgentKernelConfig::default(),
    )
    .expect("working context should report the exhausted budget");

    assert!(report.post_tokens > 16);
    assert_eq!(state.messages.len(), 1);
}

#[test]
fn test_working_context_budget_includes_untrimmed_attachment_tokens() {
    let mut prepared = prepared_with_budget(16);
    prepared.attachment_tokens = 16;
    let mut state = GeneralTurnLoopState::new(&prepared);

    let report = working_context::trim_working_context_to_budget(
        &prepared,
        &mut state,
        16,
        1,
        &AgentKernelConfig::default(),
    )
    .expect("attachment budget should be reported without trimming the current input");

    assert!(report.post_tokens > 16);
    assert_eq!(state.messages.len(), 0);
}

#[test]
fn test_working_context_trim_rejects_invalid_memory_get_result() {
    let prepared = prepared_with_budget(16);
    let mut state = GeneralTurnLoopState::new(&prepared);
    state.messages = vec![
        ChatMessage::assistant_with_tool_calls(
            "memory call",
            vec![FunctionCall::new(
                "call-memory",
                "MemoryGet",
                serde_json::json!({}),
            )],
        ),
        ChatMessage::tool("call-memory", "MemoryGet", "invalid result"),
        ChatMessage::assistant_with_tool_calls(
            "fresh call",
            vec![FunctionCall::new(
                "call-fresh",
                "Read",
                serde_json::json!({}),
            )],
        ),
        ChatMessage::tool("call-fresh", "Read", "fresh output"),
    ];

    let error = working_context::trim_working_context_to_budget(
        &prepared,
        &mut state,
        16,
        1,
        &AgentKernelConfig::default(),
    )
    .expect_err("invalid MemoryGet result should fail context rebuild");

    assert!(error.is_internal());
}

#[tokio::test]
async fn resumed_screenshots_share_request_limits_with_user_attachments() {
    use talos_llm_provider::{Attachment, AttachmentKind};
    struct Images(std::sync::Mutex<Vec<String>>, usize);
    impl talos_tools::ToolImageStore for Images {
        fn save<'a>(
            &'a self,
            _: &'a str,
            _: Vec<Attachment>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = TalosResult<Vec<talos_types::FileInfo>>>
                    + Send
                    + 'a,
            >,
        > {
            unreachable!("resume only reads persisted images")
        }
        fn read<'a>(
            &'a self,
            user: &'a str,
            ids: &'a [String],
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = TalosResult<Vec<Attachment>>> + Send + 'a>,
        > {
            Box::pin(async move {
                assert_eq!(user, "u1");
                self.0.lock().unwrap().extend_from_slice(ids);
                Ok(ids
                    .iter()
                    .map(|id| {
                        Attachment::new(
                            AttachmentKind::Image,
                            id,
                            "image/png",
                            vec![1; self.1].into(),
                        )
                    })
                    .collect())
            })
        }
    }
    for (image_bytes, expected) in [
        (
            2 * 1024 * 1024,
            vec!["file-2", "file-3", "file-4", "file-5"],
        ),
        (6 * 1024 * 1024, vec!["file-4", "file-5"]),
    ] {
        let mut prepared = prepared_with_budget(100_000);
        prepared.image_input_enabled = true;
        prepared.messages = vec![ChatMessage::user_with_attachments(
            "reference",
            vec![Attachment::new(
                AttachmentKind::Image,
                "user.png",
                "image/png",
                vec![1; 3 * 1024 * 1024].into(),
            )],
        )
        .unwrap()];
        let original: Vec<_> = (0..6)
            .map(|index| {
                let mut message = ChatMessage::tool(
                    format!("call-{index}"),
                    "screenshot",
                    "screenshot file reference",
                );
                message.image_file_ids = vec![format!("file-{index}")];
                message
            })
            .collect();
        let resumed: Vec<ChatMessage> =
            serde_json::from_value(serde_json::to_value(original).unwrap()).unwrap();
        let images = Arc::new(Images(Default::default(), image_bytes));
        let registry = talos_tools::ToolRegistry::new();
        registry.set_image_store(images.clone());
        struct Provider(Vec<&'static str>);
        #[async_trait::async_trait]
        impl LlmProvider for Provider {
            fn name(&self) -> &str {
                "attachment-validation"
            }
            fn default_model(&self) -> String {
                "test-model".into()
            }
            async fn chat(
                &self,
                request: ChatRequest,
            ) -> TalosResult<talos_llm_provider::ChatResponse> {
                self.validate_request_attachments(&request)?;
                let selected: Vec<_> = request
                    .messages
                    .iter()
                    .flat_map(|message| &message.image_file_ids)
                    .map(String::as_str)
                    .collect();
                assert_eq!(selected, self.0);
                assert_eq!(
                    request
                        .messages
                        .iter()
                        .flat_map(|message| message.attachments.iter().flatten())
                        .count(),
                    1 + self.0.len()
                );
                assert!(request
                    .messages
                    .iter()
                    .any(|message| message.content.contains("omitted")));
                Ok(talos_llm_provider::ChatResponse {
                    content: "Latest screenshots received".into(),
                    reasoning: Default::default(),
                    model: request.model,
                    usage: Default::default(),
                    finish_reason: Some("stop".into()),
                    tool_calls: None,
                })
            }
            async fn stream(&self, _: ChatRequest) -> TalosResult<talos_llm_provider::ChatStream> {
                unreachable!()
            }
            fn validate_attachments(
                &self,
                _: &str,
                images: &[Attachment],
            ) -> TalosResult<talos_llm_provider::AttachmentUsage> {
                talos_llm_provider::validate_inline_attachments(images, |image| {
                    image.media_type == "image/png"
                })
            }
        }
        let call = FunctionCall::new("resume", "echo", serde_json::json!({}));
        registry.register_external_dynamic_tool(talos_tools::RegisteredToolSpec::new(
            talos_llm_provider::ToolDefinition::new("echo", "External confirmation", None),
            talos_tools::ToolSource::Dynamic,
            talos_tools::ToolPayloadKind::Function,
            talos_tools::ToolParallelism::ParallelSafe,
            false,
            talos_tools::default_governance_for_source(talos_tools::ToolSource::Dynamic),
        ));
        let resume = AgentKernelContinuation::UserAction(AgentKernelUserActionContinuation {
            call_id: call.id.clone(),
            tool_name: call.name.clone(),
            response: talos_types::UserActionResponse::DynamicToolResult(
                talos_types::DynamicToolResult {
                    output: "continue".into(),
                    success: true,
                },
            ),
            sandbox_request_fingerprint: None,
            original_call: None,
            tool_calls: (0..6)
                .map(|i| {
                    FunctionCall::new(format!("call-{i}"), "screenshot", serde_json::json!({}))
                })
                .chain([call])
                .collect(),
            tool_messages: resumed,
            deferred_tool_calls: vec![],
            replay_dynamic_tool_results: Default::default(),
            execution_metrics: Default::default(),
        });
        let result = execute_general_turn_loop_with_resume(
            prepared,
            Arc::new(Provider(expected)),
            Arc::new(registry),
            &AgentKernelConfig::default(),
            Some(resume),
        )
        .await
        .unwrap();
        assert_eq!(result.output_text, "Latest screenshots received");
        assert_eq!(
            *images.0.lock().unwrap(),
            ["file-5", "file-4", "file-3", "file-2"]
        );
    }
}
