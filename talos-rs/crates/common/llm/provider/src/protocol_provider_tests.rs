use super::*;
use crate::{
    apply_thinking_selection, ChatMessage, FunctionCall, ModelBlockStreamEvent, ReasoningBlock,
    ReasoningContinuation, ReasoningEncryptedBlock, ReasoningTextBlock,
};
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

fn provider(protocol: LlmProtocol, replay: ReasoningReplay) -> ProtocolProvider {
    let config: ProviderConfig = serde_json::from_value(json!({
        "base_url":"http://127.0.0.1:9/private-prefix/v1", "models":[{"id":"same-model","reasoning":true,"reasoning_replay":replay}]
    })).unwrap();
    ProtocolProvider::new("user-connection", protocol, &config, "same-model").unwrap()
}

fn tool_history(continuation: ReasoningContinuation) -> ChatRequest {
    ChatRequest::builder()
        .model("same-model")
        .messages(vec![
            ChatMessage::user("use echo"),
            ChatMessage::assistant_with_tool_calls(
                "",
                vec![FunctionCall {
                    id: "call-1".into(),
                    name: "echo".into(),
                    arguments: json!({"text":"hello"}),
                }],
            )
            .with_reasoning_continuation(continuation),
            ChatMessage::tool("call-1", "echo", "hello"),
        ])
        .build()
}

#[test]
fn chat_replay_policies_change_only_outgoing_reasoning_and_preserve_tool_history() {
    let request = tool_history(ReasoningContinuation::Text("private thought".into()));
    let canonical = request.messages.clone();
    assert!(chat::request(
        request.clone(),
        false,
        chat::ChatCompletionRequestOptions::default()
    )
    .is_err());
    for (replay, field) in [
        (ReasoningReplay::ProtocolDefault, Some("reasoning")),
        (ReasoningReplay::Reasoning, Some("reasoning")),
        (ReasoningReplay::ReasoningContent, Some("reasoning_content")),
        (ReasoningReplay::Omit, None),
    ] {
        let (_, body) = provider(LlmProtocol::Chat, replay)
            .body(request.clone(), false)
            .unwrap();
        let mut assistant = body["messages"][1].clone();
        if let Some(field) = field {
            assert_eq!(
                assistant.as_object_mut().unwrap().remove(field),
                Some(json!("private thought"))
            );
        }
        assert_eq!(
            assistant,
            json!({"role":"assistant","content":"","tool_calls":[{"id":"call-1","type":"function","function":{"name":"echo","arguments":"{\"text\":\"hello\"}"}}]})
        );
        assert_eq!(
            body["messages"][2],
            json!({"role":"tool","content":"hello","tool_call_id":"call-1","name":"echo"})
        );
        assert_eq!(request.messages, canonical);
    }
    let opaque = tool_history(ReasoningContinuation::Blocks(vec![
        ReasoningBlock::Encrypted(ReasoningEncryptedBlock {
            provider_item_id: None,
            provider_index: None,
            data: "opaque".into(),
            format: None,
        }),
    ]));
    assert!(
        provider(LlmProtocol::Chat, ReasoningReplay::ProtocolDefault)
            .body(opaque.clone(), false)
            .is_err()
    );
    assert!(provider(LlmProtocol::Chat, ReasoningReplay::Omit)
        .body(opaque, false)
        .is_ok());
}

