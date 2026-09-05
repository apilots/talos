use super::*;

#[test]
fn turn_requests_reject_workspace_override() {
    let start = serde_json::json!({
        "session_id": "session-1",
        "input": {"content": [{"type": "text", "text": "hello"}]},
        "workspace_id": "workspace-other"
    });
    let retry = serde_json::json!({
        "session_id": "session-1",
        "input": {"content": [{"type": "text", "text": "hello"}]},
        "workspace_id": "workspace-other",
        "client_message_id": "message-1",
        "retry_of_message_id": "message-0"
    });

    assert!(serde_json::from_value::<StartTurnRequest>(start).is_err());
    assert!(serde_json::from_value::<RetryTurnRequest>(retry).is_err());
}

#[test]
fn user_permission_mode_uses_product_wire_values() {
    assert_eq!(
        serde_json::to_value([
            UserPermissionMode::AskForApproval,
            UserPermissionMode::ApproveForMe,
            UserPermissionMode::FullAccess,
        ])
        .unwrap(),
        serde_json::json!(["askForApproval", "approveForMe", "fullAccess"])
    );
}

#[test]
fn pending_user_action_serializes_canonical_question_shape() {
    let action = PendingUserAction {
        action_id: "action-1".to_string(),
        call_id: "call-1".to_string(),
        item_id: Some("item-1".to_string()),
        state: UserActionState::Pending,
        params: UserActionParams::Question(QuestionParams {
            header: "Release".to_string(),
            question: "Publish now?".to_string(),
            options: vec![QuestionOption {
                id: "publish".to_string(),
                label: "Publish now".to_string(),
                description: "Publish the completed work.".to_string(),
            }],
        }),
    };

    assert_eq!(
        serde_json::to_value(action).unwrap(),
        serde_json::json!({
            "actionId": "action-1",
            "callId": "call-1",
            "itemId": "item-1",
            "state": "pending",
            "params": {
                "type": "question",
                "params": {
                    "header": "Release",
                    "question": "Publish now?",
                    "options": [{
                        "id": "publish",
                        "label": "Publish now",
                        "description": "Publish the completed work."
                    }]
                }
            }
        })
    );
}

#[test]
fn user_action_response_preserves_variant_specific_data() {
    let responses = [
        UserActionResponse::DynamicToolResult(DynamicToolResult {
            output: "completed".to_string(),
            success: true,
        }),
        UserActionResponse::Question(QuestionAnswer::SelectedOption {
            option_id: "publish".to_string(),
        }),
        UserActionResponse::Question(QuestionAnswer::FreeForm {
            text: "Publish after review".to_string(),
        }),
        UserActionResponse::SandboxApproval(SandboxApprovalDecision::Deny),
    ];

    assert_eq!(
        serde_json::to_value(responses).unwrap(),
        serde_json::json!([
            {
                "type": "dynamicToolResult",
                "response": {
                    "output": "completed",
                    "success": true
                }
            },
            {
                "type": "question",
                "response": {
                    "type": "selectedOption",
                    "optionId": "publish"
                }
            },
            {
                "type": "question",
                "response": {
                    "type": "freeForm",
                    "text": "Publish after review"
                }
            },
            {
                "type": "sandboxApproval",
                "response": "deny"
            }
        ])
    );
}

#[test]
fn user_action_params_preserve_all_variant_shapes() {
    let params = [
        UserActionParams::DynamicToolResult(DynamicToolResultParams {
            tool_name: "external-tool".to_string(),
        }),
        UserActionParams::SandboxApproval(SandboxApprovalParams {
            tool_name: "bash".to_string(),
            reason: SandboxApprovalReason::RiskyCommand,
            presentation: SandboxApprovalPresentation::Command {
                command: "rm build.log".to_string(),
                cwd: "/workspace".to_string(),
                risk_summary: Some("Deletes a file".to_string()),
            },
            available_decisions: vec![
                SandboxApprovalDecision::AllowOnce,
                SandboxApprovalDecision::Deny,
                SandboxApprovalDecision::Cancel,
            ],
        }),
    ];

    assert_eq!(
        serde_json::to_value(params).unwrap(),
        serde_json::json!([
            {
                "type": "dynamicToolResult",
                "params": { "toolName": "external-tool" }
            },
            {
                "type": "sandboxApproval",
                "params": {
                    "toolName": "bash",
                    "reason": "riskyCommand",
                    "presentation": {
                        "type": "command",
                        "command": "rm build.log",
                        "cwd": "/workspace",
                        "riskSummary": "Deletes a file"
                    },
                    "availableDecisions": ["allowOnce", "deny", "cancel"]
                }
            }
        ])
    );
}

