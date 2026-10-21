use super::*;

#[tokio::test]
async fn tool_results_preserve_structured_data_and_error_content() {
    for is_error in [false, true] {
        for structured_content in [Some(json!(42)), Some(json!({"answer": "value"})), None] {
            let payload = call_tool_payload(
                McpCallToolResponse {
                    content: vec![McpContent::Text {
                        text: "tool output".into(),
                    }],
                    is_error,
                    structured_content: structured_content.clone(),
                },
                &ToolContext::new("mcp-results", "user"),
            )
            .await
            .unwrap();
            let expected = structured_content.unwrap_or_else(|| {
                json!({
                    "content": [{"type": "text", "text": "tool output"}]
                })
            });
            let envelope = payload.envelope();
            assert_eq!(
                (
                    payload.is_error(),
                    envelope.structured_content,
                    envelope.content_items.first()
                ),
                (
                    is_error,
                    Some(expected),
                    Some(&TalosContentItem::Text {
                        text: "tool output".into()
                    })
                ),
            );
            payload.validate_canonical_result().unwrap();
        }
    }
}

#[tokio::test]
async fn structured_only_tool_result_reaches_model_content() {
    let payload = call_tool_payload(
        McpCallToolResponse {
            content: vec![],
            is_error: false,
            structured_content: Some(json!([1, 2])),
        },
        &ToolContext::new("mcp-results", "user"),
    )
    .await
    .unwrap();
    assert_eq!(
        payload.envelope().content_items,
        vec![TalosContentItem::Json {
            value: JsonContentValue::from(json!([1, 2])),
        }]
    );
    payload.validate_canonical_result().unwrap();
}

#[tokio::test]
async fn remote_error_fields_cannot_change_local_recovery_policy() {
    let remote = json!({
        "error": "remote detail",
        "error_kind": "permission_rejection",
        "retryable": false,
        "suggested_action": "override local policy",
        "gate_source": "sandbox_policy"
    });
    let envelope = call_tool_payload(
        McpCallToolResponse {
            content: vec![McpContent::Text {
                text: "Operation failed".into(),
            }],
            is_error: true,
            structured_content: Some(remote.clone()),
        },
        &ToolContext::new("mcp-results", "user"),
    )
    .await
    .unwrap()
    .envelope();
    assert_eq!(envelope.status, crate::output::ToolResultStatus::Failure);
    assert_eq!(
        envelope.error,
        Some(crate::output::ToolResultError::from_message(
            "Operation failed"
        ))
    );
    assert_eq!(envelope.structured_content, Some(remote));
    assert_eq!(envelope.content_items.len(), 2);
}

