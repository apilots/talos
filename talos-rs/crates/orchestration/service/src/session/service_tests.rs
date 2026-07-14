use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use talos_error::TalosResult;
use talos_store::project::MockProjectStore;
use talos_store::session::{MockSessionStore, SessionQuery, SessionRecord, SessionStore};
use talos_types::talos::{TalosContext, TalosProvider, TalosSourceType};
use talos_types::{ModelRef, ModelSelection, SessionConf};

use super::{CreateSessionParams, SessionService};

#[derive(Default)]
struct RecordingSessionStore {
    records: Mutex<Vec<SessionRecord>>,
}

#[async_trait]
impl SessionStore for RecordingSessionStore {
    async fn insert_session_record(
        &self,
        record: &SessionRecord,
        _agent_id: &str,
        _scope: &str,
    ) -> TalosResult<()> {
        self.records.lock().unwrap().push(record.clone());
        Ok(())
    }

    async fn get_session_record(&self, session_id: &str) -> TalosResult<Option<SessionRecord>> {
        Ok(self
            .records
            .lock()
            .unwrap()
            .iter()
            .find(|record| record.id == session_id)
            .cloned())
    }

    async fn list_session_records(&self, _query: &SessionQuery) -> TalosResult<Vec<SessionRecord>> {
        Ok(self.records.lock().unwrap().clone())
    }

    async fn count_session_records(&self, _query: &SessionQuery) -> TalosResult<usize> {
        Ok(self.records.lock().unwrap().len())
    }

    async fn update_session_record(&self, record: &SessionRecord) -> TalosResult<bool> {
        let mut records = self.records.lock().unwrap();
        let Some(stored) = records.iter_mut().find(|stored| stored.id == record.id) else {
            return Ok(false);
        };
        *stored = record.clone();
        Ok(true)
    }

    async fn delete_session_record(&self, session_id: &str) -> TalosResult<bool> {
        let mut records = self.records.lock().unwrap();
        let len_before = records.len();
        records.retain(|record| record.id != session_id);
        Ok(records.len() != len_before)
    }
}

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
    }
}