#[test]
fn sandbox_approval_presentations_use_stable_tagged_shapes() {
    let presentations = [
        SandboxApprovalPresentation::FileSystem {
            operation: SandboxFileOperation::Write,
            paths: vec!["/tmp/output".to_string()],
        },
        SandboxApprovalPresentation::Network {
            scope: "internet".to_string(),
        },
    ];

    assert_eq!(
        serde_json::to_value(presentations).unwrap(),
        serde_json::json!([
            {
                "type": "fileSystem",
                "operation": "write",
                "paths": ["/tmp/output"]
            },
            {
                "type": "network",
                "scope": "internet"
            }
        ])
    );
}

fn model_info(provider: &str, id: &str, name: &str, reasoning: bool) -> ModelInfo {
    ModelInfo::from_provider_model(
        provider,
        ProviderModelConfig {
            id: id.to_string(),
            name: name.to_string(),
            context_window: 1_000_000,
            reasoning,
            reasoning_effort: ReasoningEffort::Medium,
            speed: ModelSpeed::Standard,
            input: vec![ModelInputKind::Text],
            cost: ModelCostConfig::default(),
        },
    )
}

#[test]
fn test_model_catalog_preserves_provider_order_and_duplicate_ids() {
    let mut llm = LlmConfig::default();
    let shared_model = ProviderModelConfig {
        id: "shared-model".to_string(),
        name: "Shared Model".to_string(),
        context_window: 128_000,
        reasoning: true,
        reasoning_effort: ReasoningEffort::High,
        speed: ModelSpeed::Standard,
        input: vec![ModelInputKind::Text],
        cost: ModelCostConfig::default(),
    };
    llm.anthropic.models = vec![shared_model.clone()];
    llm.openai.models = vec![shared_model.clone()];

    assert_eq!(
        resolve_model_catalog(&llm),
        vec![
            ModelInfo::from_provider_model("anthropic", shared_model.clone()),
            ModelInfo::from_provider_model("openai", shared_model),
        ]
    );
}

#[test]
fn test_model_info_serializes_canonical_model_fields_flat() {
    let model = model_info("openai", "gpt-5.5", "GPT-5.5", true);

    assert_eq!(
        serde_json::to_value(model).unwrap(),
        serde_json::json!({
            "provider": "openai",
            "id": "gpt-5.5",
            "name": "GPT-5.5",
            "context_window": 1_000_000,
            "reasoning": true,
            "reasoning_effort": "medium",
            "speed": "standard",
            "input": ["text"],
            "cost": {
                "input": 0.0,
                "output": 0.0,
                "cache_read": 0.0,
                "cache_write": 0.0
            }
        })
    );
}

#[test]
fn test_memory_importance_accepts_medium_as_normal() {
    let parsed: MemoryImportance =
        serde_json::from_str("\"medium\"").expect("medium importance should decode");

    assert_eq!(parsed, MemoryImportance::Normal);
    assert_eq!(
        serde_json::to_string(&parsed).expect("importance should encode"),
        "\"normal\""
    );
}

#[test]
fn test_agent_id() {
    let id = AgentId::new("test");
    assert_eq!(id.0, "test");

    let id2 = AgentId::new_random();
    assert!(!id2.0.is_empty());
}

#[test]
fn test_session_id() {
    let id = SessionId::new();
    assert!(!id.0.is_empty());
}