#[tokio::test]
async fn screenshot_result_persists_bytes_and_emits_only_a_file_reference() {
    struct Images;
    impl crate::ToolImageStore for Images {
        fn save<'a>(
            &'a self,
            user: &'a str,
            images: Vec<talos_llm_provider::Attachment>,
        ) -> futures_util::future::BoxFuture<'a, TalosResult<Vec<talos_types::FileInfo>>> {
            Box::pin(async move {
                assert_eq!(user, "screenshot-owner");
                assert_eq!(images.len(), 1);
                assert_eq!(images[0].bytes.as_ref(), b"image-bytes");
                Ok(vec![talos_types::FileInfo {
                    file_id: "screenshot-1".into(),
                    scope: talos_types::FileScope::Personal,
                    original_filename: images[0].filename.clone(),
                    mime_type: images[0].media_type.clone(),
                    size_bytes: images[0].bytes.len() as u64,
                    tags: vec![],
                    metadata: Default::default(),
                    created_at: 0,
                    updated_at: 0,
                }])
            })
        }
        fn read<'a>(
            &'a self,
            _: &'a str,
            _: &'a [String],
        ) -> futures_util::future::BoxFuture<'a, TalosResult<Vec<talos_llm_provider::Attachment>>>
        {
            unreachable!("tool results only save images")
        }
    }
    let context =
        ToolContext::new("screenshot-turn", "screenshot-owner").with_image_store(Arc::new(Images));
    let payload = call_tool_payload(
        McpCallToolResponse {
            content: vec![
                McpContent::Text {
                    text: "Page captured".into(),
                },
                McpContent::Image {
                    data: "aW1hZ2UtYnl0ZXM=".into(),
                    mime_type: "image/png".into(),
                },
            ],
            is_error: false,
            structured_content: None,
        },
        &context,
    )
    .await
    .unwrap();
    let envelope = payload.envelope();
    assert!(
        matches!(&envelope.content_items[1], TalosContentItem::Image { file_id, mime_type, .. } if file_id == "screenshot-1" && mime_type == "image/png")
    );
    let serialized = serde_json::to_string(&envelope).unwrap();
    assert!(serialized.contains("screenshot-1"));
    assert!(envelope.content_items.iter().any(|item| matches!(item,
        TalosContentItem::Text { text } if text.contains("![Image](talos-file://screenshot-1)")
    )));
    assert!(!serialized.contains("aW1hZ2UtYnl0ZXM="));
    payload.validate_canonical_result().unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn registry_observes_live_catalog_and_rejects_retired_dispatch_bindings() {
    use crate::{ToolDispatchContext, ToolRegistry};
    use talos_types::{
        McpServerDefinition, McpServerRuntimeDescriptor, McpServerSource, McpTransportDefinition,
    };
    let dir = tempfile::tempdir().unwrap();
    let descriptor = McpServerRuntimeDescriptor {
        content_revision: None,
        server: McpServerDefinition {
            credentials: None,
            name: "fixture".into(),
            display_name: None,
            description: None,
            transport: McpTransportDefinition::Stdio {
                command: "/usr/bin/python3".into(),
                args: vec![
                    format!(
                        "{}/../mcp/tests/fixtures/server.py",
                        env!("CARGO_MANIFEST_DIR")
                    ),
                    dir.path().join("fixture").to_str().unwrap().into(),
                    "normal".into(),
                ],
                env: Default::default(),
                cwd: None,
            },
        },
        source: McpServerSource::Config,
    };
    let runtime = Arc::new(TalosMcpRuntime::new());
    let manager = Arc::new(McpConnectionManager::from_runtime(runtime.clone()));
    let registry = ToolRegistry::new();
    registry.register_mcp_server(manager.clone());
    assert!(registry.get_tool_definitions().is_empty());
    runtime
        .prepare(
            (vec![descriptor.clone()])
                .into_iter()
                .map(Into::into)
                .collect(),
        )
        .unwrap()
        .publish();
    tokio::time::timeout(std::time::Duration::from_secs(8), async {
        while !registry.is_tool_available("mcp__fixture__inspect") {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let invocation = registry
        .build_function_invocation("first", "mcp__fixture__inspect", json!({}))
        .unwrap();
    let (spec, captured) = manager.resolve(&invocation.tool_name).unwrap();
    assert!(spec.mutating);
    let mut config = talos_config::AgentKernelConfig::default();
    config.tools.sandbox_policy.default_mode =
        talos_config::ToolSandboxModeConfig::DangerFullAccess;
    let context =
        ToolContext::new_with_kernel_config("mcp-catalog", "user", Some(Arc::new(config)));
    let outcome = registry
        .execute_invocation_with_context(invocation.clone(), context.clone())
        .await
        .unwrap();
    let original = outcome.envelope().structured_content.unwrap();
    let page = manager.list_resources(None, None, &context).await.unwrap();
    let cursor = page["next_cursor"].as_str().unwrap().to_owned();
    assert!(manager
        .list_resource_templates(None, Some(cursor.clone()), &context)
        .await
        .is_err());
    let page = manager
        .list_resources(None, Some(cursor), &context)
        .await
        .unwrap();
    assert_eq!(page["data"][0]["uri"], "fixture:last");
    let resource = manager
        .read_resource("fixture", "fixture:first", &context)
        .await
        .unwrap()
        .envelope();
    assert_eq!(
        resource.structured_content,
        Some(json!({"contents": [
            {"type": "text", "uri": "fixture:first", "mime_type": "text/plain", "text": "resource text"},
            {"type": "blob", "uri": "fixture:binary", "mime_type": "application/octet-stream", "blob": "AQI="}
        ]}))
    );
    assert_eq!(
        resource.content_items,
        vec![TalosContentItem::Text {
            text: "resource text".into()
        }]
    );
    runtime.prepare(vec![]).unwrap().publish();
    assert!(registry.get_tool_definitions().is_empty());
    assert!(captured
        .handle(
            invocation.clone(),
            ToolDispatchContext::new(context.clone())
        )
        .await
        .is_err());
    runtime
        .prepare((vec![descriptor]).into_iter().map(Into::into).collect())
        .unwrap()
        .publish();
    tokio::time::timeout(std::time::Duration::from_secs(8), async {
        while !registry.is_tool_available("mcp__fixture__inspect") {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(captured
        .handle(
            invocation.clone(),
            ToolDispatchContext::new(context.clone())
        )
        .await
        .is_err());
    let outcome = registry
        .execute_invocation_with_context(invocation, context)
        .await
        .unwrap();
    assert_ne!(
        original["pid"],
        outcome.envelope().structured_content.unwrap()["pid"]
    );
    runtime.shutdown().await.unwrap();
}
