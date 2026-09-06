use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_llm_provider::{
    ChatMessage, ChatRequest, ChatResponse, ChatStream, LlmProvider, LlmProviderRouter,
    ReasoningOutput, Usage,
};
use talos_store::profile::{MockProfileStore, ProfileUsageUpdate};
use talos_types::{ModelInfo, ModelRef, ModelUsageKind};

use super::{resolve_inner_model_ref, InnerModelChatParams, InnerModelClient};

struct TestProvider {
    models: Mutex<Vec<String>>,
    fail_chat: AtomicBool,
}

#[async_trait]
impl LlmProvider for TestProvider {
    fn name(&self) -> &str {
        "test"
    }

    fn default_model(&self) -> String {
        "inner-mini".to_string()
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        if self.fail_chat.load(Ordering::Relaxed) {
            return Err(TalosError::new(
                ErrorCode::ProviderUnavailable,
                "injected provider failure",
            ));
        }
        self.models.lock().unwrap().push(request.model.clone());
        Ok(ChatResponse {
            content: "ok".to_string(),
            reasoning: ReasoningOutput::default(),
            model: request.model,
            usage: Usage {
                prompt_tokens: 11,
                completion_tokens: 3,
                total_tokens: 14,
                cache_creation_input_tokens: Some(2),
                cache_read_input_tokens: Some(4),
            },
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        Ok(ChatStream { rx })
    }
}

struct TestRouter {
    refs: Mutex<Vec<ModelRef>>,
    provider: Arc<dyn LlmProvider>,
}

impl LlmProviderRouter for TestRouter {
    fn provider_for(&self, model_ref: &ModelRef) -> TalosResult<Arc<dyn LlmProvider>> {
        self.refs.lock().unwrap().push(model_ref.clone());
        Ok(self.provider.clone())
    }
}

#[tokio::test]
async fn client_routes_reloads_and_propagates_failures() {
    let provider = Arc::new(TestProvider {
        models: Mutex::new(Vec::new()),
        fail_chat: AtomicBool::new(false),
    });
    let router = Arc::new(TestRouter {
        refs: Mutex::new(Vec::new()),
        provider: provider.clone(),
    });
    let profiles = Arc::new(MockProfileStore::default());
    let configured = ModelRef {
        provider: "deepseek".to_string(),
        id: "inner-mini".to_string(),
    };
    let model_catalog = vec![ModelInfo::from_provider_model(
        "deepseek",
        serde_json::from_value(serde_json::json!({"id": "inner-mini"})).unwrap(),
    )];
    let client = InnerModelClient::builder()
        .configured_model(Some(configured.clone()))
        .agent_default_model(ModelRef {
            provider: "anthropic".to_string(),
            id: "agent-default".to_string(),
        })
        .model_catalog(model_catalog.clone())
        .provider_router(router.clone())
        .profile_store(profiles.clone())
        .profile_timezone("UTC".to_string())
        .build()
        .unwrap();

    client
        .chat(InnerModelChatParams {
            user_id: "user-1".to_string(),
            session_id: Some("session-1".to_string()),
            usage_kind: ModelUsageKind::Dream,
            messages: vec![ChatMessage::user("remember this")],
        })
        .await
        .unwrap();

    let reloaded = ModelRef {
        provider: "deepseek".to_string(),
        id: "inner-mini-2".to_string(),
    };
    client.reload(reloaded.clone());
    client
        .chat(InnerModelChatParams {
            user_id: "user-1".to_string(),
            session_id: Some("session-1".to_string()),
            usage_kind: ModelUsageKind::Memory,
            messages: vec![ChatMessage::user("update memory")],
        })
        .await
        .unwrap();
    let invalid_reload = ModelRef {
        provider: "deepseek".to_string(),
        id: "unknown-inner-model".to_string(),
    };
    assert!(resolve_inner_model_ref(Some(&invalid_reload), &configured, &model_catalog).is_err());
    assert_eq!(client.model_ref(), reloaded);

    let updates = profiles.updates();
    assert_eq!(updates.len(), 2);
    let ProfileUsageUpdate::ModelCall(update) = &updates[0] else {
        panic!("expected model-call usage");
    };
    let ProfileUsageUpdate::ModelCall(reloaded_update) = &updates[1] else {
        panic!("expected reloaded model-call usage");
    };
    assert_eq!(
        (
            router.refs.lock().unwrap().clone(),
            provider.models.lock().unwrap().clone(),
            update.user_id.clone(),
            update.kind,
            update.model_ref.clone(),
            update.usage.clone(),
            reloaded_update.kind,
            reloaded_update.model_ref.clone(),
        ),
        (
            vec![configured.clone(), configured.clone(), reloaded.clone()],
            vec!["inner-mini".to_string(), "inner-mini-2".to_string()],
            "user-1".to_string(),
            ModelUsageKind::Dream,
            configured,
            talos_types::TokenUsage {
                input_tokens: 11,
                output_tokens: 3,
                total_tokens: 14,
                cache_creation_input_tokens: Some(2),
                cache_read_input_tokens: Some(4),
            },
            ModelUsageKind::Memory,
            reloaded,
        )
    );

    provider.fail_chat.store(true, Ordering::Relaxed);
    let provider_error = client
        .chat(InnerModelChatParams {
            user_id: "user-1".to_string(),
            session_id: None,
            usage_kind: ModelUsageKind::Title,
            messages: vec![ChatMessage::user("title")],
        })
        .await
        .expect_err("provider failure should be returned");
    assert!(provider_error.is_code(ErrorCode::ProviderUnavailable));
    assert_eq!(profiles.updates().len(), 2);

    provider.fail_chat.store(false, Ordering::Relaxed);
    profiles.set_record_usage_failure(true);
    let metrics_error = client
        .chat(InnerModelChatParams {
            user_id: "user-1".to_string(),
            session_id: None,
            usage_kind: ModelUsageKind::Memory,
            messages: vec![ChatMessage::user("memory")],
        })
        .await
        .expect_err("metrics failure should be returned");
    assert!(metrics_error.is_code(ErrorCode::InternalError));
    assert_eq!(profiles.updates().len(), 2);
}