#[test]
fn test_context_usage_preserves_base_metrics() {
    let usage = ContextUsage::new(1, 128, 96, 42).unwrap();
    assert_eq!(
        usage,
        ContextUsage {
            used_tokens: 1,
            context_window_tokens: 128,
            effective_prompt_budget_tokens: 96,
            measured_at: 42,
        }
    );
    assert_eq!(usage.remaining_tokens(), 127);
    assert_eq!(usage.used_percent(), 0.78125);
    assert_eq!(
        ContextUsage::new(200, 128, 256, 42),
        Some(ContextUsage {
            used_tokens: 200,
            context_window_tokens: 128,
            effective_prompt_budget_tokens: 256,
            measured_at: 42,
        })
    );
    assert_eq!(ContextUsage::new(1, 0, 0, 42), None);
}

#[test]
fn test_session_usage_serializes_tokens_and_nullable_camel_case_context() {
    let usage = SessionUsage {
        tokens: talos::TokenUsage {
            input_tokens: 12,
            output_tokens: 3,
            total_tokens: 15,
            ..talos::TokenUsage::default()
        },
        context: ContextUsage::new(8, 100, 80, 42),
    };

    assert_eq!(
        serde_json::to_value(usage).unwrap(),
        serde_json::json!({
            "tokens": {
                "input_tokens": 12,
                "output_tokens": 3,
                "total_tokens": 15,
                "cache_creation_input_tokens": null,
                "cache_read_input_tokens": null
            },
            "context": {
                "usedTokens": 8,
                "contextWindowTokens": 100,
                "effectivePromptBudgetTokens": 80,
                "measuredAt": 42
            }
        })
    );
}

#[test]
fn test_session_override_policy_accepts_model_selection() {
    let conf = SessionConf {
        model_selection: Some(ModelSelection {
            model_ref: ModelRef {
                provider: "openai".to_string(),
                id: "gpt-4o".to_string(),
            },
            reasoning_effort: None,
        }),
        ..SessionConf::default()
    };
    SessionConfigOverridePolicy::validate_conf(&conf)
        .expect("known strong typed conf should be accepted");
}

#[test]
fn test_session_override_policy_builds_model_selection_conf() {
    let selection = ModelSelection {
        model_ref: ModelRef {
            provider: "openai".to_string(),
            id: "gpt-4o".to_string(),
        },
        reasoning_effort: Some(ReasoningEffort::High),
    };
    let conf = SessionConfigOverridePolicy::single_model_selection_conf(&selection);
    let extracted = SessionConfigOverridePolicy::extract_model_selection(&conf)
        .expect("conf extraction should succeed");

    assert_eq!(extracted, Some(selection));
}

#[test]
fn test_model_selection_uses_configured_default_effort_when_created_from_model() {
    let model = model_info("openai", "gpt-5.5", "GPT-5.5", true);
    let selection = ModelSelection::from_model(
        ModelRef {
            provider: "openai".to_string(),
            id: "gpt-5.5".to_string(),
        },
        &model.model,
    );

    assert_eq!(
        selection.resolve_reasoning(&model).unwrap(),
        Some(ReasoningEffort::Medium)
    );
}

#[test]
fn test_model_selection_resolves_explicit_effort() {
    let selection = ModelSelection {
        model_ref: ModelRef {
            provider: "openai".to_string(),
            id: "gpt-5.5".to_string(),
        },
        reasoning_effort: Some(ReasoningEffort::XHigh),
    };
    let model = model_info("openai", "gpt-5.5", "GPT-5.5", true);

    assert_eq!(
        selection.resolve_reasoning(&model).unwrap(),
        Some(ReasoningEffort::XHigh)
    );
}

#[test]
fn test_model_selection_resolves_disabled_reasoning() {
    let selection = ModelSelection {
        model_ref: ModelRef {
            provider: "ollama".to_string(),
            id: "qwen".to_string(),
        },
        reasoning_effort: None,
    };
    let model = model_info("ollama", "qwen", "Qwen", false);

    assert_eq!(selection.resolve_reasoning(&model).unwrap(), None);
}