#[test]
fn thinking_switches_and_effort_are_serialized_independently() {
    for protocol in [
        LlmProtocol::Chat,
        LlmProtocol::Responses,
        LlmProtocol::Messages,
    ] {
        for effort in [None, Some(ReasoningEffort::XHigh)] {
            let mut request = ChatRequest::new_user("hello", "same-model");
            request.reasoning_effort = effort;
            let (_, body) = provider(protocol, ReasoningReplay::ProtocolDefault)
                .body(request.clone(), false)
                .unwrap();
            match protocol {
                LlmProtocol::Chat => assert_eq!(
                    body["reasoning_effort"],
                    if effort.is_some() { "xhigh" } else { "none" }
                ),
                LlmProtocol::Responses => assert_eq!(
                    body["reasoning"]["effort"],
                    if effort.is_some() { "xhigh" } else { "none" }
                ),
                LlmProtocol::Messages => {
                    assert_eq!(
                        body["thinking"],
                        json!({"type":if effort.is_some(){"adaptive"}else{"disabled"}})
                    );
                    assert_eq!(
                        body.get("output_config"),
                        effort.map(|_| json!({"effort":"max"})).as_ref()
                    );
                }
            }
            request.request_parameters =
                serde_json::from_value(json!({"temperature":0.42,"custom":{"nested":[1,false]}}))
                    .unwrap();
            let (_, body) = provider(protocol, ReasoningReplay::ProtocolDefault)
                .body(request, false)
                .unwrap();
            assert_eq!(body["temperature"], 0.42);
            assert_eq!(body["custom"], json!({"nested":[1,false]}));
        }
    }
    for key in ["enable_thinking", "chat_template_kwargs"] {
        let mut request = ChatRequest::new_user("hello", "same-model");
        request.reasoning_effort = Some(ReasoningEffort::High);
        request.request_parameters = if key == "enable_thinking" {
            serde_json::from_value(json!({"enable_thinking":true})).unwrap()
        } else {
            serde_json::from_value(
                json!({"chat_template_kwargs":{"enable_thinking":true,"other":7}}),
            )
            .unwrap()
        };
        let (_, body) = provider(LlmProtocol::Chat, ReasoningReplay::ProtocolDefault)
            .body(request, false)
            .unwrap();
        assert_eq!(body["reasoning_effort"], "high");
    }
    for enabled in [false, true] {
        let mut request = ChatRequest::new_user("hello", "same-model");
        request.reasoning_effort = enabled.then_some(ReasoningEffort::High);
        request.request_parameters=serde_json::from_value(json!({"thinking":{"type":"enabled","budget_tokens":2048},"output_config":{"effort":"low"}})).unwrap();
        apply_thinking_selection(&mut request.request_parameters, enabled);
        let (_, body) = provider(LlmProtocol::Messages, ReasoningReplay::ProtocolDefault)
            .body(request, false)
            .unwrap();
        assert_eq!(
            body["thinking"],
            if enabled {
                json!({"type":"enabled","budget_tokens":2048})
            } else {
                json!({"type":"disabled"})
            }
        );
        assert_eq!(body["output_config"], json!({"effort":"low"}));
    }
}

#[test]
fn structured_messages_and_responses_continuations_retain_signatures_and_identity() {
    let signed = tool_history(ReasoningContinuation::Blocks(vec![ReasoningBlock::Text(
        ReasoningTextBlock {
            provider_item_id: None,
            provider_index: Some(0),
            text: "thought".into(),
            signature: Some("signature".into()),
            format: None,
        },
    )]));
    let (_, body) = provider(LlmProtocol::Messages, ReasoningReplay::ProtocolDefault)
        .body(signed, false)
        .unwrap();
    assert_eq!(
        body["messages"][1]["content"][0],
        json!({"type":"thinking","thinking":"thought","signature":"signature"})
    );
    assert_eq!(body["messages"][1]["content"][1]["id"], "call-1");
    assert_eq!(body["messages"][2]["content"][0]["tool_use_id"], "call-1");
    let opaque = tool_history(ReasoningContinuation::Blocks(vec![
        ReasoningBlock::Encrypted(ReasoningEncryptedBlock {
            provider_item_id: Some("rs-1".into()),
            provider_index: Some(0),
            data: "encrypted".into(),
            format: Some("openai-responses-v1".into()),
        }),
    ]));
    let (_, body) = provider(LlmProtocol::Responses, ReasoningReplay::ProtocolDefault)
        .body(opaque, false)
        .unwrap();
    assert_eq!(body["input"][1]["id"], "rs-1");
    assert_eq!(body["input"][1]["encrypted_content"], "encrypted");
    assert_eq!(body["input"][2]["call_id"], "call-1");
    assert_eq!(body["input"][3]["call_id"], "call-1");
}

async fn capture(
    replies: Vec<(&'static str, String)>,
) -> (String, tokio::task::JoinHandle<Vec<(String, Value)>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/gateway/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for (content_type, body) in replies {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let start = loop {
                let mut chunk = [0; 4096];
                let n = socket.read(&mut chunk).await.unwrap();
                assert_ne!(n, 0);
                bytes.extend_from_slice(&chunk[..n]);
                if let Some(n) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    break n + 4;
                }
            };
            let headers = String::from_utf8(bytes[..start].to_vec()).unwrap();
            let length: usize = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse().unwrap())
                })
                .unwrap();
            while bytes.len() < start + length {
                let mut chunk = [0; 4096];
                let n = socket.read(&mut chunk).await.unwrap();
                assert_ne!(n, 0);
                bytes.extend_from_slice(&chunk[..n]);
            }
            requests.push((
                headers,
                serde_json::from_slice(&bytes[start..start + length]).unwrap(),
            ));
            let response=format!("HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",body.len());
            socket.write_all(response.as_bytes()).await.unwrap();
        }
        requests
    });
    (url, server)
}

