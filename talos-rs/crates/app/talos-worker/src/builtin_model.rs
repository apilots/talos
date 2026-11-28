//! Worker-owned model selection shared by built-in Memory workloads.

use std::sync::{Arc, RwLock};

use talos_error::TalosResult;
use talos_llm_provider::{ChatMessage, ChatRequest, ChatResponse, LlmProviderRouter};

pub(crate) async fn chat(
    config: &RwLock<talos_config::Config>,
    router: &Arc<dyn LlmProviderRouter>,
    messages: Vec<ChatMessage>,
) -> TalosResult<(ChatResponse, agent_protocol::AgentMemoryModelUsage)> {
    let (model, timezone) = {
        let config = config.read().expect("Worker config lock poisoned");
        let default = talos_types::resolve_default_model_selection(&config.llm)?;
        (
            talos_types::resolve_inner_model_ref(
                config.agent.inner_model.as_ref(),
                &default.model_ref,
                &talos_types::resolve_model_catalog(&config.llm),
            )?,
            config.timezone.clone(),
        )
    };
    let response = router
        .provider_for(&model)?
        .chat(
            ChatRequest::builder()
                .model(model.id.clone())
                .messages(messages)
                .build(),
        )
        .await?;
    let usage = agent_protocol::AgentMemoryModelUsage {
        request_id: uuid::Uuid::new_v4().to_string(),
        model_ref: model,
        usage: talos_types::TokenUsage::from(&response.usage),
        completed_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock follows epoch")
            .as_secs() as i64,
        timezone,
    };
    Ok((response, usage))
}