#[test]
fn test_model_selection_rejects_explicit_effort_for_unsupported_model() {
    let selection = ModelSelection {
        model_ref: ModelRef {
            provider: "ollama".to_string(),
            id: "qwen".to_string(),
        },
        reasoning_effort: Some(ReasoningEffort::High),
    };
    let model = model_info("ollama", "qwen", "Qwen", false);

    let error = selection
        .resolve_reasoning(&model)
        .expect_err("explicit effort should be rejected");

    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
}

#[test]
fn test_model_selection_serializes_as_single_wire_object() {
    let selection = ModelSelection {
        model_ref: ModelRef {
            provider: "openai".to_string(),
            id: "gpt-5.5".to_string(),
        },
        reasoning_effort: Some(ReasoningEffort::High),
    };

    let encoded = serde_json::to_value(&selection).unwrap();

    assert_eq!(
        encoded,
        serde_json::json!({
            "modelRef": {"provider": "openai", "id": "gpt-5.5"},
            "reasoningEffort": "high"
        })
    );
}

#[test]
fn test_session_title_policy_normalizes_generated_title() {
    let title =
        session_title_or_fallback("\"Plan Service Migration.\"".to_string(), "fallback text");

    assert_eq!(title, "Plan Service Migration");
}

#[test]
fn test_session_title_policy_falls_back_to_first_user_text() {
    let title = session_title_or_fallback("Untitled Session".to_string(), "  migrate phase one\n");

    assert_eq!(title, "migrate phase one");
}

#[test]
fn test_session_patch_roundtrip_with_current_directory() {
    let patch = SessionPatch {
        conf: None,
        current_directory: Some(OptionalCurrentDirectoryPatch::Set {
            path: std::path::PathBuf::from("/repo"),
        }),
    };

    let encoded = serde_json::to_string(&patch).unwrap();
    let decoded: SessionPatch = serde_json::from_str(&encoded).unwrap();

    assert_eq!(
        decoded.current_directory,
        Some(OptionalCurrentDirectoryPatch::Set {
            path: std::path::PathBuf::from("/repo"),
        })
    );
}

#[test]
fn test_doc_update_request_roundtrip() {
    let request = DocUpdateRequest {
        content: "hello".to_string(),
        base_version: ExpectedFileVersion::Match {
            version_token: "token-1".to_string(),
        },
    };

    let encoded = serde_json::to_string(&request).unwrap();
    let decoded: DocUpdateRequest = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded, request);
}

#[test]
fn test_memory_file_summary_roundtrip() {
    let summary = MemoryFileSummary {
        date: "2026-04-17".to_string(),
        section_count: 3,
        updated_at: Some(123),
        version_token: "token-1".to_string(),
    };

    let encoded = serde_json::to_string(&summary).unwrap();
    let decoded: MemoryFileSummary = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded, summary);
}

#[test]
fn test_workspace_doc_kind_file_name() {
    assert_eq!(WorkspaceDocKind::Soul.file_name(), "SOUL.md");
    assert_eq!(WorkspaceDocKind::Identity.file_name(), "IDENTITY.md");
    assert_eq!(WorkspaceDocKind::User.file_name(), "USER.md");
    assert_eq!(WorkspaceDocKind::Heartbeat.file_name(), "HEARTBEAT.md");
    assert_eq!(WorkspaceDocKind::Memory.file_name(), "MEMORY.md");
}

#[test]
fn test_workspace_doc_detail_roundtrip() {
    let detail = WorkspaceDocInfo {
        kind: WorkspaceDocKind::Memory,
        path: "MEMORY.md".to_string(),
        exists: true,
        content: "content".to_string(),
        updated_at: Some(123),
        version_token: Some("token-1".to_string()),
    };

    let encoded = serde_json::to_string(&detail).unwrap();
    let decoded: WorkspaceDocInfo = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded, detail);
}

#[test]
fn test_memory_file_detail_roundtrip() {
    let detail = MemoryFileDetail {
        date: "2026-04-17".to_string(),
        section_count: 4,
        content: "content".to_string(),
        updated_at: Some(123),
        version_token: "token-1".to_string(),
    };

    let encoded = serde_json::to_string(&detail).unwrap();
    let decoded: MemoryFileDetail = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded, detail);
}