#[tokio::test]
async fn three_protocols_use_real_http_nonstream_stream_tools_and_protocol_auth() {
    for protocol in [
        LlmProtocol::Chat,
        LlmProtocol::Responses,
        LlmProtocol::Messages,
    ] {
        let (response,sse,endpoint)=match protocol {
            LlmProtocol::Chat=>(json!({"model":"same-model","choices":[{"message":{"content":"","reasoning":"thought","tool_calls":[{"id":"call-1","type":"function","function":{"name":"echo","arguments":"{\"text\":\"hello\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":3}}),
            "data: {\"choices\":[{\"delta\":{\"content\":\"OK\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n".to_owned(),"chat/completions"),
            LlmProtocol::Responses=>(json!({"model":"same-model","output":[{"type":"reasoning","id":"rs-1","encrypted_content":"opaque","summary":[{"type":"summary_text","text":"thought"}]},{"type":"function_call","id":"fc-1","call_id":"call-1","name":"echo","arguments":"{\"text\":\"hello\"}"}],"status":"completed"}),
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg-1\",\"delta\":\"OK\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n".to_owned(),"responses"),
            LlmProtocol::Messages=>(json!({"model":"same-model","content":[{"type":"thinking","thinking":"thought","signature":"signed"},{"type":"tool_use","id":"call-1","name":"echo","input":{"text":"hello"}}],"stop_reason":"tool_use","usage":{"input_tokens":1,"output_tokens":2}}),
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"OK\"}}\n\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\ndata: {\"type\":\"message_stop\"}\n\n".to_owned(),"messages"),
        };
        let terminal = match protocol {
            LlmProtocol::Chat => {
                json!({"model":"same-model","choices":[{"message":{"content":"OK"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}})
            }
            LlmProtocol::Responses => {
                json!({"model":"same-model","output_text":"OK","output":[],"status":"completed"})
            }
            LlmProtocol::Messages => {
                json!({"model":"same-model","content":[{"type":"text","text":"OK"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}})
            }
        };
        let (url, server) = capture(vec![
            ("application/json", response.to_string()),
            ("application/json", terminal.to_string()),
            ("text/event-stream", sse.clone()),
            ("text/event-stream", sse),
        ])
        .await;
        let config: ProviderConfig = serde_json::from_value(
            json!({"base_url":url,"api_key":"loopback-only-key","models":[{"id":"same-model"}]}),
        )
        .unwrap();
        let provider =
            ProtocolProvider::new("custom-service", protocol, &config, "same-model").unwrap();
        let first = provider
            .chat_with_tools(ChatRequest::new_user("hello", "same-model"))
            .await
            .unwrap();
        assert_eq!(first.tool_calls.as_ref().unwrap()[0].id, "call-1");
        let followup = tool_history(first.reasoning.continuation.unwrap());
        let terminal = provider.chat_with_tools(followup.clone()).await.unwrap();
        assert_eq!(terminal.content, "OK");
        assert!(!terminal.has_tool_calls());
        let mut blocks = provider
            .stream_model_blocks(followup.clone())
            .await
            .unwrap();
        let mut done = false;
        while let Some(event) = blocks.next().await {
            if matches!(event.unwrap(), ModelBlockStreamEvent::Done { .. }) {
                done = true;
            }
        }
        assert!(done);
        let mut stream = provider.stream(followup).await.unwrap();
        let mut text = String::new();
        while let Some(chunk) = stream.next().await {
            text.push_str(&chunk.unwrap().delta);
        }
        assert_eq!(text, "OK");
        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 4);
        let input_key = if protocol == LlmProtocol::Responses {
            "input"
        } else {
            "messages"
        };
        assert_eq!(requests[1].1[input_key], requests[2].1[input_key]);
        assert_eq!(requests[1].1[input_key], requests[3].1[input_key]);
        for (headers, body) in &requests {
            assert!(headers.starts_with(&format!("POST /gateway/v1/{endpoint} ")));
            assert_eq!(body["model"], "same-model");
            let headers = headers.to_ascii_lowercase();
            if protocol == LlmProtocol::Messages {
                assert!(headers.contains("x-api-key: loopback-only-key"));
                assert!(headers.contains("anthropic-version: 2023-06-01"));
                assert!(!headers.contains("authorization:"));
            } else {
                assert!(headers.contains("authorization: bearer loopback-only-key"));
                assert!(!headers.contains("x-api-key:"));
            }
        }
        match protocol {
            LlmProtocol::Chat => {
                assert_eq!(requests[1].1["messages"][1]["reasoning"], "thought");
                assert_eq!(requests[1].1["messages"][2]["tool_call_id"], "call-1");
            }
            LlmProtocol::Messages => {
                assert_eq!(
                    requests[1].1["messages"][1]["content"][0]["signature"],
                    "signed"
                );
                assert_eq!(
                    requests[1].1["messages"][2]["content"][0]["tool_use_id"],
                    "call-1"
                );
            }
            LlmProtocol::Responses => {
                assert_eq!(requests[1].1["input"][1]["encrypted_content"], "opaque");
                assert_eq!(requests[1].1["input"][3]["call_id"], "call-1");
            }
        }
    }
}

#[test]
fn unconfigured_reasoning_capability_does_not_inject_thinking_into_any_protocol() {
    for protocol in [
        LlmProtocol::Chat,
        LlmProtocol::Messages,
        LlmProtocol::Responses,
    ] {
        let config: ProviderConfig = serde_json::from_value(json!({"base_url":"http://127.0.0.1:9/v1","models":[{"id":"opaque-7f82","reasoning":false}]})).unwrap();
        let provider =
            ProtocolProvider::new("opaque-service", protocol, &config, "opaque-7f82").unwrap();
        let request = ChatRequest::new_user("hello", "opaque-7f82");
        let (_, body) = provider.body(request, false).unwrap();
        for key in [
            "reasoning",
            "reasoning_effort",
            "thinking",
            "output_config",
            "include",
        ] {
            assert!(body.get(key).is_none(), "{protocol} injected {key}");
        }
        let headers = provider.headers().unwrap();
        assert!(!headers.contains_key("authorization"));
        assert!(!headers.contains_key("x-api-key"));
        if protocol == LlmProtocol::Messages {
            assert_eq!(headers["anthropic-version"], "2023-06-01");
        }
    }
}

#[test]
fn responses_thinking_switch_does_not_suppress_effort() {
    let mut request = ChatRequest::new_user("hello", "same-model");
    request.reasoning_effort = Some(ReasoningEffort::High);
    request.request_parameters =
        serde_json::from_value(json!({"chat_template_kwargs":{"enable_thinking":true}})).unwrap();
    let (_, body) = provider(LlmProtocol::Responses, ReasoningReplay::ProtocolDefault)
        .body(request, false)
        .unwrap();
    assert_eq!(body["reasoning"], json!({"effort":"high"}));
    assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
    assert_eq!(
        body["chat_template_kwargs"],
        json!({"enable_thinking":true})
    );
}

#[tokio::test]
async fn streamed_chat_reasoning_survives_tool_continuation_for_every_replay_policy() {
    let sse = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"stream thought\"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"type\":\"function\",\"function\":{\"name\":\"echo\",\"arguments\":\"{\\\"text\\\":\\\"hello\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    let terminal = json!({"model":"opaque-7f82","choices":[{"message":{"content":"OK"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}).to_string();
    for (replay, field) in [
        (ReasoningReplay::ProtocolDefault, Some("reasoning")),
        (ReasoningReplay::Reasoning, Some("reasoning")),
        (ReasoningReplay::ReasoningContent, Some("reasoning_content")),
        (ReasoningReplay::Omit, None),
    ] {
        let (url, server) = capture(vec![
            ("text/event-stream", sse.to_owned()),
            ("application/json", terminal.clone()),
        ])
        .await;
        let config: ProviderConfig = serde_json::from_value(json!({"base_url":url,"models":[{"id":"same-model","reasoning":true,"reasoning_replay":replay}]})).unwrap();
        let provider =
            ProtocolProvider::new("opaque-service", LlmProtocol::Chat, &config, "same-model")
                .unwrap();
        let mut stream = provider
            .stream_model_blocks(ChatRequest::new_user("hello", "same-model"))
            .await
            .unwrap();
        let mut reasoning = None;
        let mut tool_seen = false;
        while let Some(event) = stream.next().await {
            match event.unwrap() {
                ModelBlockStreamEvent::ReasoningCompleted { output } => {
                    assert_eq!(output.display_text(), "stream thought");
                    reasoning = output.continuation;
                }
                ModelBlockStreamEvent::BlockStarted(block)
                    if block.kind == crate::ModelBlockKind::ToolCall =>
                {
                    assert_eq!(block.tool_call.unwrap().call_id.as_deref(), Some("call-1"));
                    tool_seen = true;
                }
                _ => {}
            }
        }
        assert!(tool_seen);
        provider
            .chat_with_tools(tool_history(
                reasoning.expect("stream reasoning must remain canonical, including Omit policy"),
            ))
            .await
            .unwrap();
        let requests = server.await.unwrap();
        let assistant = &requests[1].1["messages"][1];
        for key in ["reasoning", "reasoning_content"] {
            assert_eq!(
                assistant.get(key).cloned(),
                if Some(key) == field {
                    Some(json!("stream thought"))
                } else {
                    None
                }
            );
        }
        assert_eq!(assistant["tool_calls"][0]["id"], "call-1");
        assert_eq!(requests[1].1["messages"][2]["tool_call_id"], "call-1");
    }
}

#[tokio::test]
async fn custom_chat_stream_preserves_the_same_structured_reasoning_as_nonstream() {
    for (detail, flattenable) in [
        (
            json!({"type":"reasoning.text","text":"thought","id":"rs-1","index":0}),
            true,
        ),
        (
            json!({"type":"reasoning.text","text":"thought","signature":"","id":"rs-1","index":0}),
            true,
        ),
        (
            json!({"type":"reasoning.text","text":"thought","signature":"signature-evidence","id":"rs-1","index":0}),
            false,
        ),
        (
            json!({"type":"reasoning.encrypted","data":"opaque-evidence","id":"rs-1","index":0}),
            false,
        ),
    ] {
        let message = json!({"content":"OK","reasoning":"duplicate-alias","reasoning_details":[detail.clone()]});
        let nonstream =
            json!({"model":"same-model","choices":[{"message":message,"finish_reason":"stop"}]})
                .to_string();
        let sse = format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({"choices":[{"delta":message,"finish_reason":"stop"}]})
        );
        let (url, server) = capture(vec![
            ("application/json", nonstream),
            ("text/event-stream", sse),
        ])
        .await;
        let config: ProviderConfig = serde_json::from_value(
            json!({"base_url":url,"models":[{"id":"same-model","reasoning":true}]}),
        )
        .unwrap();
        let connection =
            ProtocolProvider::new("opaque-service", LlmProtocol::Chat, &config, "same-model")
                .unwrap();
        let nonstream = connection
            .chat(ChatRequest::new_user("hello", "same-model"))
            .await
            .unwrap();
        let mut stream = connection
            .stream_model_blocks(ChatRequest::new_user("hello", "same-model"))
            .await
            .unwrap();
        let mut streamed = None;
        while let Some(event) = stream.next().await {
            if let ModelBlockStreamEvent::ReasoningCompleted { output } = event.unwrap() {
                streamed = Some(output);
            }
        }
        let streamed = streamed.expect("structured reasoning must reach canonical output");
        assert_eq!(streamed, nonstream.reasoning);
        assert!(!streamed.display_text().contains("duplicate-alias"));
        let continuation = streamed.continuation.unwrap();
        let ReasoningContinuation::Blocks(blocks) = &continuation else {
            panic!("structured input must remain structured");
        };
        match &blocks[0] {
            ReasoningBlock::Text(block) => {
                assert_eq!(block.text, "thought");
                assert_eq!(
                    block.signature,
                    detail
                        .get("signature")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                );
                assert_eq!(block.provider_item_id.as_deref(), Some("rs-1"));
                assert_eq!(block.provider_index, Some(0));
            }
            ReasoningBlock::Encrypted(block) => {
                assert_eq!(block.data, "opaque-evidence");
                assert_eq!(block.provider_item_id.as_deref(), Some("rs-1"));
            }
            ReasoningBlock::Summary(_) => panic!("unexpected summary"),
        }
        let request = tool_history(continuation);
        for replay in [
            ReasoningReplay::ProtocolDefault,
            ReasoningReplay::Reasoning,
            ReasoningReplay::ReasoningContent,
            ReasoningReplay::Omit,
        ] {
            let result = provider(LlmProtocol::Chat, replay).body(request.clone(), false);
            if replay == ReasoningReplay::Omit {
                let (_, body) = result.unwrap();
                assert!(body["messages"][1].get("reasoning").is_none());
                assert!(body["messages"][1].get("reasoning_content").is_none());
                assert_eq!(body["messages"][1]["tool_calls"][0]["id"], "call-1");
                assert_eq!(body["messages"][2]["tool_call_id"], "call-1");
            } else if flattenable {
                assert!(result.is_ok());
            } else {
                let error = result.unwrap_err();
                assert_eq!(error.error_code(), ErrorCode::InvalidRequest);
                assert!(!error.to_string().contains("signature-evidence"));
                assert!(!error.to_string().contains("opaque-evidence"));
            }
        }
        assert_eq!(server.await.unwrap().len(), 2);
    }
}

#[tokio::test]
async fn custom_chat_auto_keeps_plain_alias_fallback_and_plain_structured_boundaries() {
    for alias in ["reasoning", "reasoning_content", "thinking"] {
        let mut delta = json!({"reasoning_details":[]});
        delta[alias] = json!("plain thought");
        let sse = format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({"choices":[{"delta":delta,"finish_reason":"stop"}]})
        );
        let (url, server) = capture(vec![("text/event-stream", sse)]).await;
        let config: ProviderConfig =
            serde_json::from_value(json!({"base_url":url,"models":[{"id":"same-model"}]})).unwrap();
        let connection =
            ProtocolProvider::new("opaque-service", LlmProtocol::Chat, &config, "same-model")
                .unwrap();
        let mut stream = connection
            .stream_model_blocks(ChatRequest::new_user("hello", "same-model"))
            .await
            .unwrap();
        let mut continuation = None;
        while let Some(event) = stream.next().await {
            if let ModelBlockStreamEvent::ReasoningCompleted { output } = event.unwrap() {
                continuation = output.continuation;
            }
        }
        assert_eq!(
            continuation,
            Some(ReasoningContinuation::Text("plain thought".into()))
        );
        server.await.unwrap();
    }
    let deltas = [
        json!({"reasoning_content":"prelude"}),
        json!({"reasoning_details":[{"type":"reasoning.encrypted","data":"opaque","id":"rs-1","index":0}]}),
        json!({"thinking":"suffix"}),
    ];
    let mut sse = deltas
        .iter()
        .map(|delta| format!("data: {}\n\n", json!({"choices":[{"delta":delta}]})))
        .collect::<String>();
    sse.push_str("data: [DONE]\n\n");
    let (url, server) = capture(vec![("text/event-stream", sse)]).await;
    let config: ProviderConfig =
        serde_json::from_value(json!({"base_url":url,"models":[{"id":"same-model"}]})).unwrap();
    let connection =
        ProtocolProvider::new("opaque-service", LlmProtocol::Chat, &config, "same-model").unwrap();
    let mut stream = connection
        .stream_model_blocks(ChatRequest::new_user("hello", "same-model"))
        .await
        .unwrap();
    let mut reasoning = None;
    while let Some(event) = stream.next().await {
        if let ModelBlockStreamEvent::ReasoningCompleted { output } = event.unwrap() {
            reasoning = Some(output);
        }
    }
    let reasoning = reasoning.unwrap();
    assert_eq!(reasoning.display_text(), "preludesuffix");
    let Some(ReasoningContinuation::Blocks(blocks)) = reasoning.continuation else {
        panic!("mixed input must preserve structured continuation");
    };
    assert_eq!(blocks.len(), 3);
    assert!(matches!(&blocks[0],ReasoningBlock::Text(block) if block.text=="prelude"));
    assert!(matches!(&blocks[1],ReasoningBlock::Encrypted(block) if block.data=="opaque"));
    assert!(matches!(&blocks[2],ReasoningBlock::Text(block) if block.text=="suffix"));
    server.await.unwrap();
}

#[test]
fn declared_efforts_preserve_independent_thinking_switch() {
    let mut provider = provider(LlmProtocol::Chat, ReasoningReplay::ProtocolDefault);
    provider
        .models
        .get_mut("same-model")
        .unwrap()
        .reasoning_efforts = Some(vec![
        ReasoningEffort::Light,
        ReasoningEffort::Medium,
        ReasoningEffort::XHigh,
    ]);
    for enabled in [false, true] {
        let mut request = ChatRequest::new_user("hello", "same-model");
        request.reasoning_effort = Some(ReasoningEffort::XHigh);
        request.request_parameters = serde_json::from_value(
            json!({"chat_template_kwargs":{"enable_thinking":enabled,"sibling":7}}),
        )
        .unwrap();
        let (_, body) = provider.body(request, false).unwrap();
        assert_eq!(body["reasoning_effort"], "xhigh");
        assert_eq!(
            body["chat_template_kwargs"],
            json!({"enable_thinking":enabled,"sibling":7})
        );
    }
}
