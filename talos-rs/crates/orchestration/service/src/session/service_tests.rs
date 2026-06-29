use std::sync::Arc;

use talos_store::project::MockProjectStore;
use talos_store::session::MockSessionStore;
use talos_types::talos::{TalosContext, TalosProvider, TalosSourceType};

use super::SessionService;

fn provider_model(id: &str) -> talos_config::ProviderModelConfig {
    talos_config::ProviderModelConfig {
        id: id.to_string(),
        name: id.to_string(),
        context_window: 32_768,
        reasoning: false,
        reasoning_effort: Default::default(),
        speed: Default::default(),
        input: Vec::new(),
        cost: Default::default(),
        compat: Default::default(),
    }
}

fn context() -> TalosContext {
    TalosContext {
        source_type: TalosSourceType::System,
        provider: TalosProvider::Http,
        user_id: "user-1".to_string(),
        message_id: "message-1".to_string(),
        trace_id: "trace-1".to_string(),
        channel_id: None,
        thread_id: None,
        project_root: None,
        environment: None,
    }
}

#[tokio::test]
async fn update_base_llm_changes_new_session_default_model() {
    let mut base_config = talos_types::TalosConfig::default();
    base_config.llm.provider = "anthropic".to_string();
    base_config.llm.model = "old-model".to_string();
    let service = SessionService::new(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        base_config,
    );
    let mut llm = talos_config::LlmConfig::default();
    llm.provider = "openai".to_string();
    llm.model = "new-model".to_string();

    service.update_base_llm(&llm);
    let session = service
        .create_session(None, context(), "default", None, None, None, None)
        .await
        .expect("session should be created");

    assert_eq!(session.config.llm.provider, "openai");
    assert_eq!(session.config.llm.model, "new-model");
}

#[tokio::test]
async fn requested_session_model_wins_when_provider_is_unchanged() {
    let mut base_config = talos_types::TalosConfig::default();
    base_config.llm.provider = "anthropic".to_string();
    base_config.llm.model = "old-default".to_string();
    base_config.llm.anthropic.models = vec![provider_model("old-default")];
    let service = SessionService::new(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        base_config,
    );
    let mut llm = talos_config::LlmConfig::default();
    llm.provider = "anthropic".to_string();
    llm.model = "new-default".to_string();
    llm.anthropic.models = vec![
        provider_model("new-default"),
        provider_model("session-model"),
    ];

    service.update_base_llm(&llm);
    let session = service
        .create_session(
            None,
            context(),
            "default",
            Some("session-model".to_string()),
            None,
            None,
            None,
        )
        .await
        .expect("session should be created");

    assert_eq!(session.config.llm.provider, "anthropic");
    assert_eq!(session.config.llm.model, "session-model");
}

#[tokio::test]
async fn provider_switch_resets_existing_session_model_to_new_default() {
    let mut base_config = talos_types::TalosConfig::default();
    base_config.llm.provider = "anthropic".to_string();
    base_config.llm.model = "old-default".to_string();
    base_config.llm.anthropic.models = vec![
        provider_model("old-default"),
        provider_model("session-model"),
    ];
    let service = SessionService::new(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        base_config,
    );

    let original = service
        .create_session(
            None,
            context(),
            "default",
            Some("session-model".to_string()),
            None,
            None,
            None,
        )
        .await
        .expect("session should be created");
    assert_eq!(original.config.llm.model, "session-model");

    let mut llm = talos_config::LlmConfig::default();
    llm.provider = "openai".to_string();
    llm.model = "new-default".to_string();
    llm.openai.models = vec![provider_model("new-default")];
    service.update_base_llm(&llm);

    let reloaded = service
        .get_session(&original.context.session_id)
        .await
        .expect("session read should succeed")
        .expect("session should exist");

    assert_eq!(reloaded.config.llm.provider, "openai");
    assert_eq!(reloaded.config.llm.model, "new-default");
}
