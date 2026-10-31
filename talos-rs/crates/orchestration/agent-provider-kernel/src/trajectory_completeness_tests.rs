use super::*;
use talos_error::{ErrorCode, TalosError};

#[tokio::test]
async fn provider_terminal_event_keeps_following_validation_error_and_known_usage() {
    let (writer, capture) = setup();
    let observation = capture
        .recorder()
        .start_model(KernelObservedPurpose::Working, "c", &request())
        .unwrap();
    observation.model_event(&ModelBlockStreamEvent::Done {
        finish_reason: Some("stop".into()),
        usage: Some(talos_llm_provider::Usage {
            prompt_tokens: 10,
            completion_tokens: 2,
            total_tokens: 12,
            ..Default::default()
        }),
    });
    let error = TalosError::internal("Provider stream completed with an active model block.");
    observation.finished(Some(&error));
    drop(observation);
    capture.flush(&writer).await;
    let records = writer.records.lock().unwrap();
    assert_eq!(records.len(), 1);
    let last = &records[0];
    let attempt = &last.requests[0].request.call_records[0];
    assert_eq!(attempt.observed_state, ModelCallState::Failed);
    assert_eq!(attempt.error, Some(error.to_api_error()));
    assert_eq!(
        attempt
            .metrics
            .as_ref()
            .unwrap()
            .usage
            .as_ref()
            .unwrap()
            .total_tokens,
        12
    );
    assert!(attempt.metrics.as_ref().unwrap().finished_at_ms.is_some());
    assert_eq!(
        last.requests[0].outputs[0]
            .response
            .as_ref()
            .unwrap()
            .complete,
        Some(true)
    );
}

