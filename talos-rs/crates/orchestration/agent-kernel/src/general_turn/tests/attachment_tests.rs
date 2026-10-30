use super::*;
use bytes::Bytes;
use talos_config::ModelInputKind;
use talos_llm_provider::{
    validate_inline_attachments, Attachment, AttachmentKind, AttachmentUsage,
};

#[derive(Default)]
struct RecordingAttachmentProvider {
    requests: Mutex<Vec<ChatRequest>>,
}

#[async_trait]
impl LlmProvider for RecordingAttachmentProvider {
    fn name(&self) -> &str {
        "recording-attachment"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.requests
            .lock()
            .expect("attachment requests mutex should lock")
            .push(request.clone());
        Ok(ChatResponse {
            content: "files received".to_string(),
            reasoning: ReasoningOutput::default(),
            model: request.model,
            usage: Usage::default(),
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        unreachable!("attachment turn uses model block streaming")
    }

    async fn stream_model_blocks(&self, request: ChatRequest) -> TalosResult<ModelBlockStream> {
        self.requests
            .lock()
            .expect("attachment requests mutex should lock")
            .push(request);
        let (tx, rx) = mpsc::channel(4);
        tokio::spawn(async move {
            tx.send(Ok(ModelBlockStreamEvent::BlockStarted(
                ModelBlockStarted::assistant_text(
                    None,
                    talos_llm_provider::ModelOutputPhase::Unknown,
                ),
            )))
            .await
            .expect("assistant start should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockDelta {
                model_item_id: "assistant_text".to_string(),
                delta: ModelBlockDelta::AssistantText {
                    delta: "files received".to_string(),
                },
            }))
            .await
            .expect("assistant delta should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockCompleted(
                ModelBlockCompleted {
                    model_item_id: "assistant_text".to_string(),
                },
            )))
            .await
            .expect("assistant completed should send");
            tx.send(Ok(ModelBlockStreamEvent::Done {
                finish_reason: Some("stop".to_string()),
                usage: None,
            }))
            .await
            .expect("done event should send");
        });
        Ok(ModelBlockStream::new(rx))
    }

    fn default_model(&self) -> String {
        "m1".to_string()
    }

    fn validate_attachments(
        &self,
        _model: &str,
        attachments: &[Attachment],
    ) -> TalosResult<AttachmentUsage> {
        validate_inline_attachments(attachments, |attachment| {
            matches!(
                (attachment.kind, attachment.media_type.as_str()),
                (AttachmentKind::Image, "image/png") | (AttachmentKind::File, "application/pdf")
            )
        })
    }
}

fn add_attachment_input(ctx: &mut InvocationContext) {
    ctx.input = UserInput {
        text: String::new(),
        file_ids: vec!["image-1".to_string(), "document-1".to_string()],
    };
    ctx.resolved_attachments = vec![
        crate::ResolvedAttachment {
            file_id: "image-1".to_string(),
            filename: "pixel.png".to_string(),
            media_type: "image/png".to_string(),
            size_bytes: 3,
            bytes: Bytes::from_static(&[1, 2, 3]),
        },
        crate::ResolvedAttachment {
            file_id: "document-1".to_string(),
            filename: "note.pdf".to_string(),
            media_type: "application/pdf".to_string(),
            size_bytes: 4,
            bytes: Bytes::from_static(b"%PDF"),
        },
    ];
    ctx.selected_model_info.model.input = vec![ModelInputKind::Image, ModelInputKind::File];
    ctx.selected_model_info.model.context_window = 32_768;
}

