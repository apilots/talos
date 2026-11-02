use super::*;
use serde_json::json;

#[test]
fn model_results_bound_success_and_failure_without_raw_structured_duplicates() {
    for success in [true, false] {
        let path = "/workspace/user/tool-output/session/turn/call/output.log";
        let output = ToolExecutionOutput {
            metrics: None,
            call_id: "large-call".into(),
            tool_name: "Bash".into(),
            arguments: json!({}),
            is_error: !success,
            outcome: ToolDispatchOutcome::executed(ToolOutputPayload::Function {
                content: "raw".repeat(1_000_000),
                success,
                structured_content: Some(
                    json!({"output": "raw-structured-secret".repeat(100_000)}),
                ),
                content_items: vec![TalosContentItem::BashOutput {
                    session_id: None,
                    status: if success {
                        talos_types::TalosToolTaskStatus::Completed
                    } else {
                        talos_types::TalosToolTaskStatus::Failed
                    },
                    exit_code: Some(if success { 0 } else { 7 }),
                    stdout: "中文\n\"".repeat(100_000),
                    stderr: String::new(),
                    truncated: true,
                    elapsed_ms: Some(1),
                    output_log: Some(talos_types::BashOutputLog {
                        path: path.into(),
                        relative_path: "tool-output/session/turn/call/output.log".into(),
                        total_bytes: 1_000_000,
                        stored_bytes: 1_000_000,
                        total_lines: 100_000,
                        capture_status: talos_types::BashOutputCaptureStatus::Complete,
                        expires_at: 1_900_000_000,
                    }),
                }],
            }),
        };
        let batch = collect_bounded_tool_batch(vec![output], &[], 2048).unwrap();
        let message = &batch.tool_messages[0];
        assert!(message.content.len() <= 2048);
        let value = serde_json::from_str::<Value>(&message.content).unwrap();
        let result = if success { &value } else { &value["result"] };
        assert_eq!(result["outputLog"]["path"], path);
        assert_eq!(result["outputLog"]["captureStatus"], "complete");
        assert_eq!(result["exit_code"], if success { 0 } else { 7 });
        assert!(!message.content.contains("raw-structured-secret"));
        assert_eq!(batch.error_count, usize::from(!success));
    }
}

#[test]
fn reduce_tool_execution_output_uses_canonical_failure_envelope() {
    let output = ToolExecutionOutput {
        metrics: None,
        call_id: "call-1".to_string(),
        tool_name: "Read".to_string(),
        arguments: serde_json::json!({"path": "missing"}),
        outcome: ToolDispatchOutcome::executed(ToolOutputPayload::Function {
            content: String::new(),
            success: false,
            structured_content: Some(serde_json::json!({
                "error": "path is required",
                "error_kind": "validation_failure",
                "retryable": true,
                "suggested_action": "fix the arguments and retry",
                "gate_source": "tool_semantic"
            })),
            content_items: vec![TalosContentItem::Text {
                text: "path is required".to_string(),
            }],
        }),
        is_error: true,
    };

    let reduced = reduce_tool_execution_output(&output);
    assert!(reduced.is_error);
    assert_eq!(
        reduced.envelope.status,
        talos_tools::ToolResultStatus::Failure
    );
}

#[test]
fn reduce_tool_execution_output_uses_canonical_success_payload() {
    let output = ToolExecutionOutput {
        metrics: None,
        call_id: "call-3".to_string(),
        tool_name: "Read".to_string(),
        arguments: serde_json::json!({"path": "README.md"}),
        outcome: ToolDispatchOutcome::executed(ToolOutputPayload::success_text(
            "plain text result",
        )),
        is_error: false,
    };

    let reduced = reduce_tool_execution_output(&output);
    assert!(!reduced.is_error);
    assert_eq!(
        reduced.payload,
        serde_json::json!({
            "type": "text",
            "text": "plain text result",
            "summary": "plain text result"
        })
    );
    assert_eq!(
        reduced.content_items,
        vec![TalosContentItem::Text {
            text: "plain text result".to_string()
        }]
    );
}

#[test]
fn memory_get_keeps_the_full_tool_result() {
    let mut state = GeneralTurnLoopState {
        started_at: Instant::now(),
        messages: Vec::new(),
        round: 0,
        output_items: Vec::new(),
        usage: TokenUsage::default(),
        last_model_usage: TokenUsage::default(),
        reasoning_log: Vec::new(),
        tool_call_log: Vec::new(),
        tool_result_log: Vec::new(),
        retrieved_memories: Vec::new(),
        tool_call_count: 0,
        tool_result_count: 0,
        tool_error_count: 0,
        pending_user_action: None,
        final_answer_streamed: false,
        execution_summary_streamed: false,
        elapsed_duration_ms: 0,
        active_turn_attachment_tokens: 0,
        browser_uploads: Vec::new(),
    };
    let body = "x".repeat(1_500);
    let output = ToolExecutionOutput {
        metrics: None,
        call_id: "call-memory".to_string(),
        tool_name: "MemoryGet".to_string(),
        arguments: serde_json::json!({"memory_id": "mem-1"}),
        outcome: ToolDispatchOutcome::executed(
            ToolOutputPayload::success_json(
                &MemoryGetResponse {
                    memory: talos_types::MemoryInfo {
                        memory_id: "mem-1".to_string(),
                        title: "Preference".to_string(),
                        body: body.clone(),
                        kind: talos_types::MemoryKind::Preference,
                        importance: talos_types::MemoryImportance::High,
                        occurred_at: 1,
                        source: talos_types::MemorySourceInfo {
                            date: "2026-07-17".to_string(),
                            session_id: None,
                            from_seq: None,
                            until_seq: None,
                        },
                    },
                },
                "serialize test memory",
            )
            .expect("memory payload"),
        ),
        is_error: false,
    };
    let batch = collect_general_turn_tool_batch(vec![output]).expect("collect memory batch");
    let mut config = AgentKernelConfig::default();
    config.compaction.retrieved_memory_max_chars = 2_000;

    apply_general_turn_tool_batch(&mut state, &batch, &config);
    apply_general_turn_tool_batch(&mut state, &batch, &config);

    assert!(state.retrieved_memories.is_empty());
    assert!(state.messages.iter().any(|message| {
        message.role == talos_llm_provider::Role::Tool && message.content.contains(&body)
    }));
    assert!(state
        .messages
        .iter()
        .all(|message| !message.content.contains("Retrieved memories:")));
}

