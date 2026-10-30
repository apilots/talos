use super::*;

#[test]
fn custom_connections_and_nested_parameters_roundtrip_without_losing_identity() {
    let llm: LlmConfig = serde_json::from_value(serde_json::json!({
        "provider":"llm-gateway", "model":"model", "custom_providers": {
            "llm-gateway": {"name":"LLM Gateway","http_timeouts":{"connect_secs":5,"response_secs":90,"stream_idle_secs":60,"stream_total_secs":600},"protocol":"chat","base_url":"http://localhost:8000/v1","models":[{"id":"model","request_parameters":{"chat_template_kwargs":{"enable_thinking":true},"thinking_budget":2048,"extra":[null,false,"x"]}}]},
            "local-inference": {"name":"Local Inference","protocol":"chat","base_url":"http://localhost:8001/v1","models":[{"id":"model"}]}
        }
    })).unwrap();
    assert_eq!(decode_llm(encode_llm(llm.clone())).unwrap(), llm);
}

#[test]
fn configured_reasoning_replay_survives_all_model_projections() {
    for replay in ["protocol_default", "omit", "reasoning", "reasoning_content"] {
        let llm: LlmConfig = serde_json::from_value(serde_json::json!({
            "provider":"custom-service", "model":"opaque-7f82", "custom_providers": {
                "custom-service": {"name":"Connection", "protocol":"chat", "base_url":"http://localhost:8000/v1", "models":[{"id":"opaque-7f82", "reasoning_replay":replay}]}
            }
        })).unwrap();
        let decoded = decode_llm(encode_llm(llm.clone())).unwrap();
        assert_eq!(decoded, llm);
        assert_eq!(
            decoded.custom_providers["custom-service"].config.models[0].reasoning_replay,
            llm.custom_providers["custom-service"].config.models[0].reasoning_replay
        );
    }
}

#[test]
fn reasoning_effort_declarations_preserve_absent_empty_and_explicit_over_rpc() {
    for efforts in [
        serde_json::Value::Null,
        serde_json::json!([]),
        serde_json::json!(["light", "medium", "xhigh"]),
    ] {
        let llm: LlmConfig = serde_json::from_value(serde_json::json!({
            "provider":"custom-service", "model":"opaque", "custom_providers": {
                "custom-service":{"name":"Connection","protocol":"chat","base_url":"http://localhost:8000/v1","models":[{"id":"opaque","reasoning":true,"reasoning_efforts":efforts}]}
            }
        })).unwrap();
        assert_eq!(decode_llm(encode_llm(llm.clone())).unwrap(), llm);
    }
}