#[test]
fn active_turn_input_reuses_ordered_attachment_validation() {
    let provider = RecordingAttachmentProvider::default();
    let mut ctx = build_ctx_with_stream(None);
    add_attachment_input(&mut ctx);

    let active_input = build_active_turn_input(
        ctx.input.clone(),
        ctx.resolved_attachments.clone(),
        &ctx.selected_model_info,
        &provider,
    )
    .expect("active turn attachments should be prepared");

    assert_eq!(active_input.input, ctx.input);
    assert_eq!(
        active_input.message.input_origin,
        Some(talos_llm_provider::MessageInputOrigin {
            kind: talos_llm_provider::MessageInputKind::User,
            source: talos_llm_provider::MessageInputSource::User,
        })
    );
    assert_eq!(
        active_input
            .message
            .attachments
            .as_ref()
            .unwrap()
            .iter()
            .map(|a| a.file_id.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("image-1"), Some("document-1")]
    );
    assert_eq!(
        active_input.message.attachments,
        Some(
            ctx.resolved_attachments
                .iter()
                .map(|resolved| Attachment {
                    file_id: Some(resolved.file_id.clone()),
                    kind: if resolved.media_type.starts_with("image/") {
                        AttachmentKind::Image
                    } else {
                        AttachmentKind::File
                    },
                    filename: resolved.filename.clone(),
                    media_type: resolved.media_type.clone(),
                    bytes: resolved.bytes.clone(),
                })
                .collect()
        )
    );
    assert!(active_input.attachment_tokens > 0);
}

#[tokio::test]
async fn attachment_only_turn_sends_ordered_files_and_persists_only_references() {
    let workspace = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace).expect("temp workspace should exist");
    let memory_store = Arc::new(TestMemoryStore::new());
    let tool_registry = Arc::new(ToolRegistry::new());
    let provider = Arc::new(RecordingAttachmentProvider::default());
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.workspace_dir = workspace.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace);
    let mut ctx = build_ctx_with_stream_for_session("attachment-only", None);
    add_attachment_input(&mut ctx);

    let result = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "",
        provider.clone(),
        tool_registry.clone(),
        tool_registry,
        memory_store,
        &config,
        skill_options_from_config(&workspace, &config),
    )
    .await
    .expect("attachment-only turn should succeed");

    assert_eq!(result.output_text(), "files received");
    {
        let requests = provider
            .requests
            .lock()
            .expect("attachment requests mutex should lock");
        assert_eq!(requests.len(), 1);
        let user_message = requests[0]
            .messages
            .last()
            .expect("current user message should exist");
        assert_eq!(user_message.content, "");
        assert_eq!(
            user_message.input_origin,
            Some(talos_llm_provider::MessageInputOrigin {
                kind: talos_llm_provider::MessageInputKind::User,
                source: talos_llm_provider::MessageInputSource::User,
            })
        );
        assert_eq!(
            user_message
                .attachments
                .as_ref()
                .unwrap()
                .iter()
                .map(|a| a.file_id.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("image-1"), Some("document-1")]
        );
        assert_eq!(
            user_message.attachments.as_ref(),
            Some(
                &ctx.resolved_attachments
                    .iter()
                    .map(|resolved| Attachment {
                        file_id: Some(resolved.file_id.clone()),
                        kind: if resolved.media_type.starts_with("image/") {
                            talos_llm_provider::AttachmentKind::Image
                        } else {
                            talos_llm_provider::AttachmentKind::File
                        },
                        filename: resolved.filename.clone(),
                        media_type: resolved.media_type.clone(),
                        bytes: resolved.bytes.clone(),
                    })
                    .collect::<Vec<_>>()
            )
        );
    }
}

#[tokio::test]
async fn model_capability_rejects_attachments_before_prompt_work() {
    let workspace = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace).expect("temp workspace should exist");
    let memory_store = Arc::new(TestMemoryStore::new());
    let tool_registry = Arc::new(ToolRegistry::new());
    let provider = Arc::new(RecordingAttachmentProvider::default());
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.workspace_dir = workspace.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace);
    let mut ctx = build_ctx_with_stream(None);
    add_attachment_input(&mut ctx);
    ctx.selected_model_info.model.input = vec![ModelInputKind::Text];

    let error = prepare_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "",
        provider,
        tool_registry,
        memory_store,
        &config,
        Some(skill_options_from_config(&workspace, &config)),
    )
    .await
    .expect_err("text-only model must reject attachments");

    assert!(error.is_code(talos_error::ErrorCode::UnsupportedAttachment));
}