#[test]
fn reduce_tool_execution_output_rejects_structured_only_success_payload() {
    let output = ToolExecutionOutput {
        metrics: None,
        call_id: "call-4".to_string(),
        tool_name: "BadTool".to_string(),
        arguments: serde_json::json!({}),
        outcome: ToolDispatchOutcome::executed(ToolOutputPayload::Function {
            content: "{}".to_string(),
            success: true,
            structured_content: Some(serde_json::json!({"result": "hidden"})),
            content_items: Vec::new(),
        }),
        is_error: false,
    };

    let reduced = reduce_tool_execution_output(&output);
    assert!(reduced.is_error);
    assert_eq!(
        reduced.content_items,
        vec![TalosContentItem::Text {
            text: "canonical tool result requires content_items".to_string()
        }]
    );
    assert_ne!(
        reduced.payload,
        serde_json::json!({"result": "hidden"}),
        "model projection must not use structured_content as the primary payload"
    );
}

#[test]
fn repeated_approvals_preserve_only_the_unexecuted_tool_suffix() {
    let arguments = serde_json::json!({
        "command": "printf approved > marker",
        "timeout": 3000
    });
    let output = ToolExecutionOutput {
        metrics: None,
        call_id: "call-approval".to_string(),
        tool_name: "Bash".to_string(),
        arguments: arguments.clone(),
        outcome: ToolDispatchOutcome::pending_sandbox_approval(
            "Bash",
            talos_types::SandboxApprovalParams {
                tool_name: "Bash".to_string(),
                reason: talos_types::SandboxApprovalReason::RiskyCommand,
                presentation: talos_types::SandboxApprovalPresentation::Command {
                    command: "printf approved > marker".to_string(),
                    cwd: "/workspace".to_string(),
                    risk_summary: None,
                },
                available_decisions: vec![talos_types::SandboxApprovalDecision::AllowOnce],
            },
            "sar_scope".to_string(),
        ),
        is_error: false,
    };

    let calls = vec![
        FunctionCall::new("call-approval", "Bash", arguments.clone()),
        FunctionCall::new(
            "call-after-approval",
            "Bash",
            serde_json::json!({"command": "printf second"}),
        ),
        FunctionCall::new(
            "call-after-second-approval",
            "Read",
            serde_json::json!({"path": "README.md"}),
        ),
    ];
    let batch = collect_general_turn_tool_batch_for_calls(vec![output], &calls)
        .expect("collect first approval");

    let pending = batch
        .pending_user_action
        .expect("first approval should be pending");
    assert_eq!(pending.arguments, arguments);
    assert_eq!(
        pending.deferred_tool_calls,
        vec![calls[1].clone(), calls[2].clone()]
    );

    let resumed_outputs = vec![
        ToolExecutionOutput {
            metrics: None,
            call_id: calls[0].id.clone(),
            tool_name: calls[0].name.clone(),
            arguments: calls[0].arguments.clone(),
            outcome: ToolDispatchOutcome::executed(ToolOutputPayload::success_text("first")),
            is_error: false,
        },
        ToolExecutionOutput {
            metrics: None,
            call_id: calls[1].id.clone(),
            tool_name: calls[1].name.clone(),
            arguments: calls[1].arguments.clone(),
            outcome: ToolDispatchOutcome::pending_sandbox_approval(
                "Bash",
                talos_types::SandboxApprovalParams {
                    tool_name: "Bash".to_string(),
                    reason: talos_types::SandboxApprovalReason::RiskyCommand,
                    presentation: talos_types::SandboxApprovalPresentation::Command {
                        command: "printf second".to_string(),
                        cwd: "/workspace".to_string(),
                        risk_summary: None,
                    },
                    available_decisions: vec![talos_types::SandboxApprovalDecision::AllowOnce],
                },
                "sar_scope_second".to_string(),
            ),
            is_error: false,
        },
    ];
    let resumed_batch = collect_general_turn_tool_batch_for_calls(resumed_outputs, &calls)
        .expect("collect second approval");

    let pending = resumed_batch
        .pending_user_action
        .expect("second approval should be pending");
    assert_eq!(pending.deferred_tool_calls, vec![calls[2].clone()]);
}

#[test]
fn tool_batch_never_exposes_multiple_pending_user_actions() {
    let outputs = ["call-first", "call-second"]
        .into_iter()
        .map(|call_id| ToolExecutionOutput {
            metrics: None,
            call_id: call_id.to_string(),
            tool_name: "AskUserQuestion".to_string(),
            arguments: serde_json::json!({"question": call_id}),
            outcome: ToolDispatchOutcome::pending_dynamic(
                "AskUserQuestion",
                serde_json::json!({"question": call_id}),
            ),
            is_error: false,
        })
        .collect();

    let error = collect_general_turn_tool_batch(outputs)
        .expect_err("multiple pending actions must fail the tool batch");

    assert!(error.to_string().contains("multiple pending user actions"));
}