#[tokio::test]
async fn nonstream_protocol_argument_text_survives_the_capture_boundary() {
    use talos_llm_provider::protocols::{chat, responses};
    let raw = " { \"command\" : \"echo 你好\" }\n";
    let chat_response = chat::chat_response(
        serde_json::from_value(serde_json::json!({
            "model":"bound-model",
            "choices":[{"message":{"content":"using tool","tool_calls":[{
                "id":"call", "type":"function", "function":{"name":"bash","arguments":raw}
            }]},"finish_reason":"tool_calls"}]
        }))
        .unwrap(),
        "fixture",
    )
    .unwrap();
    let responses_response = responses::chat_response(
        serde_json::from_value(serde_json::json!({
            "model":"bound-model", "status":"completed",
            "output":[{"type":"function_call","call_id":"call","name":"bash","arguments":raw}]
        }))
        .unwrap(),
    );
    for response in [chat_response, responses_response] {
        let original_call = &response.get_tool_calls()[0];
        assert_eq!(original_call.arguments_text.as_deref(), Some(raw));
        assert_eq!(
            serde_json::to_value(original_call).unwrap(),
            serde_json::json!({
                "id":"call", "name":"bash", "arguments":{"command":"echo 你好"}
            })
        );
        assert_eq!(
            original_call,
            &FunctionCall::new("call", "bash", serde_json::json!({"command":"echo 你好"}))
        );
        let (writer, capture) = setup();
        let observation = capture
            .recorder()
            .start_model(KernelObservedPurpose::Working, "c", &request())
            .unwrap();
        observation.response(&response);
        observation.tool_calls(response.get_tool_calls());
        observation.finished(None);
        drop(observation);
        capture.flush(&writer).await;
        let records = writer.records.lock().unwrap();
        assert_eq!(records.len(), 1);
        let last = &records[0];
        let TrajectoryPayload::Response { parts } = last
            .payloads
            .values()
            .find(|p| matches!(p, TrajectoryPayload::Response { .. }))
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(
            last.tools[0].tool.source_model_request_id.as_ref(),
            Some(&last.requests[0].request.model_request_id)
        );
        assert_eq!(
            last.tools[0].tool.source_call_record_id.as_ref(),
            Some(&last.requests[0].request.call_records[0].call_record_id)
        );
        let call = parts
            .iter()
            .find_map(|p| {
                if let ResponsePart::ToolCall { call } = p {
                    Some(call)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(call.arguments_text.as_deref(), Some(raw));
        assert_eq!(
            call.arguments,
            serde_json::json!({"command":"echo 你好"}).into()
        );
    }
}

#[tokio::test]
async fn failed_stream_keeps_raw_prefix_without_invented_tool_identity() {
    let (history, capture) = setup();
    let observation = capture
        .recorder()
        .start_model(KernelObservedPurpose::Working, "c", &request())
        .unwrap();
    observation.model_event(&ModelBlockStreamEvent::BlockStarted(
        ModelBlockStarted::tool_call(0, None, Some("Read".into())),
    ));
    let fragments = [r#" { "path": "#, r#""src/"#];
    for fragment in fragments {
        observation.model_event(&ModelBlockStreamEvent::BlockDelta {
            model_item_id: "tool_call_0".into(),
            delta: ModelBlockDelta::ToolArguments {
                delta: fragment.into(),
            },
        });
    }
    let error = TalosError::new(ErrorCode::ServiceUnavailable, "Provider stream failed.");
    observation.finished(Some(&error));
    capture.flush(&history).await;
    let records = history.records.lock().unwrap();
    let detail = &records[0].requests[0];
    assert_eq!(
        detail.request.call_records[0].observed_state,
        ModelCallState::Failed
    );
    assert_eq!(
        detail.request.call_records[0].error,
        Some(error.to_api_error())
    );
    let reference = detail.outputs[0].response.as_ref().unwrap();
    assert_eq!(reference.complete, Some(false));
    let TrajectoryPayload::Response { parts } = &records[0].payloads[&reference.payload_id] else {
        panic!()
    };
    let ResponsePart::ToolCall { call } = &parts[0] else {
        panic!()
    };
    assert_eq!(call.call_id, "");
    assert_eq!(
        call.arguments_text.as_deref(),
        Some(fragments.concat().as_str())
    );
}

#[tokio::test]
async fn prompt_snapshots_preserve_public_replay_without_private_continuation_data() {
    use talos_llm_provider::{
        protocols::chat, ReasoningBlock, ReasoningContinuation, ReasoningEncryptedBlock,
        ReasoningSummaryBlock, ReasoningTextBlock,
    };
    let (history, capture) = setup();
    let expected = [
        vec![SnapshotReasoningPart::Text {
            text: "first public replay".into(),
        }],
        vec![SnapshotReasoningPart::Text {
            text: "different public replay".into(),
        }],
        vec![
            SnapshotReasoningPart::Text {
                text: "public text".into(),
            },
            SnapshotReasoningPart::Summary {
                text: "public summary".into(),
            },
        ],
    ];
    let continuations = [
        ReasoningContinuation::Text("first public replay".into()),
        ReasoningContinuation::Text("different public replay".into()),
        ReasoningContinuation::Blocks(vec![
            ReasoningBlock::Text(ReasoningTextBlock {
                provider_item_id: None,
                provider_index: None,
                text: "public text".into(),
                signature: Some("private-signature".into()),
                format: None,
            }),
            ReasoningBlock::Encrypted(ReasoningEncryptedBlock {
                provider_item_id: None,
                provider_index: None,
                data: "private-encrypted-data".into(),
                format: None,
            }),
            ReasoningBlock::Summary(ReasoningSummaryBlock {
                provider_item_id: None,
                provider_index: None,
                text: "public summary".into(),
                format: None,
            }),
        ]),
    ];
    for continuation in continuations {
        let message =
            ChatMessage::assistant("same content").with_reasoning_continuation(continuation);
        if let Some(ReasoningContinuation::Text(text)) = &message.reasoning_continuation {
            let wire = chat::messages(
                vec![message.clone()],
                /*cache_control*/ false,
                chat::ChatReasoningContinuationFormat::ReasoningContent,
            )
            .unwrap();
            assert_eq!(wire[0].reasoning_content.as_ref(), Some(text));
        }
        let mut input = request();
        input.messages = vec![message];
        let observation = capture
            .recorder()
            .start_model(KernelObservedPurpose::Working, "c", &input)
            .unwrap();
        done(&*observation);
        observation.finished(None);
    }
    capture.flush(&history).await;
    let records = history.records.lock().unwrap();
    let saved = &records[0];
    assert_ne!(saved.requests[0].prompt, saved.requests[1].prompt);
    for (detail, expected) in saved.requests.iter().zip(expected) {
        let reference = detail.prompt.as_ref().unwrap();
        assert_eq!(reference.complete, Some(true));
        let TrajectoryPayload::Prompt { messages } = &saved.payloads[&reference.payload_id] else {
            panic!("prompt expected")
        };
        assert_eq!(messages[0].reasoning.as_ref(), Some(&expected));
    }
    let serialized = serde_json::to_string(&saved.payloads).unwrap();
    assert!(!serialized.contains("private-signature"));
    assert!(!serialized.contains("private-encrypted-data"));
}