fn config_with_model(provider: &str, model: &str) -> talos_types::TalosConfig {
    let mut config = talos_types::TalosConfig::default();
    config.llm.provider = provider.to_string();
    config.llm.model = model.to_string();
    let models = match provider {
        "anthropic" => &mut config.llm.anthropic.models,
        "openai" => &mut config.llm.openai.models,
        other => panic!("unsupported test provider {other}"),
    };
    models.push(provider_model(model));
    config
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

fn create_session_params(
    context: TalosContext,
    target_agent: &str,
    model_selection: Option<ModelSelection>,
) -> CreateSessionParams {
    CreateSessionParams {
        session_id: None,
        context,
        target_agent: target_agent.to_string(),
        model_selection,
        conf: None,
        workspace_id: None,
        project_id: None,
        current_directory: None,
    }
}

#[tokio::test]
async fn create_session_without_selection_uses_global_default() {
    let base_config = config_with_model("anthropic", "global-model");
    let service = SessionService::new(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        base_config,
    );
    let session = service
        .create_session(create_session_params(context(), "default", None))
        .await
        .expect("session should be created");

    assert_eq!(session.config.llm.provider, "anthropic");
    assert_eq!(session.config.llm.model, "global-model");
    assert_eq!(
        session.conf.model_selection,
        Some(ModelSelection::without_reasoning(ModelRef {
            provider: "anthropic".to_string(),
            id: "global-model".to_string(),
        }))
    );
}

#[tokio::test]
async fn create_session_persists_configured_default_reasoning_effort() {
    let mut config = config_with_model("anthropic", "reasoning-model");
    config.llm.anthropic.models[0].reasoning = true;
    config.llm.anthropic.models[0].reasoning_effort = talos_config::ReasoningEffort::High;
    let service = SessionService::new(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        config,
    );

    let session = service
        .create_session(create_session_params(context(), "default", None))
        .await
        .unwrap();

    assert_eq!(
        session.conf.model_selection.unwrap().reasoning_effort,
        Some(talos_config::ReasoningEffort::High)
    );
}

#[tokio::test]
async fn update_base_llm_changes_new_session_default_model() {
    let base_config = config_with_model("anthropic", "old-model");
    let service = SessionService::new(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        base_config,
    );
    let llm = talos_config::LlmConfig {
        provider: "openai".to_string(),
        model: "new-model".to_string(),
        openai: talos_config::ProviderConfig {
            models: vec![provider_model("new-model")],
            ..Default::default()
        },
        ..Default::default()
    };

    service.update_base_llm(&llm);
    let session = service
        .create_session(create_session_params(context(), "default", None))
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
    let mut llm = talos_config::LlmConfig {
        provider: "anthropic".to_string(),
        model: "new-default".to_string(),
        ..Default::default()
    };
    llm.anthropic.models = vec![
        provider_model("new-default"),
        provider_model("session-model"),
    ];

    service.update_base_llm(&llm);
    let session = service
        .create_session(create_session_params(
            context(),
            "default",
            Some(ModelSelection::without_reasoning(ModelRef {
                provider: "anthropic".to_string(),
                id: "session-model".to_string(),
            })),
        ))
        .await
        .expect("session should be created");

    assert_eq!(session.config.llm.provider, "anthropic");
    assert_eq!(session.config.llm.model, "session-model");
}

#[tokio::test]
async fn create_session_conf_model_selection_matches_stored_model() {
    let mut base_config = config_with_model("anthropic", "default-model");
    base_config.llm.openai.models = vec![provider_model("conf-model")];
    let store = Arc::new(RecordingSessionStore::default());
    let service = SessionService::new(
        store.clone(),
        Arc::new(MockProjectStore::new()),
        base_config,
    );
    let mut params = create_session_params(context(), "default", None);
    params.conf = Some(SessionConf {
        model_selection: Some(ModelSelection::without_reasoning(ModelRef {
            provider: "openai".to_string(),
            id: "conf-model".to_string(),
        })),
    });

    let session = service
        .create_session(params)
        .await
        .expect("session should be created");

    assert_eq!(session.config.llm.provider, "openai");
    assert_eq!(session.config.llm.model, "conf-model");
    let records = store.records.lock().unwrap();
    assert_eq!(records[0].model, "conf-model");
}

#[tokio::test]
async fn existing_session_keeps_persisted_selection_when_default_changes() {
    let base_config = config_with_model("anthropic", "old-default");
    let store = Arc::new(RecordingSessionStore::default());
    let service = SessionService::new(store, Arc::new(MockProjectStore::new()), base_config);

    let original = service
        .create_session(create_session_params(context(), "default", None))
        .await
        .expect("session should be created");
    assert_eq!(original.config.llm.provider, "anthropic");
    assert_eq!(original.config.llm.model, "old-default");

    let mut llm = talos_config::LlmConfig {
        provider: "anthropic".to_string(),
        model: "new-default".to_string(),
        ..Default::default()
    };
    llm.anthropic.models = vec![provider_model("old-default"), provider_model("new-default")];
    service.update_base_llm(&llm);

    let reloaded = service
        .get_session(&original.context.session_id)
        .await
        .expect("session read should succeed")
        .expect("session should exist");

    assert_eq!(reloaded.config.llm.provider, "anthropic");
    assert_eq!(reloaded.config.llm.model, "old-default");
}

#[tokio::test]
async fn session_without_explicit_model_selection_is_rejected_without_writing() {
    let store = Arc::new(RecordingSessionStore::default());
    let service = SessionService::new(
        store.clone(),
        Arc::new(MockProjectStore::new()),
        config_with_model("anthropic", "current-model"),
    );
    let created = service
        .create_session(create_session_params(context(), "default", None))
        .await
        .unwrap();
    {
        let mut records = store.records.lock().unwrap();
        records[0].conf = SessionConf::default();
    }

    let error = service
        .get_session(&created.context.session_id)
        .await
        .expect_err("persisted session without model selection must fail");

    assert!(error
        .to_string()
        .contains("incompatible persisted session format"));
    assert_eq!(
        store.records.lock().unwrap()[0].conf,
        SessionConf::default(),
        "failed reads must not mutate persisted session state"
    );
}

#[tokio::test]
async fn create_session_preserves_workspace_routing_key() {
    let service = SessionService::new(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        config_with_model("anthropic", "claude-3-5-sonnet"),
    );

    let session = service
        .create_session(CreateSessionParams {
            workspace_id: Some("workspace-alpha".to_string()),
            ..create_session_params(context(), "default", None)
        })
        .await
        .expect("session should be created");

    assert_eq!(
        session.context.workspace_id,
        Some("workspace-alpha".to_string())
    );
}
